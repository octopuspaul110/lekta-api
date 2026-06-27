use axum::{Json, extract::{Path,State}};
use chrono::{DateTime, Duration, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::{extractor::AuthUser, refresh_token::{generate_refresh_token, hash_refresh_token}}, error::{AppError, AppResult}, state::AppState, workspaces::{extractor::WorkspaceContext, types::WorkspaceRole}};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateInvitationRequest {
    #[validate(email)]
    pub email: String,
    pub role: WorkspaceRole,
    pub personal_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InvitationResponse {
    pub id: Uuid,
    pub email: String,
    pub role: WorkspaceRole,
    pub expires_at: DateTime<Utc>,
}

pub async fn create_invitation(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    auth: AuthUser,
    Json(req): Json<CreateInvitationRequest>,
) -> AppResult<(StatusCode,Json<InvitationResponse>)> {
    req.validate()?;

    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    if matches!(ctx.role,WorkspaceRole::Proprietor) {
        return Err(AppError::BadRequest("cannot invite as proprietor".into()));
    }

    let email_lower = req.email.to_lowercase();

    // check if member is existing
    let existing_member = sqlx::query!(
        r#"
        SELECT WM.ID FROM workspace_members wm
        JOIN users u ON u.id = wm.user_id
        WHERE wm.workspace_id = $1 AND lower(u.email) = $2 and WM.status = 'active'
        "#,
        ctx.workspace_id,
        email_lower
    ).fetch_optional(&state.db)
    .await?;

    if existing_member.is_some() {
        return Err(AppError::Conflict("user is already a member".into()));
    }

    // check member has no outstanding invitation
    let existing_invite = sqlx::query!(
        r#"
        SELECT id FROM workspace_invitations
        WHERE workspace_id = $1 AND email = $2 AND accepted = FALSE AND expires_at > NOW()
        "#,
        ctx.workspace_id,
        email_lower
    )
    .fetch_optional(&state.db)
    .await?;

    if existing_invite.is_some(){
        return Err(AppError::Conflict("invitation already pending".into()));
    }

    let token_raw = generate_refresh_token();
    let token_hash = hash_refresh_token(&token_raw);
    let expires_at = Utc::now() + Duration::days(7);

    let role_str = serde_json::to_value(&req.role)?
        .as_str()
        .ok_or_else(|| AppError::Internal("role serialization failed".into()))?
        .to_string();

    let invitation = sqlx::query!(
        r#"
        INSERT INTO workspace_invitations (workspace_id, email, role, token_hash, invited_by_user_id, expires_at, personal_message)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, email, expires_at
        "#,
        ctx.workspace_id,
        email_lower,
        role_str,
        token_hash,
        auth.user_id,
        expires_at,
        req.personal_message
    )
    .fetch_one(&state.db)
    .await?;

    // log invitation link (TODO: send via ses)
    if !state.config.environment.is_production() {
        tracing::info!(
            workspace_slug = %ctx.slug,
            email = %email_lower,
            invitation_token = %token_raw,
            "invitation created (DEV ONLY)"
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            id: invitation.id,
            email: invitation.email,
            role: req.role,
            expires_at: invitation.expires_at,
        }),
    ))
}

pub async fn list_invitations(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<Json<Vec<InvitationResponse>>> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let rows = sqlx::query!(
        r#"
        SELECT id, email, role, expires_at
        FROM workspace_invitations
        WHERE workspace_id = $1 AND accepted = FALSE AND expires_at > NOW()
        ORDER BY created_at DESC
        "#,
        ctx.workspace_id
    )
    .fetch_all(&state.db)
    .await?;

    let invitations: Result<Vec<_>, AppError> = rows
        .into_iter()
        .map(|r| {
            let role: WorkspaceRole = serde_json::from_value(serde_json::json!(r.role))
                .map_err(|_| AppError::Internal("invalid role".into()))?;
            Ok(InvitationResponse {
                id: r.id,
                email: r.email,
                role,
                expires_at: r.expires_at
            })
        })
        .collect();

    Ok(Json(invitations?))
}

pub async fn cancel_invitation(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, invitation_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let result = sqlx::query!(
        r#"
        DELETE FROM workspace_invitations
        WHERE id = $1 AND workspace_id = $2 AND accepted = FALSE
        "#,
        invitation_id,
        ctx.workspace_id
    )
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0b0000 {
        return Err(AppError::NotFound("invite not found".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct AcceptInvitationResponse {
    pub workspace_id: Uuid,
    pub workspace_slug: String,
    pub workspace_name: String,
    pub role: WorkspaceRole,
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(token): Path<String>,
) -> AppResult<Json<AcceptInvitationResponse>> {
    let token_hash = hash_refresh_token(&token);

    let invitation = sqlx::query!(
        r#"
        SELECT id, workspace_id, email, role, expires_at, accepted
        FROM workspace_invitations
        WHERE token_hash = $1
        "#,
        token_hash
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("invitation not found".into()))?;

    if invitation.accepted {
        return Err(AppError::NotFound("invitations already accepted".into()));
    }

    if invitation.expires_at < Utc::now() {
        return Err(AppError::Unauthorized("invitations expired".into()));
    }

    // verify the invitee's email matches the invitation
    if auth.email.to_lowercase() != invitation.email {
        return Err(AppError::Forbidden("invitation was sent to a different email".into()));
    }

    let workspace = sqlx::query!(
        r#"
        SELECT id, slug, name 
        FROM workspaces 
        WHERE id = $1 AND deleted_at IS NULL
        "#,
        invitation.workspace_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let role: WorkspaceRole = serde_json::from_value(serde_json::json!(&invitation.role))
        .map_err(|_|AppError::Internal("invalid role".into()))?;

    let mut tx = state.db.begin().await?;

    // insert member into workspace
    sqlx::query!(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role, status, invited_by_user_id)
        VALUES ($1, $2, $3, 'active', (SELECT invited_by_user_id FROM workspace_invitations WHERE id = $4))
        ON CONFLICT (workspace_id, user_id) DO UPDATE
            SET role = $3, status = 'active', removed_at = NULL
        "#,
        invitation.workspace_id,
        auth.user_id,
        invitation.role,
        invitation.id
    )
    .execute(&mut *tx)
    .await?;

    // mar invitation as accepted
    sqlx::query!(
        r#"
        UPDATE workspace_invitations
        SET accepted = TRUE, accepted_at = NOW(), accepted_by_user_id = $1
        WHERE id = $2
        "#,
        auth.user_id,
        invitation.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // TODO: enqueue student_welcome_sequence job if role is student
    if matches!(role, WorkspaceRole::Student){
        tracing::info!(
            workspace_id = %invitation.workspace_id,
            user_id = %auth.user_id,
            "would enqueue student_welcome_sequence (TODO when jobs are built)"
        );
    }

    Ok(Json(AcceptInvitationResponse {
        workspace_id: workspace.id,
        workspace_name: workspace.name,
        workspace_slug: workspace.slug,
        role
    }))

}