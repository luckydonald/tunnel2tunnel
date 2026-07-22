//! fail2ban-style ban rules + round-robined SSH tarpit methods.
//!
//! All new tarpit logic lives under this module (never inline in `lib.rs`),
//! per project convention. Sibling modules implement the three tarpit
//! methods; this module owns the shared in-memory ban/threshold state and
//! the decision logic for which method (if any) applies to a connection.

pub mod banner_drip;
pub mod fake_shell;
pub mod slow_auth;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::PgPool;
use tokio::sync::Mutex;
use uuid::Uuid;

use tunnel2tunnel_core::models::{
    ban_rule::BanRule, connection_log::ConnectionLog, settings::Settings,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TarpitMethod {
    BannerDrip,
    SlowAuth,
    FakeShell,
}

impl TarpitMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BannerDrip => "banner_drip",
            Self::SlowAuth => "slow_auth",
            Self::FakeShell => "fake_shell",
        }
    }

    /// Deterministic round-robin over the three methods, driven by how many
    /// times this identity has crossed the ban threshold. Pure and
    /// DB/network-free so it's directly unit-testable.
    pub fn round_robin(trigger_count: u64) -> Self {
        match trigger_count % 3 {
            0 => Self::BannerDrip,
            1 => Self::SlowAuth,
            _ => Self::FakeShell,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BanUntil {
    NotBanned,
    At(Instant),
    Forever,
}

impl BanUntil {
    fn is_active(self, now: Instant) -> bool {
        match self {
            BanUntil::NotBanned => false,
            BanUntil::At(t) => now < t,
            BanUntil::Forever => true,
        }
    }
}

#[derive(Debug)]
pub(crate) struct TarpitEntry {
    fail_count: u32,
    window_start: Instant,
    trigger_count: u64,
    /// Once a success is recorded for this key (peer_ip only — user keys
    /// never drive banner-drip), this is permanently set to `false`: banner
    /// drip must never be used against an identity known to have logged in
    /// successfully, since it intercepts before russh/auth ever runs.
    banner_drip_eligible: bool,
    banned: BanUntil,
}

impl TarpitEntry {
    fn new(now: Instant) -> Self {
        Self {
            fail_count: 0,
            window_start: now,
            trigger_count: 0,
            banner_drip_eligible: true,
            banned: BanUntil::NotBanned,
        }
    }

    fn is_banned(&self, now: Instant) -> bool {
        self.banned.is_active(now)
    }
}

/// Key: peer_ip directly, or `user:{uuid}` for user-scoped state.
pub type TarpitState = Arc<Mutex<HashMap<String, TarpitEntry>>>;

pub fn new_state() -> TarpitState {
    Arc::new(Mutex::new(HashMap::new()))
}

fn user_key(user_id: Uuid) -> String {
    format!("user:{user_id}")
}

#[derive(Debug, Clone)]
pub struct Thresholds {
    pub count: u32,
    pub window: Duration,
    pub enabled: bool,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            count: 5,
            window: Duration::from_secs(600),
            enabled: true,
        }
    }
}

pub type SharedThresholds = Arc<Mutex<Thresholds>>;

// ── Pure, DB/network-free logic — the part covered by fast unit tests ──────

/// Records one failed attempt for `key`. Returns whether `key` is banned
/// after this call. Crossing the threshold sets a time-bounded ban (auto
/// bans are never indefinite — only admin-created `ban_rules` can be) and
/// bumps `trigger_count`, which drives the next round-robin method pick.
fn record_failure(
    map: &mut HashMap<String, TarpitEntry>,
    key: &str,
    thresholds: &Thresholds,
    now: Instant,
) -> bool {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| TarpitEntry::new(now));

    if now.duration_since(entry.window_start) > thresholds.window {
        entry.fail_count = 0;
        entry.window_start = now;
    }

    entry.fail_count += 1;

    if entry.fail_count >= thresholds.count {
        entry.trigger_count += 1;
        entry.banned = BanUntil::At(now + thresholds.window);
        entry.fail_count = 0;
        entry.window_start = now;
    }

    entry.is_banned(now)
}

/// A successful login resets the failure tally (so future failures start
/// counting fresh) but deliberately does NOT lift an already-active ban —
/// success only guarantees the *current* session isn't interrupted (which is
/// automatic: the success path never routes through a tarpit branch), not
/// that a standing ban is forgiven for future connections from the same
/// peer_ip/user.
fn clear_fail_tally_on_success(map: &mut HashMap<String, TarpitEntry>, key: &str, now: Instant) {
    if let Some(entry) = map.get_mut(key) {
        entry.fail_count = 0;
        entry.window_start = now;
    }
}

