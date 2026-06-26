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