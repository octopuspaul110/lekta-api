use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize,Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobPayload {
    /// Send an email via SES
    SendEmail {
        to: String,
        subject: String,
        template: String,
        vars: serde_json::Value,
    },

    /// Send a push notification via FCM
    SendPushNotification {
        user_id: Uuid,
        title: String,
        body: String,
        data: serde_json::Value,
    }, 

    /// Materialize occurences of a recurring class within a window.
    MaterializeClassOccurrences {
        class_id: Uuid,
        from: DateTime<Utc>,
        to: DateTime<Utc>
    },

    /// Auto-submit an in-progress exam attempt whose deadline has passed.
    AutoSubmitExpiredAttempt {
        attempt_id: Uuid,
    },  

    /// Send the sequence of welcome messages to a new student.
    StudentWelcomeSequence {
        workspace_id: Uuid,
        user_id: Uuid,
    },

    /// Charge a workspaces monthly subscription.
    ChargeMonthlySubscription {
        workspace_id: Uuid,
    }, 

    /// Generate an AI summary for a note.
    GenerateNoteSummary {
        note_id: Uuid,
    },

    /// Generate AI exam analytics for a completed attempt.
    GenerateExamAnalytics {
        attempt_id: Uuid
    },

    /// Verify payment against Paystack (defensive reconciliation).
    ReconcilePayment {
        payment_id: Uuid
    } ,

    /// Nightly cleanup - expired invitations, old sessions, etc.
    NightlyCleanup,
}