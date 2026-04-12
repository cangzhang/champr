use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "admin" => Some(Self::Admin),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct SourceRecord {
    pub id: i64,
    pub key: String,
    pub label: String,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ChampionDataRecord {
    pub id: i64,
    pub source_id: i64,
    pub source_key: String,
    pub mode: String,
    pub version: String,
    pub champion_id: i64,
    pub champion_alias: String,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: PublicUser,
}

#[derive(Debug, Serialize)]
pub struct PublicUser {
    pub id: i64,
    pub email: String,
    pub role: UserRole,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<UserRecord> for PublicUser {
    type Error = String;

    fn try_from(value: UserRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            email: value.email,
            role: UserRole::from_db(&value.role)
                .ok_or_else(|| format!("unsupported role {}", value.role))?,
            is_active: value.is_active != 0,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub role: UserRole,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub password: Option<String>,
    pub role: Option<UserRole>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PublicSource {
    pub id: i64,
    pub label: String,
    pub value: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<SourceRecord> for PublicSource {
    fn from(value: SourceRecord) -> Self {
        Self {
            id: value.id,
            label: value.label,
            value: value.key,
            is_active: value.is_active != 0,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSourceRequest {
    pub key: String,
    pub label: String,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSourceRequest {
    pub key: Option<String>,
    pub label: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ModeQuery {
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UpsertChampionDataRequest {
    pub source_id: Option<i64>,
    pub source_key: Option<String>,
    pub champion_id: i64,
    pub champion_alias: String,
    pub mode: Option<String>,
    pub version: String,
    pub content: Value,
}

#[derive(Debug, Deserialize)]
pub struct BatchUpsertChampionDataRequest {
    pub items: Vec<UpsertChampionDataRequest>,
}

#[derive(Debug, Serialize)]
pub struct BatchUpsertChampionDataResponse {
    pub items: Vec<ChampionDataResponse>,
}

#[derive(Debug, Serialize)]
pub struct ChampionDataResponse {
    pub id: i64,
    pub source_id: i64,
    pub source: String,
    pub mode: String,
    pub version: String,
    pub champion_alias: String,
    pub champion_id: String,
    pub content: Value,
    pub created_at: String,
    pub updated_at: String,
}

impl ChampionDataResponse {
    pub fn try_from_record(value: ChampionDataRecord) -> Result<Self, String> {
        let content = serde_json::from_str(&value.payload)
            .map_err(|err| format!("invalid stored payload: {err}"))?;

        Ok(Self {
            id: value.id,
            source_id: value.source_id,
            source: value.source_key,
            mode: value.mode,
            version: value.version,
            champion_alias: value.champion_alias,
            champion_id: value.champion_id.to_string(),
            content,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

pub fn normalize_email(value: &str) -> String {
    value.trim().to_lowercase()
}

pub fn normalize_source_key(value: &str) -> String {
    value.trim().to_lowercase()
}

pub fn normalize_alias(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

pub fn normalize_mode(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .unwrap_or("ranked")
        .to_lowercase()
}

pub fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

pub fn db_bool(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
