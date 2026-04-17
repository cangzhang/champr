use std::{
    collections::HashMap,
    fs,
    io::{self, Cursor},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{anyhow, Context};
use flate2::read::GzDecoder;
use futures::future::join_all;
use futures::future::try_join;
use kv_log_macro::{error, info, warn};
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar::Archive;

use crate::builds::{self, BuildData, ItemBuild};

const BUILD_SERVER_URL: &str = env!("CHAMPR_BUILD_SERVER_URL");
const DATA_DRAGON_BASE_URL: &str = "https://ddragon.leagueoflegends.com";
const DEFAULT_LOCAL_SERVICE_URL: &str = "http://127.0.0.1:3030";
const SERVER_URL_ENV_KEY: &str = "CHAMPR_SERVER_URL";

static SERVICE_URL: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum FetchError {
    Failed,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampInfo {
    pub version: String,
    pub id: String,
    pub key: String,
    pub name: String,
    pub title: String,
    // pub blurb: String,
    // pub info: Info,
    pub image: Image,
    pub tags: Vec<String>,
    // pub partype: String,
    // pub stats: Stats,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub full: String,
    pub sprite: String,
    pub group: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub type ChampionsMap = HashMap<String, ChampInfo>;

#[derive(Debug, Deserialize)]
struct ChampionListResponse {
    data: ChampionsMap,
}

pub fn service_url() -> &'static str {
    SERVICE_URL.get_or_init(resolve_service_url).as_str()
}

fn resolve_service_url() -> String {
    let runtime_env = std::env::var(SERVER_URL_ENV_KEY).ok();
    let env_file = find_env_file_value(SERVER_URL_ENV_KEY);

    resolve_service_url_from_sources(
        runtime_env.as_deref(),
        env_file.as_deref(),
        cfg!(debug_assertions),
        BUILD_SERVER_URL,
    )
}

fn resolve_service_url_from_sources(
    runtime_env: Option<&str>,
    env_file: Option<&str>,
    use_local_default: bool,
    build_service_url: &str,
) -> String {
    runtime_env
        .and_then(normalize_service_url)
        .or_else(|| env_file.and_then(normalize_service_url))
        .unwrap_or_else(|| {
            if use_local_default {
                DEFAULT_LOCAL_SERVICE_URL.to_string()
            } else {
                build_service_url.to_string()
            }
        })
}

fn find_env_file_value(key: &str) -> Option<String> {
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

async fn fetch_latest_data_dragon_version() -> Result<String, FetchError> {
    if let Ok(resp) = reqwest::get(format!("{DATA_DRAGON_BASE_URL}/api/versions.json")).await {
        if let Ok(versions) = resp.json::<Vec<String>>().await {
            if let Some(version) = versions.into_iter().next() {
                return Ok(version);
            }
        }
    }

    Err(FetchError::Failed)
}

async fn fetch_champion_list_for_version(version: &str) -> Result<ChampionsMap, FetchError> {
    let url = format!("{DATA_DRAGON_BASE_URL}/cdn/{version}/data/en_US/champion.json");
    if let Ok(resp) = reqwest::get(url).await {
        if let Ok(data) = resp.json::<ChampionListResponse>().await {
            return Ok(data.data);
        }
    }

    Err(FetchError::Failed)
}

pub async fn fetch_champion_list() -> Result<ChampionsMap, FetchError> {
    let version = fetch_latest_data_dragon_version().await?;
    fetch_champion_list_for_version(&version).await
}

pub async fn init_for_ui() -> Result<(ChampionsMap, Vec<DataDragonRune>), FetchError> {
    let version = fetch_latest_data_dragon_version().await?;
    try_join(
        fetch_champion_list_for_version(&version),
        fetch_data_dragon_runes_for_version(&version),
    )
    .await
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListBuildsResp {
    pub id: i64,
    pub source: String,
    pub version: String,
    #[serde(rename = "champion_alias")]
    pub champion_alias: String,
    #[serde(rename = "champion_id")]
    pub champion_id: String,
    pub content: Vec<builds::BuildSection>,
}

pub async fn list_builds(url: &String) -> Result<Vec<builds::BuildSection>, FetchError> {
    match reqwest::get(url).await {
        Ok(resp) => match resp.json::<ListBuildsResp>().await {
            Ok(resp) => Ok(resp.content),
            Err(e) => {
                println!("list_builds_by_alias: {:?}", e);
                Err(FetchError::Failed)
            }
        },
        Err(err) => {
            println!("fetch source list: {:?}", err);
            Err(FetchError::Failed)
        }
    }
}

pub async fn list_builds_by_alias(
    source: &String,
    champion: &String,
) -> Result<Vec<builds::BuildSection>, FetchError> {
    let url = format!(
        "{}/api/source/{source}/champion-alias/{champion}",
        service_url()
    );
    list_builds(&url).await
}

pub async fn list_builds_by_id(
    source: &String,
    champion_id: i64,
) -> Result<Vec<builds::BuildSection>, FetchError> {
    let url = format!(
        "{}/api/source/{source}/champion-id/{champion_id}",
        service_url()
    );
    list_builds(&url).await
}

pub async fn fetch_champion_runes(
    source: String,
    champion: String,
) -> Result<BuildData, FetchError> {
    let meta = list_builds_by_alias(&source, &champion).await?;
    let runes = meta.iter().flat_map(|b| b.runes.clone()).collect();
    let builds: Vec<ItemBuild> = meta.iter().flat_map(|b| b.item_builds.clone()).collect();
    Ok(BuildData(runes, builds))
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub runes: Vec<SlotRune>,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRune {
    pub id: u64,
    pub key: String,
    pub icon: String,
    pub name: String,
    pub short_desc: String,
    pub long_desc: String,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDragonRune {
    pub id: u64,
    pub key: String,
    pub icon: String,
    pub name: String,
    pub slots: Vec<Slot>,
}

async fn fetch_data_dragon_runes_for_version(
    version: &str,
) -> Result<Vec<DataDragonRune>, FetchError> {
    let url = format!("{DATA_DRAGON_BASE_URL}/cdn/{version}/data/en_US/runesReforged.json");
    if let Ok(resp) = reqwest::get(url).await {
        if let Ok(data) = resp.json::<Vec<DataDragonRune>>().await {
            return Ok(data);
        }
    }

    Err(FetchError::Failed)
}

pub async fn fetch_data_dragon_runes() -> Result<Vec<DataDragonRune>, FetchError> {
    let version = fetch_latest_data_dragon_version().await?;
    fetch_data_dragon_runes_for_version(&version).await
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestRelease {
    pub name: String,
    pub tag_name: String,
    pub html_url: String,
}

pub async fn fetch_latest_release() -> Result<LatestRelease, FetchError> {
    let client = reqwest::Client::new();

    match client
        .get("https://api.github.com/repos/cangzhang/champ-r/releases/latest".to_string())
        .header(USER_AGENT, "ChampR_rs")
        .send()
        .await
    {
        Ok(resp) => resp.json::<LatestRelease>().await.map_err(|err| {
            error!("latest release serialize: {:?}", err);
            FetchError::Failed
        }),
        Err(err) => {
            error!("fetch latest release: {:?}", err);
            Err(FetchError::Failed)
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dist {
    pub tarball: String,
    pub file_count: i64,
    pub unpacked_size: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source_version: String,
    pub description: String,
    pub dist: Dist,
}

pub async fn get_remote_package_data(source: &String) -> Result<(String, String), reqwest::Error> {
    let r = reqwest::get(format!(
        "https://mirrors.cloud.tencent.com/npm/@champ-r/{source}/latest"
    ))
    .await?;
    let pak = r.json::<Package>().await?;
    Ok((pak.version, pak.dist.tarball))
}

pub async fn download_and_extract_tgz(url: &str, output_dir: &str) -> io::Result<()> {
    // Download the file
    let response = reqwest::get(url).await.unwrap();
    let content = response.bytes().await.unwrap();
    // Cursor allows us to read bytes as a stream
    let cursor = Cursor::new(content);
    // Decompress gzip
    let gz = GzDecoder::new(cursor);
    // Extract tarball
    let mut archive = Archive::new(gz);
    archive.unpack(output_dir)?;

    Ok(())
}

pub async fn read_local_build_file(file_path: String) -> anyhow::Result<Value> {
    use tokio::fs::File;
    use tokio::io::AsyncReadExt;

    let mut file = File::open(&file_path)
        .await
        .with_context(|| format!("Failed to open file: {}", &file_path))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .await
        .with_context(|| format!("Failed to read from file: {}", &file_path))?;
    let parsed = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse JSON in file: {}", &file_path))?;

    Ok(parsed)
}

pub async fn read_from_local_folder(
    output_dir: &str,
) -> anyhow::Result<Vec<Vec<builds::BuildSection>>> {
    let paths = fs::read_dir(output_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path().is_file()
                && entry.file_name() != "package.json"
                && entry.file_name() != "index.json"
        })
        .map(|entry| entry.path().into_os_string().into_string().unwrap())
        .collect::<Vec<String>>();
    let tasks: Vec<_> = paths
        .into_iter()
        .map(|p| read_local_build_file(p.clone()))
        .collect();
    let results = join_all(tasks).await;

    let files = results
        .into_iter()
        .filter_map(|result| match result {
            Ok(value) => match serde_json::from_value::<Vec<builds::BuildSection>>(value) {
                Ok(builds) => Some(builds),
                Err(e) => {
                    warn!("Error: {:?}", e);
                    None
                }
            },
            Err(e) => {
                warn!("Error: {:?}", e);
                None
            }
        })
        .collect();

    Ok(files)
}

pub async fn download_tar_and_apply_for_source(
    source: &String,
    lol_dir: Option<String>,
    is_tencent: bool,
) -> anyhow::Result<()> {
    let (_version, tar_url) = get_remote_package_data(source).await?;

    info!("found download url for {}, {}", &source, &tar_url);

    let output_dir = format!(".npm/{source}");
    let output_path = Path::new(&output_dir);

    if let Err(err) = fs::create_dir_all(output_path) {
        error!("create output dir: {:?}", err);
        return Err(anyhow!("create output dir: {:?}", err));
    }

    download_and_extract_tgz(&tar_url, &output_dir).await?;
    let dest_folder = format!("{}/package", &output_dir);
    let files = read_from_local_folder(&dest_folder).await?;

    info!("found {} builds for {}", files.len(), source);

    if lol_dir.is_some() {
        let dir = lol_dir.unwrap();

        files.iter().for_each(|sections| {
            let sections = sections.clone();
            let alias = sections[0].alias.clone();
            builds::apply_builds_from_data(sections, &dir.clone(), source, &alias, is_tencent);
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_value_supports_quotes_and_comments() {
        let contents = r#"
            # ignored
            CHAMPR_SERVER_URL="http://127.0.0.1:3030/"
        "#;

        assert_eq!(
            parse_env_value(contents, SERVER_URL_ENV_KEY).as_deref(),
            Some("http://127.0.0.1:3030")
        );
    }

    #[test]
    fn resolve_service_url_prefers_runtime_env() {
        assert_eq!(
            resolve_service_url_from_sources(
                Some("http://runtime.local:3030/"),
                Some("http://env-file.local:3030"),
                true,
                "http://build.local:3030",
            ),
            "http://runtime.local:3030"
        );
    }

    #[test]
    fn resolve_service_url_prefers_env_file_over_defaults() {
        assert_eq!(
            resolve_service_url_from_sources(
                None,
                Some("http://env-file.local:3030/"),
                true,
                "http://build.local:3030",
            ),
            "http://env-file.local:3030"
        );
    }

    #[test]
    fn resolve_service_url_uses_local_default_for_debug_runs() {
        assert_eq!(
            resolve_service_url_from_sources(None, None, true, "http://build.local:3030"),
            DEFAULT_LOCAL_SERVICE_URL
        );
    }

    #[test]
    fn resolve_service_url_uses_build_url_for_packaged_runs() {
        assert_eq!(
            resolve_service_url_from_sources(None, None, false, "http://build.local:3030"),
            "http://build.local:3030"
        );
    }

    #[tokio::test]
    async fn apply_builds_for_riot_server() -> anyhow::Result<()> {
        femme::with_level(femme::LevelFilter::Info);

        let source = String::from("op.gg");
        download_tar_and_apply_for_source(&source, Some(String::from(".local_builds")), false)
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn apply_builds_for_tencent_server() -> anyhow::Result<()> {
        femme::with_level(femme::LevelFilter::Info);

        let source = String::from("op.gg");
        download_tar_and_apply_for_source(&source, Some(String::from(".local_builds")), true)
            .await?;

        Ok(())
    }
}
