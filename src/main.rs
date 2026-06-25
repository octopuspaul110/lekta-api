use std::net::SocketAddr;
use lekta_api::{config::Config, state::AppState};
use tokio::{net::TcpListener, signal::{self, unix::SignalKind}};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use axum::{Router, routing::{get, post}};

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
