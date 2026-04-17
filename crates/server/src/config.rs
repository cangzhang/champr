use std::{
    fs,
    path::{Path, PathBuf},
};

const SERVER_ADDR_ENV_KEY: &str = "SERVER_ADDR";
const DATABASE_URL_ENV_KEY: &str = "DATABASE_URL";
const JWT_SECRET_ENV_KEY: &str = "JWT_SECRET";
const BOOTSTRAP_ADMIN_EMAIL_ENV_KEY: &str = "BOOTSTRAP_ADMIN_EMAIL";
const BOOTSTRAP_ADMIN_PASSWORD_ENV_KEY: &str = "BOOTSTRAP_ADMIN_PASSWORD";

#[derive(Debug, Clone)]
pub struct Config {
    pub addr: String,
    pub database_url: String,
    pub jwt_secret: String,
    pub bootstrap_admin_email: Option<String>,
    pub bootstrap_admin_password: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            addr: env_or_dotenv(SERVER_ADDR_ENV_KEY).unwrap_or_else(|| "0.0.0.0:3030".to_string()),
            database_url: env_or_dotenv(DATABASE_URL_ENV_KEY)
                .unwrap_or_else(|| "sqlite://data/champr.db".to_string()),
            jwt_secret: env_or_dotenv(JWT_SECRET_ENV_KEY)
                .unwrap_or_else(|| "change-me-in-production".to_string()),
            bootstrap_admin_email: env_or_dotenv(BOOTSTRAP_ADMIN_EMAIL_ENV_KEY),
            bootstrap_admin_password: env_or_dotenv(BOOTSTRAP_ADMIN_PASSWORD_ENV_KEY),
        }
    }
}

fn env_or_dotenv(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .as_deref()
        .and_then(normalize_env_value)
        .or_else(|| find_dotenv_value(key))
}

fn find_dotenv_value(key: &str) -> Option<String> {
    for candidate in env_file_candidates() {
        if let Some(value) = read_env_value(&candidate, key) {
            return Some(value);
        }
    }

    None
}

fn env_file_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(dir) = std::env::current_dir() {
        collect_env_candidates(&dir, &mut candidates);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            collect_env_candidates(dir, &mut candidates);
        }
    }

    candidates
}

fn collect_env_candidates(start: &Path, candidates: &mut Vec<PathBuf>) {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(".env");
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
}

fn read_env_value(path: &Path, key: &str) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    parse_env_value(&contents, key)
}

fn parse_env_value(contents: &str, key: &str) -> Option<String> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };

        if name.trim() != key {
            continue;
        }

        return normalize_env_value(value);
    }

    None
}

fn normalize_env_value(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
