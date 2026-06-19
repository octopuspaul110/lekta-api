use std::str::FromStr;
use std::env;
use uuid::Uuid;
use url::Url;
use thiserror::Error;

const DEFAULT_PORT : u16 = 8080;
const DEFAULT_ACCESS_TOKEN_EXPIRY : u64 = 900;

#[derive(Error, Debug)]
pub enum ConfigError{
    #[error("Missing environment variables: {0:?}")]
    Missing(Vec<String>),
    #[error("Invalid value for {var}: {reason}")]
    InvalidValue{ var : String, reason : String},
    #[error("Parse error: {0}")]
    Parse(#[from] std::num::ParseIntError)

}
#[derive(Debug, Clone)]
pub enum Environment {
    Development,
    Staging,
    Production
}
impl FromStr for Environment {
    type Err = ();

    fn from_str(s: &str) -> Result<Environment, Self::Err> {
        match s {
            "development" => Ok(Environment::Development),
            "staging" => Ok(Environment::Staging),
            "production" => Ok(Environment::Production),
            _ => Err(()),
        }
    }
}

impl Environment {
    pub fn is_production(&self) -> bool {
        match self {
            Environment::Production => true,
            _ => false
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub database_url : String,
    pub redis_url : String,
    pub jwt_secret : String,
    pub access_token_expiry_seconds : u64,
    pub refresh_token_expiry_days   : i64,
    pub paystack_secret_key : String,
    pub paystack_webhook_secret : String,
    pub paystack_base_url : String,
    pub anthropic_api_key : String,
    pub anthropic_concurrency_limit : usize,
    pub aws_access_key_id : String,
    pub aws_secret_access_key : String,
    pub aws_region : String,
    pub s3_bucket : String,
    pub ses_from_email : String,
    pub firebase_service_account_json : String,
    pub firebase_project_id : String,
    pub google_oauth_client_id : String,
    pub app_base_url :String,
    pub sentry_dsn : Option<String>,
    pub environment : Environment,
    pub admin_user_ids : Vec<Uuid>,
    pub port : u16,
    pub free_tier_ai_queries_per_day : u32,
    pub worker_count : usize,
    pub behind_proxy : bool,
    pub rust_log : String
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
        .field("database_url", &redact_url(&self.database_url))
        .field("redis_url", &redact_url(&self.redis_url))
        .field("jwt_secret", &"<redacted>")
        .field("access_token_expiry_seconds", &self.access_token_expiry_seconds)
        .field("refresh_token_expiry_days", &self.refresh_token_expiry_days)
        .field("paystack_secret_key", &"<redacted>")
        .field("paystack_webhook_secret", &"<redacted>")
        .field("paystack_base_url", &self.paystack_base_url)
        .field("anthropic_api_key", &"<redacted>")
        .field("anthropic_concurrency_limit", &self.anthropic_concurrency_limit)
        .field("aws_access_key_id", &"<redacted>")
        .field("aws_secret_access_key", &"<redacted>")
        .field("aws_region", &self.aws_region)
        .field("s3_bucket", &self.s3_bucket)
        .field("ses_from_email", &self.ses_from_email)
        .field("firebase_service_account_json", &"<redacted>")
        .field("firebase_project_id", &self.firebase_project_id)
        .field("google_oauth_client_id", &self.google_oauth_client_id)
        .field("app_base_url", &self.app_base_url)
        .field("sentry_dsn", &"<redacted>")
        .field("environment", &self.environment)
        .field("admin_user_ids", &self.admin_user_ids)
        .field("port", &self.port)
        .field("free_tier_ai_queries_per_day", &self.free_tier_ai_queries_per_day)
        .field("worker_count", &self.worker_count)
        .field("behind_proxy", &self.behind_proxy)
        .field("rust_log", &self.rust_log)
        .finish()
    }
}

fn redact_url(url : &str) -> String {
    if let Ok(mut parsed) = Url::parse(url) {
        if parsed.password().is_some() {
            let _ = parsed.set_password(Some("***"));
        }
        parsed.to_string()
    } else {
        "***INVALID_URL***".into()
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError>{
        dotenvy::dotenv().ok();

        let mut missing = Vec::new();
        let mut get = |key : &str| -> String {
            env::var(key).unwrap_or_else(|_| {
                missing.push(key.to_string());
                String::new()
            })
        };

        let database_url = get("DATABASE_URL");
        let redis_url = get("REDIS_URL");
        let jwt_secret = get("JWT_SECRET");
        let paystack_secret_key = get("PAYSTACK_SECRET_KEY");
        let paystack_webhook_secret = get("PAYSTACK_WEBHOOK_SECRET");
        let anthropic_api_key = get("ANTHROPIC_API_KEY");
        let aws_access_key_id = get("AWS_ACCESS_KEY_ID");
        let aws_secret_access_key = get("AWS_SECRET_ACCESS_KEY");
        let aws_region = get("AWS_REGION");
        let s3_bucket = get("S3_BUCKET");
        let ses_from_email = get("SES_FROM_EMAIL");
        let firebase_service_account_json = get("FIREBASE_SERVICE_ACCOUNT_JSON");
        let firebase_project_id = get("FIREBASE_PROJECT_ID");
        let google_oauth_client_id = get("GOOGLE_OAUTH_CLIENT_ID");
        let app_base_url = get("APP_BASE_URL");
        let sentry_dsn = env::var("SENTRY_DSN").ok();
        let environment = get("ENVIRONMENT");
        
        if !jwt_secret.is_empty() && jwt_secret.len() < 32 {
            return Err(ConfigError::InvalidValue { var: "JWT_SECRET".into(), reason: "must be at least 32 bytes".into()});
        }
        if !missing.is_empty() {
            return Err(ConfigError::Missing(missing));
        }

        let port : u16 = env::var("PORT")
        .unwrap_or_else(|_| {
            DEFAULT_PORT.to_string()
        })
        .parse()
        .map_err(|e| ConfigError::Parse(e))?;

        let access_token_expiry_seconds : u64 = env::var("ACCESS_TOKEN_EXPIRY_SECONDS")
        .ok()
        .and_then(|s|s.parse().ok())
        .unwrap_or(DEFAULT_ACCESS_TOKEN_EXPIRY);

        let refresh_token_expiry_days : i64 = env::var("REFRESH_TOKEN_EXPIRY_DAYS")
        .ok()
        .and_then(|s|s.parse().ok())
        .unwrap_or(30);

        let paystack_base_url : String = env::var("PAYSTACK_BASE_URL")
        .unwrap_or(String::from("https://api.paystack.co"));

        let anthropic_concurrency_limit : usize = env::var("ANTHROPIC_CONCURRENCY_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

        let free_tier_ai_queries_per_day = env::var("FREE_TIER_AI_QUERIES_PER_DAY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

        let worker_count : usize = env::var("WORKER_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);

        if !(1..=32).contains(&worker_count) {
            return Err(ConfigError::InvalidValue { var: "WORKER_COUNT".into(), reason: format!("must be 1-32, got {}",worker_count) });
        }

        let behind_proxy : bool = env::var("BEHIND_PROXY")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
        
        let admin_user_ids : Vec<Uuid> = env::var("ADMIN_USER_IDS")
        .unwrap_or_default()
        .split(',')
        .filter(|s|{
            !s.trim().is_empty()
        })
        .map(|s|
            s.trim()
            .parse::<Uuid>())
        .collect::<Result<Vec<_>,_>>()
        .map_err(|e| ConfigError::InvalidValue { var: "ADMIN_USER_IDS".into(), reason: e.to_string() })?;

        if env::var("RAILWAY_ENVIRONMENT").is_ok() {
            if (paystack_secret_key).starts_with("sk_test_") {
                tracing::warn!("App running on Railway with paystack TEST key, you might want to switch to original secret key");
            }
            if !(app_base_url.starts_with("https://")) {
                return Err(ConfigError::InvalidValue { var : "APP_BASE_URL".into(), reason : format!("must use https:// in production")});
            }
        }

        let rust_log = env::var("RUST_LOG").unwrap_or_else(|_| { "lekta_api=debug,sqlx=warn,tower_http=info".into()});

        let environment : Environment = Environment::from_str(&environment).unwrap_or(Environment::Development);

        Ok( 
            Self {
                database_url,
                redis_url,
                jwt_secret,
                access_token_expiry_seconds,
                refresh_token_expiry_days,
                paystack_secret_key,
                paystack_webhook_secret,
                paystack_base_url,
                anthropic_api_key,
                anthropic_concurrency_limit,
                aws_access_key_id,
                aws_secret_access_key,
                aws_region,
                s3_bucket,
                ses_from_email,
                firebase_service_account_json,
                firebase_project_id,
                google_oauth_client_id,
                app_base_url,
                sentry_dsn,
                environment,
                admin_user_ids,
                port,
                free_tier_ai_queries_per_day,
                worker_count,
                behind_proxy,
                rust_log
            }
        )
    }
}
