//! Ereignisströme der Anlage dauerhaft offen halten.

use std::future::Future;
use std::time::{Duration, Instant};

use sf_onehub::sf_backoff::Backoff;

/// Ruft `f` immer wieder auf, sobald der Stream endet oder abbricht. Die
/// Wartezeit dazwischen wächst, bis eine Verbindung stabil lief, und richtet
/// sich nach den Rate-Limits der Anlage (siehe `sf-backoff`).
pub async fn forever<F, Fut>(what: &'static str, max: Duration, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = sf_onehub::Result<()>>,
{
    let mut backoff = Backoff::new(Duration::from_secs(1), max);
    loop {
        let started = Instant::now();
        let hint = match f().await {
            Ok(()) => {
                tracing::debug!("{what}: Stream von der Anlage beendet");
                None
            }
            Err(e) => {
                let hint = e.retry_after();
                match hint {
                    Some(wait) => tracing::warn!(?wait, "{what}: Rate-Limit der Anlage"),
                    None => tracing::warn!(error = %e, "{what}: Ereignisse unterbrochen"),
                }
                hint
            }
        };
        tokio::time::sleep(backoff.next(started.elapsed(), hint)).await;
    }
}