// ── Async, DB-touching wrappers used from lib.rs ────────────────────────────

pub async fn record_auth_failure(
    state: &TarpitState,
    peer_ip: &str,
    user_id: Option<Uuid>,
    thresholds: &Thresholds,
) {
    if !thresholds.enabled {
        return;
    }
    let now = Instant::now();
    let mut map = state.lock().await;
    record_failure(&mut map, peer_ip, thresholds, now);
    if let Some(uid) = user_id {
        record_failure(&mut map, &user_key(uid), thresholds, now);
    }
}

pub async fn record_auth_success(state: &TarpitState, peer_ip: &str, user_id: Option<Uuid>) {
    let now = Instant::now();
    let mut map = state.lock().await;
    clear_fail_tally_on_success(&mut map, peer_ip, now);
    if let Some(entry) = map.get_mut(peer_ip) {
        entry.banner_drip_eligible = false;
    }
    if let Some(uid) = user_id {
        clear_fail_tally_on_success(&mut map, &user_key(uid), now);
    }
}

/// Called once per accepted TCP connection, before the socket is handed to
/// russh (or drip-fed directly). Only ever returns `BannerDrip` or `None` —
/// slow-auth/fake-shell are decided later, inside the `Handler` callbacks,
/// once we know which auth method is actually being attempted.
pub async fn decide_pre_auth_tarpit(
    state: &TarpitState,
    pool: &PgPool,
    peer_ip: &str,
) -> Option<TarpitMethod> {
    let now = Instant::now();
    let (banned, trigger_count, cached_ineligible) = {
        let map = state.lock().await;
        match map.get(peer_ip) {
            Some(e) if e.is_banned(now) => (true, e.trigger_count, !e.banner_drip_eligible),
            _ => (false, 0, false),
        }
    };
    if !banned {
        return None;
    }

    let mut method = TarpitMethod::round_robin(trigger_count);
    if method == TarpitMethod::BannerDrip {
        let ineligible = cached_ineligible
            || ConnectionLog::peer_ip_has_known_good_history(pool, peer_ip)
                .await
                .unwrap_or(false);
        if ineligible {
            let mut map = state.lock().await;
            map.entry(peer_ip.to_string())
                .or_insert_with(|| TarpitEntry::new(now))
                .banner_drip_eligible = false;
            method = TarpitMethod::SlowAuth;
        }
    }
    Some(method)
}

/// Called from inside an auth `Handler` callback (password/keyboard-interactive/
/// publickey) once we already know russh is driving this connection — banner
/// drip is never selectable here since the real SSH-2.0 line has already gone
/// out by the time any `Handler` callback runs.
pub async fn decide_in_auth_tarpit(
    state: &TarpitState,
    peer_ip: &str,
    user_id: Option<Uuid>,
) -> Option<TarpitMethod> {
    let now = Instant::now();
    let map = state.lock().await;
    let mut banned = false;
    let mut trigger_count = 0u64;
    if let Some(e) = map.get(peer_ip) {
        if e.is_banned(now) {
            banned = true;
            trigger_count = trigger_count.max(e.trigger_count);
        }
    }
    if let Some(uid) = user_id {
        if let Some(e) = map.get(&user_key(uid)) {
            if e.is_banned(now) {
                banned = true;
                trigger_count = trigger_count.max(e.trigger_count);
            }
        }
    }
    if !banned {
        return None;
    }
    Some(match TarpitMethod::round_robin(trigger_count) {
        TarpitMethod::BannerDrip => TarpitMethod::SlowAuth,
        other => other,
    })
}

