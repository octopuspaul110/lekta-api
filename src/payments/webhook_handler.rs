use axum::{body::Bytes, extract::State, response::IntoResponse, http::{HeaderMap, StatusCode}};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha512;
use subtle::ConstantTimeEq;

use crate::{error::{AppError, AppResult}, state::AppState};

type HmacSha512 = Hmac<Sha512>;

#[derive(Debug, Deserialize)]
struct WebhookPayload {
    event: String,
    data: WebhookData,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct WebhookData {
    id: i64,
    reference: String,
    amount: i64,
    status: String,
    #[serde(default)]
    fees: Option<i64>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

pub async fn paystack_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // verify signature
    let signature = match headers.get("x-paystack-signature") {
        Some(v) => match v.to_str() {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!("invalid signature header encoding");
                return (StatusCode::BAD_REQUEST, "invalid signature").into_response();
            }
        },
        None => {
            tracing::warn!("missing signature header");
            return (StatusCode::BAD_REQUEST, "missing signature").into_response();
        }
    };

    let mut mac = match HmacSha512::new_from_slice(state.config.paystack_webhook_secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            tracing::error!("failed to build hmac");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    mac.update(&body);
    let expected = mac.finalize().into_bytes();
    let expected_hex = hex::encode(expected);

    if signature.as_bytes().ct_eq(expected_hex.as_bytes()).unwrap_u8() != 1 {
        tracing::warn!(
            provided = %signature,
            "webhook signature mismatch - possible attack"
        );
        return (StatusCode::UNAUTHORIZED, "invalid signature").into_response()
    }

    // Step 2: Parse body
    let payload: WebhookPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse webhook body");
            return (StatusCode::BAD_REQUEST, "invalid body").into_response()
        }
    };

    // Step 3: Record in idempotency table
    let event_id = payload.data.id.to_string();
    let raw_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let insert_result = sqlx::query!(
        r#"
        INSERT INTO paystack_webhook_events (event_id, event_type, raw_payload)
        VALUES ($1, $2, $3)
        ON CONFLICT (event_id) DO NOTHING
        RETURNING event_id
        "#,
        event_id,
        payload.event,
        raw_json
    )
    .fetch_optional(&state.db)
    .await;

    match insert_result {
        Ok(None) => {
            // Already processed - return 200 immediately
            tracing::info!(event_id = %event_id, "duplicate webhook event, ignoring");
            return (StatusCode::OK,"").into_response();
        }
        Ok(Some(_)) => {
            // new event - process
        }
        Err(e) => {
            tracing::error!(error = %e, "webhook idempotency insert failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    }

    // Step 4: Process
    if let Err(e) = process_event(&state, &payload).await {
        tracing::error!(
            event_id = %event_id,
            error = ?e,
            "webhook processing failed"
        );
        // Mark as errored so it can be retried later
        let _ = sqlx::query!(
            "UPDATE paystack_webhook_events SET error_message = $1 WHERE event_id = $2",
            format!("{:?}", e),
            event_id
        )
        .execute(&state.db)
        .await;
        // return 200
        return (StatusCode::OK, "").into_response();
    }

    // Mark as processed
    let _ = sqlx::query!(
        r#"
        UPDATE paystack_webhook_events
        SET processed = TRUE, processed_at = NOW()
        WHERE event_id = $1
        "#,
        event_id
    )
    .execute(&state.db)
    .await;
    

    (StatusCode::OK, "").into_response()
}

async fn process_event(
    state: &AppState,
    payload: &WebhookPayload
) -> AppResult<()> {
    match payload.event.as_str() {
        "charge.success" => process_charge_success(state, payload).await,
        "charge.failed" => process_charge_failed(state, payload).await,
        other => {
            tracing::info!(event_type = %other, "ignoring unsupported webhook event");
            Ok(())
        }
    }
}

async fn process_charge_success(state: &AppState, payload: &WebhookPayload) -> AppResult<()> {
    // DEFENSIVE: verify via paystack API - don't trust webhook alone
    let verified = state.paystack.verify_transaction(&payload.data.reference).await?;

    if verified.status != "success" {
        return Err(AppError::Internal(format!("webhook claimed success but verify says {}", verified.status)));
    }

    // Find the payment
    let payment = sqlx::query!(
        r#"
        SELECT id, enrollment_id, workspace_id, amount_kobo, platform_fee_kobo, status
        FROM payments WHERE reference = $1
        "#,
        payload.data.reference
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("payment not found for reference {}", payload.data.reference)))?;

    // Idempotency: if already successful, do nothing
    if payment.status == "successful" {
        tracing::info!(reference = %payload.data.reference, "payment already successful, skipping");
        return Ok(());
    }

    let paystack_fee_kobo = verified.fees.unwrap_or(0);
    let center_amount_kobo = payment.amount_kobo - payment.platform_fee_kobo - paystack_fee_kobo;

    let mut tx = state.db.begin().await?;

    // Update payment
    sqlx::query!(
        r#"
        UPDATE payments
        SET status = 'successful',
            paid_at = NOW(),
            paystack_fee_kobo = $1,
            center_amount_kobo = $2,
            paystack_response = $3
        WHERE id = $4
        "#,
        paystack_fee_kobo,
        center_amount_kobo,
        serde_json::to_value(&verified)?,
        payment.id
    )
    .execute(&mut *tx)
    .await?;

    // Activate enrollment if this was a tuition payment
    if let Some(enrollment_id) = payment.enrollment_id {
        let plan = sqlx::query!(
            r#"
            SELECT tp.duration_days
            FROM enrollments e
            JOIN tuition_plans tp ON tp.id = e.tuition_plan_id
            WHERE e.id = $1
            "#,
            enrollment_id
        )
        .fetch_one(&mut *tx)
        .await?;

    let now = Utc::now();
    let ends_at = now + Duration::days(plan.duration_days as i64);

    sqlx::query!(
        r#"
        UPDATE enrollments
        SET status = 'active', starts_at = $1, ends_at = $2
        WHERE id = $3
        "#,
        now,
        ends_at,
        enrollment_id
    )
    .execute(&mut *tx)
    .await?;
    }

    tx.commit().await?;

    tracing::info!(
        reference = %payload.data.reference,
        amount_kobo = payment.amount_kobo,
        center_amount_kobo = center_amount_kobo,
        "payment successfull and enrollment activated"
    );

    // TODO: enqueue notification jobs (send receipt email, push notification to payer + workspace admin)

    Ok(()) 
}

async fn process_charge_failed(
    state: &AppState,
    payload: &WebhookPayload
) -> AppResult<()> {
    sqlx::query!(
        r#"
        UPDATE payments
        SET status = 'failed', failure_reason = $1
        WHERE reference = $2 AND status = 'pending'
        "#,
        format!("charge failed: {}",payload.data.status),
        payload.data.reference
    )
    .execute(&state.db)
    .await?;

    tracing::info!(reference = %payload.data.reference, "payment marked as failed");

    Ok(())
}