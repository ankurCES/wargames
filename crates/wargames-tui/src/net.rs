//! Shared network helpers.
//!
//! The 12-second ceiling on every outbound fetch is a contract carried
//! forward from the JS implementation (every fetch there uses
//! `AbortSignal.timeout(12000)`). `WOPR_NET_TIMEOUT_MS` overrides it.

use std::time::Duration;
use tokio::time::timeout;

pub fn ceiling() -> Duration {
    let ms = std::env::var("WOPR_NET_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(12_000);
    Duration::from_millis(ms)
}

/// Run `fut` under the shared ceiling. Returns `Err(Timeout)` on expiry —
/// callers decide whether to fall back to a deterministic action.
pub async fn with_ceiling<F, T>(fut: F) -> Result<T, tokio::time::error::Elapsed>
where
    F: std::future::Future<Output = T>,
{
    timeout(ceiling(), fut).await
}