//! Slow-auth drip-feed tarpit: accept the connection and let the real SSH
//! handshake proceed, but delay each auth reply on branches that were
//! already going to reject. Never called on a path that leads to
//! `Auth::Accept`, so a legitimate key is never slowed down.

use std::time::Duration;

const BASE_DELAY: Duration = Duration::from_secs(3);
const JITTER_MAX_MS: u64 = 4000;

/// Sleeps `BASE_DELAY` plus up to `JITTER_MAX_MS` of jitter before the caller
/// proceeds to reject the auth attempt.
pub async fn delay() {
    let mut buf = [0u8; 8];
    let _ = getrandom::fill(&mut buf);
    let jitter_ms = u64::from_be_bytes(buf) % JITTER_MAX_MS;
    tokio::time::sleep(BASE_DELAY + Duration::from_millis(jitter_ms)).await;
}
