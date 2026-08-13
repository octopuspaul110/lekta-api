use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{error::AppResult, jobs::{dispatcher::dispatch, types::JobPayload}, state::AppState};


pub async fn run_worker(state: AppState, mut shutdown: broadcast::Receiver<()>) {
    let poll_interval = Duration::from_secs(2);
    tracing::info!("job worker started");

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                tracing::info!("job worker shutting down");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {}
        }

        match claim_and_run_one(&state).await {
            Ok(true) => continue,
            Ok(false) => continue, // no jobs available, sleep next iteration
            Err(e) => {
                tracing::error!(error = ?e, "worker iteration error"); 
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub async fn claim_and_run_one(state: &AppState) -> AppResult<bool> {
    let claimed = sqlx::query!(
        r#"
        UPDATE jobs
        SET status = 'processing', started_at = NOW(), attempts = attempts + 1
        WHERE id = (
            SELECT id FROM jobs
            WHERE status = 'pending' AND run_at < NOW()
            ORDER BY run_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING id, job_type, payload, attempts, max_attempts
        "#
    )
    .fetch_optional(&state.db)
    .await?;

    let job = match claimed {
        Some(j) => j,
        None => return Ok(false),
    };

    let payload: Result<JobPayload, _> = serde_json::from_value(job.payload);
    let payload = match payload {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(job_id = %job.id, error = ?e, "job payload deserialization failed");
            sqlx::query!(
                "UPDATE jobs SET status = 'failed', error_message = $1, completed_at = NOW() WHERE id = $2",
                format!("payload parse error: {}", e),
                job.id
            )
            .execute(&state.db)
            .await?;
            return Ok(true);
        }
    };

    let result = dispatch(state, payload).await;

    match result {
        Ok(_) => {
            sqlx::query!(
                "UPDATE jobs SET status = 'completed', completed_at = NOW() WHERE id = $1",
                job.id
            )
            .execute(&state.db)
            .await?;
            Ok(true)
        }
        Err(e) => {
            let err_msg = format!("{:?}", e);
            let should_retry = job.attempts < job.max_attempts;

            if should_retry {
                // Exponential backoff: 60s * 2^(attempt - 1)
                let delay_secs = 60_i64 * (1_i64 << (job.attempts as i64 - 1));
                sqlx::query!(
                    "UPDATE jobs 
                    SET status = 'pending',
                        run_at = NOW() + ($1::BIGINT * INTERVAL '1 second'),
                        error_message = $2
                    WHERE id = $3",
                    delay_secs,
                    err_msg,
                    job.id
                )
                .execute(&state.db)
                .await?;
                
                tracing::warn!(
                    job_id = %job.id,
                    attempts = %job.attempts,
                    delay = %delay_secs,
                    "job failed, retrying..."
                );
            } else {
                sqlx::query!(
                    "UPDATE jobs 
                    SET status = 'failed', error_message = $1, completed_at = NOW() WHERE id = $2",
                    err_msg,
                    job.id
                )
                .execute(&state.db)
                .await?;
                
                tracing::error!(job_id = %job.id, attempts = job.attempts, "job failed permanently");
            }

            Ok(true)
        }
    }
}

/// Public API: enqueue a job to run at some point
pub async fn enqueue(
    state: &AppState,
    payload: JobPayload,
    run_at: Option<DateTime<Utc>>,
)  -> AppResult<Uuid> {
    let job_id = Uuid::now_v7();
    let job_type = match &payload {
        JobPayload::SendEmail { .. } => "send_email",
        JobPayload::SendPushNotification { .. } => "send_push_notification",
        JobPayload::MaterializeClassOccurrences { .. } => "materialize_class_occurences",
        JobPayload::AutoSubmitExpiredAttempt { .. } => "auto_submit_expired_attempt",
        JobPayload::StudentWelcomeSequence { .. } => "student_welcome_sequence",
        JobPayload::ChargeMonthlySubscription { .. } => "charge_monthly_subscription",
        JobPayload::GenerateNoteSummary { .. } => "generate_note_summary",
        JobPayload::GenerateExamAnalytics { .. } => "generate_exam_analytics",
        JobPayload::ReconcilePayment { .. } => "reconcile_payment",
        JobPayload::NightlyCleanup => "nightly_cleanup",
    };

    let payload_json = serde_json::to_value(&payload)?;
    let run_at = run_at.unwrap_or_else(Utc::now);

    sqlx::query!(
        "INSERT INTO jobs (id, job_type, payload, run_at) VALUES ($1, $2, $3, $4)",
        job_id,
        job_type,
        payload_json,
        run_at
    )
    .execute(&state.db)
    .await?;

    Ok(job_id)
}


