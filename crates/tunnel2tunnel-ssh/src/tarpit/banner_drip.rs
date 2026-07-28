//! Endlessh-style tarpit: RFC4253 §4.2 allows a server to send any number of
//! "other lines of data" (CRLF-terminated, <=255 bytes, not starting with
//! "SSH-") before its real identification string. This drips such lines
//! forever and never sends the real `SSH-2.0-...` line, so the client hangs
//! waiting for a handshake that will never come.
//!
//! `russh::server::run_stream` unconditionally writes the real ID string as
//! its first action, before any `Handler` callback fires — there is no hook
//! to intercept this. So this method operates directly on the raw
//! `TcpStream`, before it's ever handed to russh, for connections already
//! selected for this tarpit method at accept time.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use tunnel2tunnel_core::models::connection_log::ConnectionLog;

use super::{fail2ban_tarpit_line, BanSource};

const DRIP_INTERVAL: Duration = Duration::from_secs(10);
/// Generous cap so a large scan can't grow unbounded fd/task usage forever.
const MAX_DRIP_DURATION: Duration = Duration::from_secs(6 * 60 * 60);

/// A random RFC4253-legal "other line of data": never starts with `"SSH-"`,
/// CRLF-terminated, well within the 255-byte limit.
pub fn random_rfc4253_line() -> String {
    let mut buf = [0u8; 24];
    // getrandom::fill only fails on catastrophic OS RNG failure; falling
    // back to an all-zero buffer still yields a harmless (if less random)
    // valid tarpit line rather than panicking the accept loop.
    let _ = getrandom::fill(&mut buf);
    let mut line = format!("{:x}", u128::from_be_bytes(buf[..16].try_into().unwrap()));
    line.push_str(&format!(
        "{:x}",
        u64::from_be_bytes(buf[16..24].try_into().unwrap())
    ));
    format!("{line}\r\n")
}

/// Drips random lines at a banned peer_ip that will never receive the real
/// SSH identification string. Runs until the client gives up (write fails)
/// or `MAX_DRIP_DURATION` elapses.
pub async fn run(
    mut socket: TcpStream,
    peer_ip: String,
    pool: PgPool,
    fail2ban: Option<Arc<String>>,
    fail_reason: &'static str,
    source: Option<BanSource>,
) {
    let (threshold_id, ban_rule_id) = match source {
        Some(BanSource::Threshold(id)) => (Some(id), None),
        Some(BanSource::AdminRule(id)) => (None, Some(id)),
        None => (None, None),
    };
    let now = time::OffsetDateTime::now_utc();
    let log = ConnectionLog::create(
        &pool,
        None,
        None,
        Some(&peer_ip),
        None,
        None,
        None,
        Some(fail_reason),
        None,
        Some("banner_drip"),
        Some("trap"),
        threshold_id,
        ban_rule_id,
        now,
    )
    .await
    .ok();

    if let Some(path) = &fail2ban {
        let line = fail2ban_tarpit_line(&crate::chrono_like_timestamp(), &peer_ip);
        let _ = crate::append_to_file(path, &line).await;
    }

    let deadline = tokio::time::Instant::now() + MAX_DRIP_DURATION;
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        let line = random_rfc4253_line();
        if socket.write_all(line.as_bytes()).await.is_err() {
            break;
        }
        tokio::time::sleep(DRIP_INTERVAL).await;
    }

    if let Some(log) = log {
        let _ = ConnectionLog::set_ended(&pool, log.id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_never_starts_with_ssh_dash() {
        for _ in 0..50 {
            let line = random_rfc4253_line();
            assert!(!line.starts_with("SSH-"));
            assert!(line.ends_with("\r\n"));
            assert!(line.len() <= 255);
        }
    }
}
