pub fn generate_refresh_token() -> String {
    use rand::RngCore;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD,Engine};

    let mut bytes = [0u8;32];
    rand::thread_rng().fill_bytes(&mut bytes);

    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_refresh_token(raw : &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());

    hex::encode(hasher.finalize())
}