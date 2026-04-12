use std::time::{Duration, SystemTime, UNIX_EPOCH};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::http::{HeaderMap, header::AUTHORIZATION};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use crate::{
    db::{AppState, find_user_by_email, find_user_by_id},
    error::{AppError, AppResult},
    models::{UserRecord, UserRole, normalize_email},
};

const TOKEN_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 7);

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: i64,
    role: String,
    exp: usize,
    iat: usize,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: i64,
    pub role: UserRole,
}

impl AuthenticatedUser {
    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }
}

pub fn hash_password(password: &str) -> AppResult<String> {
    validate_password(password)?;

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| AppError::internal(err))
}

pub fn verify_password(hash: &str, password: &str) -> AppResult<bool> {
    let parsed = PasswordHash::new(hash).map_err(AppError::internal)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn issue_jwt(user: &UserRecord, secret: &str) -> AppResult<String> {
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let exp = issued_at + TOKEN_TTL.as_secs();

    let claims = Claims {
        sub: user.id,
        role: user.role.clone(),
        exp: exp as usize,
        iat: issued_at as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(AppError::internal)
}

pub async fn require_admin(headers: &HeaderMap, state: &AppState) -> AppResult<AuthenticatedUser> {
    let user = require_user(headers, state).await?;
    if user.is_admin() {
        Ok(user)
    } else {
        Err(AppError::Forbidden("admin access required".to_string()))
    }
}

pub async fn require_user(headers: &HeaderMap, state: &AppState) -> AppResult<AuthenticatedUser> {
    authenticate(headers, state)
        .await?
        .ok_or_else(|| AppError::Unauthorized("missing authorization".to_string()))
}

pub async fn authenticate(
    headers: &HeaderMap,
    state: &AppState,
) -> AppResult<Option<AuthenticatedUser>> {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };

    let raw = value
        .to_str()
        .map_err(|_| AppError::Unauthorized("invalid authorization header".to_string()))?;

    if let Some(token) = raw.strip_prefix("Bearer ") {
        return authenticate_bearer(token, state).await.map(Some);
    }

    if let Some(encoded) = raw.strip_prefix("Basic ") {
        return authenticate_basic(encoded, state).await.map(Some);
    }

    Err(AppError::Unauthorized(
        "unsupported authorization scheme".to_string(),
    ))
}

fn validate_password(password: &str) -> AppResult<()> {
    if password.len() < 8 {
        Err(AppError::BadRequest(
            "password must be at least 8 characters".to_string(),
        ))
    } else {
        Ok(())
    }
}

async fn authenticate_bearer(token: &str, state: &AppState) -> AppResult<AuthenticatedUser> {
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid token".to_string()))?
    .claims;

    let Some(user) = find_user_by_id(&state.db, claims.sub)
        .await
        .map_err(AppError::internal)?
    else {
        return Err(AppError::Unauthorized("user not found".to_string()));
    };

    ensure_active_user(user)
}

async fn authenticate_basic(encoded: &str, state: &AppState) -> AppResult<AuthenticatedUser> {
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| AppError::Unauthorized("invalid basic auth encoding".to_string()))?;
    let raw = String::from_utf8(decoded)
        .map_err(|_| AppError::Unauthorized("invalid basic auth payload".to_string()))?;
    let mut parts = raw.splitn(2, ':');
    let email = normalize_email(parts.next().unwrap_or_default());
    let password = parts.next().unwrap_or_default();

    if email.is_empty() || password.is_empty() {
        return Err(AppError::Unauthorized(
            "basic auth requires email and password".to_string(),
        ));
    }

    let Some(user) = find_user_by_email(&state.db, &email)
        .await
        .map_err(AppError::internal)?
    else {
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    };

    if !verify_password(&user.password_hash, password)? {
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    }

    ensure_active_user(user)
}

fn ensure_active_user(user: UserRecord) -> AppResult<AuthenticatedUser> {
    if user.is_active == 0 {
        return Err(AppError::Unauthorized("user is inactive".to_string()));
    }

    let role = UserRole::from_db(&user.role)
        .ok_or_else(|| AppError::internal(format!("invalid role {}", user.role)))?;

    Ok(AuthenticatedUser { id: user.id, role })
}
