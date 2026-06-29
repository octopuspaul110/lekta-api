use serde::{Deserialize, Serialize};
use sqlx::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceRole {
    Proprietor,
    Admin,
    Tutor,
    Student
}

impl WorkspaceRole {
    pub fn is_admin_or_above(&self) -> bool{
        matches!(self, Self::Proprietor | Self::Admin)
    }

    pub fn is_tutor_or_above(&self) -> bool {
        matches!(self, Self::Proprietor | Self::Admin | Self::Tutor)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PaymentMode {
    LektaManaged,
    External,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type",rename_all = "snake_case")]
pub enum OnboardingStep {
    WelcomeMessage {
        title: String,
        content: String,
        delay_hours: u32,
    },
    TutorIntroductions {
        delay_hours: u32
    },
    ChannelTour {
        channel_slugs: Vec<String>,
        delay_hours: u32
    },
    EnrollmentReminder {
        delay_hours: u32
    },
    FirstClassREminder {
        delay_hours: u32
    },
    Custom {
        title: String,
        content: String,
        delay_hours: u32
    },
}

impl OnboardingStep {
    pub fn default_sequence(workspace_name: &str) -> Vec<Self> {
        vec![
            Self::WelcomeMessage {
                title: format!("welcome to {}",workspace_name),
                content: format!(
                    "We are glad to have you. Take a moment to look around - your tutors are here to help you succeed."
                ),
                delay_hours: 0,
            },
            Self::TutorIntroductions { delay_hours: 24 },
            Self::EnrollmentReminder { delay_hours: 72 },
            Self::WelcomeMessage { 
                title: "How is your first week going?".to_string(), 
                content: "If you need anything at all, your tutors and admins are here to help. Reply in #general anytime.".to_string(), 
                delay_hours: 168,
            },
        ]
    }
}