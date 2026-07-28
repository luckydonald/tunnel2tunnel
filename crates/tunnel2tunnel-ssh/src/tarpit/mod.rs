//! fail2ban-style ban rules + round-robined SSH tarpit methods.
//!
//! All new tarpit logic lives under this module (never inline in `lib.rs`),
//! per project convention. Sibling modules implement the three tarpit
//! methods; this module owns the shared in-memory ban/threshold state and
//! the decision logic for which action (trap or ban, and if trap, which
//! method) applies to a connection.

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
    tarpit_threshold::TarpitThreshold,
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

/// Which row decided an active ban — a tripped `tarpit_thresholds` rule, or
/// an admin-created `ban_rules` row (the admin is reached via
/// `ban_rules.created_by`, no direct reference needed here).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BanSource {
    Threshold(Uuid),
    AdminRule(Uuid),
}

/// The action actually resolved for a connection attempt right now — looked
/// up from whichever row's `action` (`trap`/`ban`) is currently configured,
/// so editing a rule takes effect on the next attempt without restarting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TarpitOutcome {
    None,
    Trap {
        method: TarpitMethod,
        source: Option<BanSource>,
    },
    Ban {
        source: BanSource,
    },
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
    /// One running fail-count/window-start tally per configured threshold
    /// rule (keyed by `tarpit_thresholds.id`) — any single rule reaching its
    /// own `fail_count` within its own `window_seconds` trips a ban.
    tallies: HashMap<Uuid, (u32, Instant)>,
    trigger_count: u64,
    /// Once a success is recorded for this key (peer_ip only — user keys
    /// never drive banner-drip), this is permanently set to `false`: banner
    /// drip must never be used against an identity known to have logged in
    /// successfully, since it intercepts before russh/auth ever runs.
    banner_drip_eligible: bool,
    banned: BanUntil,
    /// Which rule decided the currently-active `banned` state, if any.
    /// Looked up again (not baked in) each time an outcome is resolved, so
    /// editing that rule's `action` takes effect immediately.
    ban_source: Option<BanSource>,
}

impl TarpitEntry {
    fn new(_now: Instant) -> Self {
        Self {
            tallies: HashMap::new(),
            trigger_count: 0,
            banner_drip_eligible: true,
            banned: BanUntil::NotBanned,
            ban_source: None,
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

/// Admin-configurable threshold rules and active ban rules, plus the global
/// enforcement toggle. Any one threshold rule reaching its own `fail_count`
/// within its own `window_seconds` trips a ban — rules are independent, not
/// combined.
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    pub enabled: bool,
    pub rules: Vec<TarpitThreshold>,
    pub ban_rules: Vec<BanRule>,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: Vec::new(),
            ban_rules: Vec::new(),
        }
    }
}

pub type SharedThresholds = Arc<Mutex<ThresholdConfig>>;

// ── Pure, DB/network-free logic — the part covered by fast unit tests ──────

/// Records one failed attempt for `key`. Returns whether `key` is banned
/// after this call. Crossing any rule's threshold sets a time-bounded ban
/// (auto bans are never indefinite — only admin-created `ban_rules` can be,
/// using the longest window among the rules that tripped, independent of
/// which one's action "wins") and bumps `trigger_count`, which drives the
/// next round-robin method pick. If several rules trip in the same call, a
/// `"ban"`-action rule always outranks a `"trap"`-action one; ties within
/// the same action are broken by the longest window.
fn record_failure(
    map: &mut HashMap<String, TarpitEntry>,
    key: &str,
    rules: &[TarpitThreshold],
    now: Instant,
) -> bool {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| TarpitEntry::new(now));

    let mut winner: Option<(&TarpitThreshold, Duration)> = None;
    let mut max_window = Duration::ZERO;
    for rule in rules {
        let window = Duration::from_secs(rule.window_seconds.max(0) as u64);
        let tally = entry.tallies.entry(rule.id).or_insert((0, now));

        if now.duration_since(tally.1) > window {
            tally.0 = 0;
            tally.1 = now;
        }

        tally.0 += 1;

        if tally.0 >= rule.fail_count.max(0) as u32 {
            tally.0 = 0;
            tally.1 = now;
            max_window = max_window.max(window);

            let is_better = match &winner {
                None => true,
                Some((best, best_window)) => {
                    (rule.action == "ban", window) > (best.action == "ban", *best_window)
                }
            };
            if is_better {
                winner = Some((rule, window));
            }
        }
    }

    if let Some((rule, _)) = winner {
        entry.trigger_count += 1;
        entry.banned = BanUntil::At(now + max_window);
        entry.ban_source = Some(BanSource::Threshold(rule.id));
    }

    entry.is_banned(now)
}

