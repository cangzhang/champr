use std::{
    env, fs,
    path::{Path, PathBuf},
};

const BUILD_SERVER_URL_ENV_KEY: &str = "CHAMPR_BUILD_SERVER_URL";
const SERVER_URL_ENV_KEY: &str = "CHAMPR_SERVER_URL";
const DEFAULT_REMOTE_SERVICE_URL: &str = "http://150.230.215.177:3030";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));

    println!("cargo:rerun-if-env-changed={SERVER_URL_ENV_KEY}");
    emit_rerun_for_env_files(&manifest_dir);

    let service_url = env::var(SERVER_URL_ENV_KEY)
        .ok()
        .as_deref()
        .and_then(normalize_service_url)
        .or_else(|| find_dotenv_value(&manifest_dir, SERVER_URL_ENV_KEY))
        .unwrap_or_else(|| DEFAULT_REMOTE_SERVICE_URL.to_string());

    println!("cargo:rustc-env={BUILD_SERVER_URL_ENV_KEY}={service_url}");
}

fn emit_rerun_for_env_files(start: &Path) {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(".env");
        println!("cargo:rerun-if-changed={}", candidate.display());
    }
}

fn find_dotenv_value(start: &Path, key: &str) -> Option<String> {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(".env");
        if let Some(value) = read_env_value(&candidate, key) {
            return Some(value);
        }
    }

    None
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

        return normalize_service_url(value);
    }

    None
}

fn normalize_service_url(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