/// Periodically refreshes admin-configurable thresholds and merges active
/// `ban_rules` rows into the in-memory tarpit state, so webui edits take
/// effect without a server restart.
pub fn spawn_settings_refresher(pool: PgPool, state: TarpitState, thresholds: SharedThresholds) {
    tokio::spawn(async move {
        loop {
            refresh_once(&pool, &state, &thresholds).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

async fn refresh_once(pool: &PgPool, state: &TarpitState, thresholds: &SharedThresholds) {
    let count = Settings::get(pool, "tarpit_threshold_count")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(5);
    let window_secs = Settings::get(pool, "tarpit_threshold_window_seconds")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    let enabled = Settings::get(pool, "tarpit_enabled")
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(true);

    {
        let mut t = thresholds.lock().await;
        t.count = count;
        t.window = Duration::from_secs(window_secs);
        t.enabled = enabled;
    }

    let Ok(rules) = BanRule::list_active(pool).await else {
        return;
    };

    let now_wall = time::OffsetDateTime::now_utc();
    let now_mono = Instant::now();
    let mut map = state.lock().await;
    for rule in rules {
        let key = match rule.scope_type.as_str() {
            "peer_ip" => rule.peer_ip.clone(),
            "user" => rule.user_id.map(user_key),
            _ => None,
        };
        let Some(key) = key else { continue };
        let banned = match rule.active_until {
            None => BanUntil::Forever,
            Some(active_until) => {
                let remaining = active_until - now_wall;
                if remaining.is_negative() {
                    continue;
                }
                BanUntil::At(now_mono + Duration::from_secs(remaining.whole_seconds().max(0) as u64))
            }
        };
        map.entry(key).or_insert_with(|| TarpitEntry::new(now_mono)).banned = banned;
    }
}

/// Tarpit-triggered rejections reuse the *exact existing* fail2ban line shape
/// (sentinel username `tarpit`) so `contrib/fail2ban/filter.d/tunnel2tunnel.conf`
/// needs no changes and an external fail2ban deployment gets defense-in-depth
/// for free.
pub fn fail2ban_tarpit_line(timestamp: &str, peer_ip: &str) -> String {
    format!(
        "{timestamp} t2t sshd[0]: Failed publickey for invalid user tarpit from {peer_ip} port 0 ssh2\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_cycles() {
        assert_eq!(TarpitMethod::round_robin(0), TarpitMethod::BannerDrip);
        assert_eq!(TarpitMethod::round_robin(1), TarpitMethod::SlowAuth);
        assert_eq!(TarpitMethod::round_robin(2), TarpitMethod::FakeShell);
        assert_eq!(TarpitMethod::round_robin(3), TarpitMethod::BannerDrip);
        assert_eq!(TarpitMethod::round_robin(4), TarpitMethod::SlowAuth);
    }

    fn thresholds() -> Thresholds {
        Thresholds { count: 3, window: Duration::from_secs(60), enabled: true }
    }

    #[test]
    fn record_failure_not_banned_below_threshold() {
        let mut map = HashMap::new();
        let t = thresholds();
        let now = Instant::now();
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert_eq!(map["1.2.3.4"].trigger_count, 0);
    }

    #[test]
    fn record_failure_bans_at_threshold() {
        let mut map = HashMap::new();
        let t = thresholds();
        let now = Instant::now();
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert!(record_failure(&mut map, "1.2.3.4", &t, now));
        assert_eq!(map["1.2.3.4"].trigger_count, 1);
    }

    #[test]
    fn record_failure_window_resets_stale_count() {
        let mut map = HashMap::new();
        let t = thresholds();
        let now = Instant::now();
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        // Well outside the 60s window — should reset instead of accumulating.
        let later = now + Duration::from_secs(120);
        assert!(!record_failure(&mut map, "1.2.3.4", &t, later));
        assert_eq!(map["1.2.3.4"].fail_count, 1);
    }

    #[test]
    fn success_clears_tally_but_not_existing_ban() {
        let mut map = HashMap::new();
        let t = thresholds();
        let now = Instant::now();
        record_failure(&mut map, "1.2.3.4", &t, now);
        record_failure(&mut map, "1.2.3.4", &t, now);
        record_failure(&mut map, "1.2.3.4", &t, now);
        assert!(map["1.2.3.4"].is_banned(now));

        clear_fail_tally_on_success(&mut map, "1.2.3.4", now);
        assert_eq!(map["1.2.3.4"].fail_count, 0);
        // Ban itself must survive a success — success only guarantees the
        // *current* session isn't interrupted, not that the ban is lifted.
        assert!(map["1.2.3.4"].is_banned(now));
    }

    #[test]
    fn banner_drip_eligibility_flips_once_and_stays() {
        let mut map = HashMap::new();
        let now = Instant::now();
        map.insert("1.2.3.4".to_string(), TarpitEntry::new(now));
        assert!(map["1.2.3.4"].banner_drip_eligible);
        map.get_mut("1.2.3.4").unwrap().banner_drip_eligible = false;
        assert!(!map["1.2.3.4"].banner_drip_eligible);
    }

    #[test]
    fn fail2ban_tarpit_line_matches_filter_regex() {
        let re = regex::Regex::new(
            r"^.+ t2t sshd\[\d+\]: Failed publickey for invalid user .+ from \S+ port \d+ ssh2$",
        )
        .unwrap();
        let line = fail2ban_tarpit_line("Jan  1 00:00:00", "203.0.113.5");
        assert!(re.is_match(line.trim_end()), "line did not match filter regex: {line:?}");
    }
}