/// A successful login resets every rule's failure tally (so future failures
/// start counting fresh) but deliberately does NOT lift an already-active
/// ban — success only guarantees the *current* session isn't interrupted
/// (which is automatic: the success path never routes through a tarpit
/// branch), not that a standing ban is forgiven for future connections from
/// the same peer_ip/user.
fn clear_fail_tally_on_success(map: &mut HashMap<String, TarpitEntry>, key: &str, _now: Instant) {
    if let Some(entry) = map.get_mut(key) {
        entry.tallies.clear();
    }
}

/// Resolves what should actually happen for an active ban, by looking up
/// its source's *current* `action` — so editing a rule's action takes
/// effect on the very next connection attempt, no restart needed. Falls
/// back to a lenient `Trap` (no reference) if the source row was deleted
/// after the ban took effect, rather than hard-banning forever against a
/// rule that no longer exists.
fn resolve_outcome(
    ban_source: &BanSource,
    trigger_count: u64,
    rules: &[TarpitThreshold],
    ban_rules: &[BanRule],
) -> TarpitOutcome {
    match ban_source {
        BanSource::Threshold(id) => match rules.iter().find(|r| &r.id == id) {
            None => TarpitOutcome::Trap {
                method: TarpitMethod::round_robin(trigger_count),
                source: None,
            },
            Some(rule) if rule.action == "ban" => TarpitOutcome::Ban {
                source: BanSource::Threshold(*id),
            },
            Some(_) => TarpitOutcome::Trap {
                method: TarpitMethod::round_robin(trigger_count),
                source: Some(BanSource::Threshold(*id)),
            },
        },
        BanSource::AdminRule(id) => match ban_rules.iter().find(|r| &r.id == id) {
            None => TarpitOutcome::Trap {
                method: TarpitMethod::round_robin(trigger_count),
                source: None,
            },
            Some(rule) if rule.action == "ban" => TarpitOutcome::Ban {
                source: BanSource::AdminRule(*id),
            },
            Some(_) => TarpitOutcome::Trap {
                method: TarpitMethod::round_robin(trigger_count),
                source: Some(BanSource::AdminRule(*id)),
            },
        },
    }
}

// ── Async, DB-touching wrappers used from lib.rs ────────────────────────────

