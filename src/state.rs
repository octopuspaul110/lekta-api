use std::sync::Arc;
use std::time::Duration;
use sqlx::PgPool;
use redis::aio::ConnectionManager;

use crate::config::{Config};
use crate::error::{AppError, AppResult};
use crate::payments::paystack_client::PaystackClient;
use crate::storage::s3_client::S3Client;


#[derive(Clone)]
pub struct AppState {
    pub config      : Arc<Config>,
    pub db          : PgPool,
    pub redis       : ConnectionManager,
    pub http        : reqwest::Client,
    pub paystack    : Arc<PaystackClient>,
    // pub anthropic: Arc<AnthropicClient>,
    pub s3          : Arc<S3Client>,
    // pub ses: Arc<aws_sdk_ses::Client>,
    // pub fcm: Arc<FcmClient>,
}
impl AppState {
    pub async fn new(config : Config) -> AppResult<Self> {
        let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;

        sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .map_err(|e|AppError::Internal(format!("migration failed: {}",e)))?;

        let redis_client = redis::Client::open(config.redis_url.as_str())?;

        let redis = ConnectionManager::new(redis_client).await?;

        let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()?;

        let paystack = Arc::new(PaystackClient::new(
            http.clone(), 
            config.paystack_secret_key.clone(), 
            config.paystack_base_url.clone()),
        );

        let s3 = Arc::new(
            S3Client::new(config.s3_bucket.clone(), config.aws_region.clone()).await?
        );

        Ok(
            Self { config: Arc::new(config), db, redis, http, paystack, s3 }
        )
    
    }
}