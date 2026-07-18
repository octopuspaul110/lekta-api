use std::time::Duration;

use chrono::Timelike;
use tokio::sync::broadcast;
use crate::{jobs::{types::JobPayload, worker::enqueue}, state::AppState};

pub async fn run_scheduler(state: AppState, mut shutdown: broadcast::Receiver<()>) {
    let poll_interval = Duration::from_secs(60);
    tracing::info!("job scheduler started");

    let mut last_cleanup_date: Option<chrono::NaiveDate> = None;

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                tracing::info!("job scheduler shutting down");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {}
        }

        let now = chrono::Utc::now();
        let today = now.date_naive();

        // Nightly cleanup at 02:00 UTC
        if now.time().hour() == 2 && last_cleanup_date != Some(today) {
            if let Err(e) = enqueue(&state, JobPayload::NightlyCleanup, None).await {
                tracing::error!(error = ?e, "failed to enqueue nightly cleanup");
            } else {
                last_cleanup_date = Some(today);
            }
        }
    }
}