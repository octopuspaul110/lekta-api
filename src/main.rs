use std::net::SocketAddr;
use lekta_api::{config::Config, state::AppState};
use tokio::{net::TcpListener, signal::{self, unix::SignalKind}};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use axum::{Router, routing::{delete, get, patch, post}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;

    let env_filter = EnvFilter::try_new(&config.rust_log).unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry()
    .with(env_filter);

    if config.environment.is_production(){
        registry.with(tracing_subscriber::fmt::layer().json()).init();
    }else {
        registry.with(tracing_subscriber::fmt::layer().pretty()).init();
    }

    let _sentry = config.sentry_dsn.as_ref().map(|dsn| {
        sentry::init((
            dsn.as_str(),
            sentry::ClientOptions {
                release : sentry::release_name!(),
                environment : Some(format!("{:?}", config.environment).into()),
                ..Default::default()
            }
        ))
    });

    let port = config.port;
    tracing::info!("Connecting to database and running migrations...");
    let state = AppState::new(config).await?;
    tracing::info!("Application state initialized, starting lekta-api...");

    let app = Router::new()
            .route("/api/v1/health", get(|| async {"ok"}))
            .route("/api/v1/auth/register", post(lekta_api::auth::handlers::register))
            .route("/api/v1/auth/login", post(lekta_api::auth::handlers::login))
            .route("/api/v1/auth/refresh", post(lekta_api::auth::handlers::refresh))
            .route("/api/v1/auth/logout", post(lekta_api::auth::handlers::logout))
            .route("/api/v1/auth/logout-all", post(lekta_api::auth::handlers::logout_all))
            .route("/api/v1/auth/forgot-password", post(lekta_api::auth::handlers::forgot_password))
            .route("/api/v1/auth/reset-password", post(lekta_api::auth::handlers::reset_password))
            .route("/api/v1/auth/verify-email", post(lekta_api::auth::handlers::verify_email))
            .route("/api/v1/workspaces", post(lekta_api::workspaces::handlers::create_workspace))
            .route("/api/v1/workspaces", get(lekta_api::workspaces::handlers::list_my_workspaces))
            .route("/api/v1/workspaces/{slug}", get(lekta_api::workspaces::handlers::get_workspace))
            .route("/api/v1/workspaces/{slug}", patch(lekta_api::workspaces::handlers::update_workspace))
            .route("/api/v1/workspaces/{slug}/transfer-ownership", post(lekta_api::workspaces::handlers::transfer_ownership))
            .route("/api/v1/workspaces/{slug}", delete(lekta_api::workspaces::handlers::delete_workspace))
            .route("/api/v1/workspaces/{slug}/members",
       get(lekta_api::workspaces::members_handlers::list_members))
            .route("/api/v1/workspaces/{slug}/members/{user_id}",
                patch(lekta_api::workspaces::members_handlers::change_member_role).delete(lekta_api::workspaces::members_handlers::remove_member))
            .route("/api/v1/workspaces/{slug}/invitations",
                post(lekta_api::workspaces::invitations_handlers::create_invitation).get(lekta_api::workspaces::invitations_handlers::list_invitations))
            .route("/api/v1/workspaces/{slug}/invitations/{id}",
                delete(lekta_api::workspaces::invitations_handlers::cancel_invitation))
            .route("/api/v1/invitations/{token}/accept",
                post(lekta_api::workspaces::invitations_handlers::accept_invitation))
            .route("/api/v1/invitations/{slug}/onboarding", get(lekta_api::workspaces::handlers::get_onboarding)
            .patch(lekta_api::workspaces::handlers::update_onboarding))
            .route("/api/v1/workspaces/{slug}/channels", 
            get(lekta_api::channels::handlers::list_channels)
            .post(lekta_api::channels::handlers::list_channels))
            .route("/api/v1/channels/{id}", 
            get(lekta_api::channels::handlers::get_channel)
            .patch(lekta_api::channels::handlers::update_channel)
            )
            .route("/api/v1/channels/{id}/archive", post(lekta_api::channels::handlers::archive_channel))
            .route("/api/v1/channels/{id}/members", 
            post(lekta_api::channels::member_handlers::add_or_join_channel))
            .route("/api/v1/channels/{id}/members/{user_id}",
       delete(lekta_api::channels::member_handlers::remove_channel_member))
            .route("/api/v1/channels/{id}/read",
       post(lekta_api::channels::member_handlers::mark_channel_read))
       .route("/api/v1/channels/{id}/messages", 
       get(lekta_api::channels::messages_handlers::list_messages)
       .post(lekta_api::channels::messages_handlers::send_message))
       .route("/api/v1/messages/{id}", patch(lekta_api::channels::messages_handlers::edit_message)
       .delete(lekta_api::channels::messages_handlers::delete_message))
       .route("/api/v1/messages/{id}/thread", get(lekta_api::channels::messages_handlers::get_thread))
       .route("/api/v1/messages/{id}/reactions", post(lekta_api::channels::reactions_handlers::add_reaction))
       .route("/api/v1/messages/{id}/reactions/{emoji}", delete(lekta_api::channels::reactions_handlers::remove_reaction))
       .route("/api/v1/workspaces/{slug}/search", get(lekta_api::channels::search::search))
       .route("api/v1/payments/banks", get(lekta_api::payments::onboarding_handlers::list_banks))
       .route("api/v1/payments/resolve_account", get(lekta_api::payments::onboarding_handlers::resolve_account))
       .route("api/v1/workspaces/{slug}/onboarding/paystack", post(lekta_api::payments::onboarding_handlers::onboard_paystack))
       .route("api/v1/workspaces/{slug}/tuition_plans", post(lekta_api::payments::tuition_handlers::create_tuition_plan).get(lekta_api::payments::tuition_handlers::list_tuition_plans))
       .route("api/v1/tuition_plans/{id}", patch(lekta_api::payments::tuition_handlers::create_tuition_plan)
       .delete(lekta_api::payments::tuition_handlers::list_tuition_plans))
       .route("/api/v1/workspaces/{slug}/enrollments", post(lekta_api::payments::enrollment_handlers::initiate_enrollment))
       .route("/api/v1/workspaces/{slug}/enrollments/manual", post(lekta_api::payments::enrollment_handlers::manual_enrollment))
       .route("/api/v1/webhooks/paystack", post(lekta_api::payments::webhook_handler::paystack_webhook))
       .route("/api/v1/workspaces/{slug}/classes", post(lekta_api::classes::handlers::create_class)
       .get(lekta_api::classes::handlers::list_classes))
       .route("/api/v1/classes/{id}", 
       patch(lekta_api::classes::handlers::update_class)
       .get(lekta_api::classes::handlers::get_class)
       .delete(lekta_api::classes::handlers::cancel_class))
       .route("/api/v1/classes/{id}/attendance", post(lekta_api::classes::handlers::mark_attendance))
       .route("/api/v1/classes/{id}/checkin", post(lekta_api::classes::handlers::self_checkin))
       .route("/api/v1/workspaces/{slug}/tutors",
       get(lekta_api::tutors::handlers::list_tutors))
       .route("/api/v1/workspaces/{slug}/tutors/{user_id}",
            get(lekta_api::tutors::handlers::get_tutor_profile))
        .route("/api/v1/workspaces/{slug}/tutors/{user_id}/profile",
            post(lekta_api::tutors::handlers::upsert_tutor_profile))
        .route("/api/v1/workspaces/{slug}/tutors/{user_id}/verify",
            post(lekta_api::tutors::handlers::verify_tutor))
        .route("/api/v1/workspaces/{slug}/tutors/{user_id}/photo-upload-url",
            post(lekta_api::tutors::handlers::tutor_photo_upload_url))
        .route("/api/v1/workspaces/{slug}/tutors/{user_id}/ratings",
            get(lekta_api::tutors::ratings_handlers::list_ratings)
            .post(lekta_api::tutors::ratings_handlers::create_rating))
        .route("/api/v1/workspaces/{slug}/tutor-ratings/{rating_id}",
            delete(lekta_api::tutors::ratings_handlers::delete_rating))
        .route("/api/v1/workspaces/{slug}/assignments",
       post(lekta_api::assignments::handlers::create_assignment)
        .get(lekta_api::assignments::handlers::list_assignment))
        .route("/api/v1/assignments/{id}",
       patch(lekta_api::assignments::handlers::update_assignment)
       .delete(lekta_api::assignments::handlers::delete_assignment))
       .route("/api/v1/assignments/{id}/publish",
       post(lekta_api::assignments::handlers::publish_assignment))
       .route("/api/v1/assignments/{id}/submissions",
       post(lekta_api::assignments::submissions_handlers::submit_assignment)
       .get(lekta_api::assignments::submissions_handlers::list_submissions))
       .route("/api/v1/submissions/{id}/grade",
       post(lekta_api::assignments::submissions_handlers::grade_submission))
       .route("/api/v1/question-banks",
       get(lekta_api::exams::question_banks_handlers::list_banks))
       .route("/api/v1/workspaces/{slug}/question-banks",
       post(lekta_api::exams::question_banks_handlers::create_workspace_bank))
       .route("/api/v1/question-banks/{id}/publish",
       post(lekta_api::exams::question_banks_handlers::publish_bank))
       .route("/api/v1/question-banks/{id}/questions",
       post(lekta_api::exams::questions_handlers::create_questions)
       .get(lekta_api::exams::questions_handlers::list_questions))
       .route("/api/v1/questions/{id}",
       delete(lekta_api::exams::questions_handlers::delete_question))
        .with_state(state);
    
    let addr = SocketAddr::from(([0,0,0,0],port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on {}", addr);
    // let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.ok();
    };
    let terminate = async {
        signal::unix::signal(SignalKind::terminate())
        .expect("SIGTERM handler")
        .recv()
        .await;
    };

    tokio::select! {_ = ctrl_c => {}, _ = terminate => {}}
    tracing::info!("Shutdown signal received, finishing in-flight requests...");

}