pub async fn record_auth_failure(
    state: &TarpitState,
    peer_ip: &str,
    user_id: Option<Uuid>,
    thresholds: &ThresholdConfig,
) {
    if !thresholds.enabled {
        return;
    }
    let now = Instant::now();
    let mut map = state.lock().await;
    record_failure(&mut map, peer_ip, &thresholds.rules, now);
    if let Some(uid) = user_id {
        record_failure(&mut map, &user_key(uid), &thresholds.rules, now);
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
/// russh (or drip-fed directly, or dropped outright for a hard `Ban`).
/// `Trap{method: SlowAuth | FakeShell, ..}`/`None` are decided further at
/// accept time but not acted on until inside the `Handler` callbacks, once
/// we know which auth method is actually being attempted.
pub async fn decide_pre_auth_tarpit(
    state: &TarpitState,
    thresholds: &SharedThresholds,
    pool: &PgPool,
    peer_ip: &str,
) -> TarpitOutcome {
    let now = Instant::now();
    let (banned, ban_source, trigger_count, cached_ineligible) = {
        let map = state.lock().await;
        match map.get(peer_ip) {
            Some(e) if e.is_banned(now) => (
                true,
                e.ban_source.clone(),
                e.trigger_count,
                !e.banner_drip_eligible,
            ),
            _ => (false, None, 0, false),
        }
    };
    if !banned {
        return TarpitOutcome::None;
    }

    let (rules, ban_rules) = {
        let t = thresholds.lock().await;
        (t.rules.clone(), t.ban_rules.clone())
    };
    let mut outcome = match ban_source {
        Some(source) => resolve_outcome(&source, trigger_count, &rules, &ban_rules),
        None => TarpitOutcome::Trap {
            method: TarpitMethod::round_robin(trigger_count),
            source: None,
        },
    };

    if let TarpitOutcome::Trap {
        method: TarpitMethod::BannerDrip,
        source,
    } = &outcome
    {
        let ineligible = cached_ineligible
            || ConnectionLog::peer_ip_has_known_good_history(pool, peer_ip)
                .await
                .unwrap_or(false);
        if ineligible {
            let mut map = state.lock().await;
            map.entry(peer_ip.to_string())
                .or_insert_with(|| TarpitEntry::new(now))
                .banner_drip_eligible = false;
            outcome = TarpitOutcome::Trap {
                method: TarpitMethod::SlowAuth,
                source: source.clone(),
            };
        }
    }
    outcome
}

/// Called from inside an auth `Handler` callback (password/keyboard-interactive/
/// publickey) once we already know russh is driving this connection — banner
/// drip is never selectable here since the real SSH-2.0 line has already gone
/// out by the time any `Handler` callback runs.
pub async fn decide_in_auth_tarpit(
    state: &TarpitState,
    thresholds: &SharedThresholds,
    peer_ip: &str,
    user_id: Option<Uuid>,
) -> TarpitOutcome {
    let now = Instant::now();
    let (banned, ban_source, trigger_count) = {
        let map = state.lock().await;
        let mut banned = false;
        let mut trigger_count = 0u64;
        let mut source = None;
        if let Some(e) = map.get(peer_ip) {
            if e.is_banned(now) {
                banned = true;
                trigger_count = trigger_count.max(e.trigger_count);
                source = e.ban_source.clone();
            }
        }
        if let Some(uid) = user_id {
            if let Some(e) = map.get(&user_key(uid)) {
                if e.is_banned(now) {
                    banned = true;
                    trigger_count = trigger_count.max(e.trigger_count);
                    // Prefer the user-scoped source (more specific identity)
                    // when both peer_ip and user are banned simultaneously.
                    source = e.ban_source.clone().or(source);
                }
            }
        }
        (banned, source, trigger_count)
    };
    if !banned {
        return TarpitOutcome::None;
    }

    let (rules, ban_rules) = {
        let t = thresholds.lock().await;
        (t.rules.clone(), t.ban_rules.clone())
    };
    let outcome = match ban_source {
        Some(source) => resolve_outcome(&source, trigger_count, &rules, &ban_rules),
        None => TarpitOutcome::Trap {
            method: TarpitMethod::round_robin(trigger_count),
            source: None,
        },
    };
    match outcome {
        TarpitOutcome::Trap {
            method: TarpitMethod::BannerDrip,
            source,
        } => TarpitOutcome::Trap {
            method: TarpitMethod::SlowAuth,
            source,
        },
        other => other,
    }
}

/// Periodically refreshes admin-configurable thresholds and merges active
/// `ban_rules` rows into the in-memory tarpit state, so webui edits take
/// effect without a server restart. Runs the first refresh synchronously
/// (awaited by the caller) so threshold rules are loaded from the DB before
/// the server starts accepting connections — otherwise the default empty
/// rule set would let an initial burst of failures through uncounted.
pub async fn spawn_settings_refresher(
    pool: PgPool,
    state: TarpitState,
    thresholds: SharedThresholds,
) {
    refresh_once(&pool, &state, &thresholds).await;
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            refresh_once(&pool, &state, &thresholds).await;
        }
    });
}

async fn refresh_once(pool: &PgPool, state: &TarpitState, thresholds: &SharedThresholds) {
    let enabled = Settings::get(pool, "tarpit_enabled")
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(true);
    let rules = TarpitThreshold::list_enabled(pool)
        .await
        .unwrap_or_default();
    let active_ban_rules = BanRule::list_active(pool).await.unwrap_or_default();

    {
        let mut t = thresholds.lock().await;
        t.enabled = enabled;
        t.rules = rules;
        t.ban_rules = active_ban_rules.clone();
    }

    let now_wall = time::OffsetDateTime::now_utc();
    let now_mono = Instant::now();
    let mut map = state.lock().await;
    for rule in active_ban_rules {
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
                BanUntil::At(
                    now_mono + Duration::from_secs(remaining.whole_seconds().max(0) as u64),
                )
            }
        };
        let entry = map.entry(key).or_insert_with(|| TarpitEntry::new(now_mono));
        entry.banned = banned;
        entry.ban_source = Some(BanSource::AdminRule(rule.id));
    }
}

