use axum::http::{HeaderMap, StatusCode};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub email: Option<String>,
    pub exp: usize,
}

#[derive(Debug, Deserialize)]
pub struct SendCodeRequest {
    pub email: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyCodeRequest {
    pub email: String,
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub email_or_login: String,
}

#[derive(Debug, Deserialize)]
pub struct ConfirmResetPasswordRequest {
    pub email_or_login: String,
    pub code: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

fn get_jwt_secret() -> Vec<u8> {
    std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "signal-secret-jwt-key-2026".to_string())
        .into_bytes()
}

pub fn create_jwt(user_id: &str, username: &str, email: Option<&str>) -> Result<String, String> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(30))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        email: email.map(|s| s.to_string()),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&get_jwt_secret()),
    )
    .map_err(|e| e.to_string())
}

pub fn verify_jwt(token: &str) -> Result<Claims, String> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(&get_jwt_secret()),
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
    if trimmed.len() > 128 {
        return Err("Пароль не должен превышать 128 символов");
    }
    Ok(())
}

pub fn validate_email(email: &str) -> Result<String, &'static str> {
    let clean = email.trim().to_lowercase();
    if clean.is_empty() {
        return Err("Email обязателен для заполнения");
    }
    if clean.contains(' ') || clean.contains('\t') || clean.contains('\n') {
        return Err("Email не должен содержать пробелы");
    }
    if !email_address::EmailAddress::is_valid(&clean) {
        return Err("Укажите корректный адрес электронной почты (например, name@example.com)");
    }
    let parts: Vec<&str> = clean.split('@').collect();
    if parts.len() != 2 {
        return Err("Некорректный формат email");
    }
    let domain = parts[1];
    if !domain.contains('.') {
        return Err("Email должен содержать доменную зону (например, .com, .ru)");
    }
    let domain_parts: Vec<&str> = domain.split('.').collect();
    let tld = domain_parts.last().unwrap_or(&"");
    if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("Некорректная доменная зона почты");
    }
    Ok(clean)
}
