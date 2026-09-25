use axum::http::{HeaderMap, StatusCode};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub exp: usize,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GoogleAuthRequest {
    pub credential: String,
}

#[derive(Debug, Serialize)]
pub struct AuthConfigResponse {
    pub google_client_id: Option<String>,
    pub google_auth_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

static DYNAMIC_JWT_SECRET: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

fn get_jwt_secret() -> &'static [u8] {
    DYNAMIC_JWT_SECRET.get_or_init(|| {
        if let Ok(val) = std::env::var("JWT_SECRET") {
            let trimmed = val.trim();
            if !trimmed.is_empty() && trimmed != "super_secret_jwt_random_key_replace_in_prod" {
                return trimmed.as_bytes().to_vec();
            }
        }
        let random_secret: String = (0..64)
            .map(|_| {
                const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()-_=+";
                CHARSET[fastrand::usize(..CHARSET.len())] as char
            })
            .collect();
        random_secret.into_bytes()
    })
}

pub fn create_jwt(user_id: &str, username: &str) -> Result<String, String> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(30))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(get_jwt_secret()),
    )
    .map_err(|e| e.to_string())
}

pub fn verify_jwt(token: &str) -> Result<Claims, String> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(get_jwt_secret()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| e.to_string())
}

pub fn extract_claims_from_headers(headers: &HeaderMap) -> Result<Claims, (StatusCode, &'static str)> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

    let token = if auth_header.starts_with("Bearer ") {
        &auth_header[7..]
    } else {
        auth_header
    };

    verify_jwt(token).map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))
}

pub fn hash_password(password: &str) -> Result<String, StatusCode> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

pub fn validate_username(username: &str) -> Result<(), &'static str> {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err("Имя пользователя обязательно для заполнения");
    }
    if trimmed.contains(' ') || trimmed.contains('\t') || trimmed.contains('\n') {
        return Err("Имя пользователя не должно содержать пробелы");
    }
    if trimmed.len() < 3 {
        return Err("Имя пользователя должно содержать не менее 3 символов");
    }
    if trimmed.len() > 30 {
        return Err("Имя пользователя не должно превышать 30 символов");
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("Имя пользователя может содержать только латинские буквы, цифры, '_' и '-'");
    }
    Ok(())
}

pub fn validate_password(password: &str) -> Result<(), &'static str> {
    let trimmed = password.trim();
    if trimmed.is_empty() {
        return Err("Пароль обязателен для заполнения");
    }
    if trimmed.len() < 6 {
        return Err("Пароль должен содержать не менее 6 символов");
    }
    if trimmed.len() > 72 {
        return Err("Пароль не должен превышать 72 символа");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_create_and_verify() {
        let user_id = "test-user-uuid-123";
        let username = "alex_smith";
        let token = create_jwt(user_id, username).expect("JWT creation should succeed");
        assert!(!token.is_empty());

        let claims = verify_jwt(&token).expect("JWT verification should succeed");
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.username, username);
    }

    #[test]
    fn test_validate_username() {
        assert!(validate_username("alex_123").is_ok());
        assert!(validate_username("john-doe").is_ok());
        assert!(validate_username("al").is_err());
        assert!(validate_username("alex smith").is_err());
        assert!(validate_username("alex@smith").is_err());
    }

    #[test]
    fn test_validate_password() {
        assert!(validate_password("123456").is_ok());
        assert!(validate_password("short").is_err());
        assert!(validate_password("").is_err());
    }

    #[test]
    fn test_password_hash_and_verify() {
        let pass = "strong_password_99";
        let hash = hash_password(pass).expect("Hashing should succeed");
        assert!(verify_password(pass, &hash));
        assert!(!verify_password("wrong_password", &hash));
    }
}