/// Writes the `connection_logs` row for a hard `Ban` decided before `russh`
/// ever touched the connection (accept-time peer_ip ban) — the socket is
/// dropped by the caller right after this, no tarpit method engages at all.
pub async fn log_hard_ban(pool: PgPool, peer_ip: String, source: BanSource) {
    let now = time::OffsetDateTime::now_utc();
    let (threshold_id, ban_rule_id) = match source {
        BanSource::Threshold(id) => (Some(id), None),
        BanSource::AdminRule(id) => (None, Some(id)),
    };
    // The connection is already closed by the time this runs (the accept
    // loop `continue`s right after spawning this), so there's no later
    // disconnect event to wait for — stamp `ended_at` immediately.
    match ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some("ip banned"),
        None,
        None,
        Some("ban"),
        threshold_id,
        ban_rule_id,
        now,
    )
    .await
    {
        Ok(log) => {
            if let Err(e) = ConnectionLog::set_ended(&pool, log.id).await {
                tracing::warn!(err = %e, "failed to mark hard-ban connection ended");
            }
        }
        Err(e) => tracing::warn!(err = %e, "failed to write hard-ban log"),
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

    fn rule_with_action(fail_count: i32, window_secs: i64, action: &str) -> TarpitThreshold {
        let now = time::OffsetDateTime::now_utc();
        TarpitThreshold {
            id: Uuid::now_v7(),
            fail_count,
            window_seconds: window_secs,
            enabled: true,
            action: action.to_string(),
            ts: tunnel2tunnel_core::timestamps::TimestampsSoftDelete {
                timestamps: tunnel2tunnel_core::timestamps::Timestamps {
                    created_at: now,
                    updated_at: now,
                },
                soft_delete: tunnel2tunnel_core::timestamps::SoftDelete { deleted_at: None },
            },
        }
    }

    fn rule(fail_count: i32, window_secs: i64) -> TarpitThreshold {
        rule_with_action(fail_count, window_secs, "trap")
    }

    fn thresholds() -> Vec<TarpitThreshold> {
        vec![rule(3, 60)]
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
        assert_eq!(
            map["1.2.3.4"].ban_source,
            Some(BanSource::Threshold(t[0].id))
        );
    }

    #[test]
    fn record_failure_bans_on_very_first_attempt_when_fail_count_is_one() {
        let mut map = HashMap::new();
        let t = vec![rule(1, 60)];
        let now = Instant::now();
        assert!(record_failure(&mut map, "1.2.3.4", &t, now));
        assert_eq!(map["1.2.3.4"].trigger_count, 1);
        assert_eq!(
            map["1.2.3.4"].ban_source,
            Some(BanSource::Threshold(t[0].id))
        );
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
        assert_eq!(map["1.2.3.4"].tallies[&t[0].id].0, 1);
    }

    #[test]
    fn independent_rules_trip_on_their_own_window() {
        // 2-in-30s rule and a 5-in-600s rule, evaluated independently — the
        // tight rule should trip first even though the loose rule is nowhere
        // near its own count.
        let mut map = HashMap::new();
        let t = vec![rule(2, 30), rule(5, 600)];
        let now = Instant::now();
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert!(record_failure(&mut map, "1.2.3.4", &t, now));
        assert_eq!(map["1.2.3.4"].trigger_count, 1);
    }

    #[test]
    fn ban_action_rule_trips_straight_to_ban() {
        let mut map = HashMap::new();
        let t = vec![rule_with_action(2, 60, "ban")];
        let now = Instant::now();
        assert!(!record_failure(&mut map, "1.2.3.4", &t, now));
        assert!(record_failure(&mut map, "1.2.3.4", &t, now));
        let outcome = resolve_outcome(
            map["1.2.3.4"].ban_source.as_ref().unwrap(),
            map["1.2.3.4"].trigger_count,
            &t,
            &[],
        );
        assert_eq!(
            outcome,
            TarpitOutcome::Ban {
                source: BanSource::Threshold(t[0].id)
            }
        );
    }

    #[test]
    fn mixed_trap_and_ban_rules_prefer_ban() {
        // Both rules trip on the same failure — the "ban" rule must win even
        // though the "trap" rule has a longer window (ban always outranks
        // trap, regardless of window length).
        let mut map = HashMap::new();
        let t = vec![
            rule_with_action(1, 600, "trap"),
            rule_with_action(1, 60, "ban"),
        ];
        let now = Instant::now();
        assert!(record_failure(&mut map, "1.2.3.4", &t, now));
        assert_eq!(
            map["1.2.3.4"].ban_source,
            Some(BanSource::Threshold(t[1].id))
        );
    }

    #[test]
    fn resolve_outcome_falls_back_to_trap_when_rule_missing() {
        // The rule that originally tripped the ban has since been deleted —
        // don't hard-ban forever against a rule that no longer exists.
        let missing_id = Uuid::now_v7();
        let outcome = resolve_outcome(&BanSource::Threshold(missing_id), 0, &[], &[]);
        assert_eq!(
            outcome,
            TarpitOutcome::Trap {
                method: TarpitMethod::BannerDrip,
                source: None
            }
        );

        let outcome = resolve_outcome(&BanSource::AdminRule(missing_id), 0, &[], &[]);
        assert_eq!(
            outcome,
            TarpitOutcome::Trap {
                method: TarpitMethod::BannerDrip,
                source: None
            }
        );
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
        assert!(map["1.2.3.4"].tallies.is_empty());
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
        assert!(
            re.is_match(line.trim_end()),
            "line did not match filter regex: {line:?}"
        );
    }
}
