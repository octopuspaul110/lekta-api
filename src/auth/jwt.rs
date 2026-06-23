
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{config::Config, error::{AppError, AppResult}};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    sub : String,
    iat : u64,  
    exp : u64,
    jti : String
}

pub fn encode_access_token(user_id : Uuid, config : &Config) -> AppResult<String> {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_err(|e| AppError::Internal(format!("clock error: {}",e)))?
    .as_secs();

    let claims = Claims {
        sub : user_id.to_string(),
        iat : now,
        exp : now + config.access_token_expiry_seconds,
        jti : Uuid::new_v4().to_string(),
    };

    encode(
        &Header::default(), //default is HS256
        &claims, 
        &EncodingKey::from_secret(config.jwt_secret.as_bytes())
    )
    .map_err(|e| AppError::Internal(format!("jwt encode failed: {}",e)))
}

pub fn decode_access_token(token : &str, config : &Config) -> AppResult<Claims> {
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 30;

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|e| {
        use jsonwebtoken::errors::ErrorKind;
        match e.kind() {
            ErrorKind::ExpiredSignature => AppError::Unauthorized("Token expired".into()),
            _ => AppError::Unauthorized("Invalid token".into()),
        }
    })
}