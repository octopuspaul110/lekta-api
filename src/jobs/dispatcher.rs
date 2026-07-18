use crate::{error::AppResult, jobs::types::JobPayload, state::AppState};

pub async fn dispatch(state: &AppState, payload: JobPayload) -> AppResult<()> {
    match payload {
        JobPayload::SendEmail { 
            to, 
            subject, 
            template, 
            vars 
        } => {
            tracing::info!(to = %to, subject = %subject, "TODO: send email via SES");
            let _ = (template, vars);
            Ok(())
        },
        JobPayload::SendPushNotification { 
            user_id, 
            title, 
            body, 
            data 
        } => {
            tracing::info!(user_id = %user_id, title = %title, body = %body, data = %data, "TODO: send push notification via FCM");
            Ok(())
        },
        JobPayload::MaterializeClassOccurrences { 
            class_id, 
            from, 
            to 
        } => {
            tracing::info!(class_id = %class_id, from = %from, to = %to, "TODO: materialize class occurences via rrule");
            Ok(())
        },
        JobPayload::AutoSubmitExpiredAttempt { 
            attempt_id 
        } => {
            tracing::info!(attempt_id = %attempt_id, "TODO: auto-submit expired attempt");
            Ok(())
        },
        JobPayload::StudentWelcomeSequence { 
            workspace_id, 
            user_id 
        } => {
            tracing::info!(workspace_id = %workspace_id, user_id = %user_id, "TODO: schedule welcome sequence steps");
            Ok(())
        },
        JobPayload::ChargeMonthlySubscription { 
            workspace_id 
        } => {
            tracing::info!(workspace_id = %workspace_id, "TODO: charge monthly subscription");
            Ok(())
        },
        JobPayload::GenerateNoteSummary { 
            note_id 
        } => {
            tracing::info!(note_id = %note_id, "TODO: generate note summary via anthropic");
            Ok(())
        },
        JobPayload::GenerateExamAnalytics { 
            attempt_id 
        } => {
            tracing::info!(attempt_id = %attempt_id, "TODO: generate exam analytics");
            Ok(())
        },
        JobPayload::ReconcilePayment { 
            payment_id 
        } => {
            tracing::info!(payment_id = %payment_id, "TODO: reconcile payment ith paystack");
            let _ = state;
            Ok(())
        },
        JobPayload::NightlyCleanup => {
            tracing::info!("running nighly cleanup");
            nightly_cleanup(state).await
        },
    }
}

async fn nightly_cleanup(state: &AppState) -> AppResult<()> {
    // Expire old invitations
    sqlx::query!(
        "DELETE FROM workspace_invitations WHERE expires_at < NOW() AND accepted = FALSE"
    )
    .execute(&state.db)
    .await?;

    // Expire old refresh tokens
    sqlx::query!(
        "DELETE FROM refresh_tokens WHERE expires_at < NOW() OR (revoked = TRUE AND revoked_at < NOW() - INTERVAL '30 days')"
    )
    .execute(&state.db)
    .await?;

    // Expire old password reset tokens
    sqlx::query!(
        "DELETE FROM password_reset_tokens WHERE expires_at < NOW() OR used = TRUE"
    )
    .execute(&state.db)
    .await?;

    // Expire old verification tokens
    sqlx::query!(
        "DELETE FROM email_verification_tokens WHERE expires_at < NOW() OR used = TRUE"
    )
    .execute(&state.db)
    .await?;

    Ok(())
}

