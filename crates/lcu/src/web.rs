use std::{
    collections::HashMap,
    fs,
    io::{self, Cursor},
    path::Path,
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

pub const SERVICE_URL: &str = "http://150.230.215.177:3030";

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

pub async fn fetch_champion_list() -> Result<ChampionsMap, FetchError> {
    let url = format!("{SERVICE_URL}/api/data-dragon/champions",);
    if let Ok(resp) = reqwest::get(url).await {
        if let Ok(data) = resp.json::<ChampionsMap>().await {
            return Ok(data);
        }
    }

    Err(FetchError::Failed)
}

pub async fn init_for_ui() -> Result<(ChampionsMap, Vec<DataDragonRune>), FetchError> {
    try_join(fetch_champion_list(), fetch_data_dragon_runes()).await
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
    let url = format!("{SERVICE_URL}/api/source/{source}/champion-alias/{champion}");
    list_builds(&url).await
}

pub async fn list_builds_by_id(
    source: &String,
    champion_id: i64,
) -> Result<Vec<builds::BuildSection>, FetchError> {
    let url = format!("{SERVICE_URL}/api/source/{source}/champion-id/{champion_id}");
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

pub async fn fetch_data_dragon_runes() -> Result<Vec<DataDragonRune>, FetchError> {
    if let Ok(resp) = reqwest::get(format!("{SERVICE_URL}/api/data-dragon/runes")).await {
        if let Ok(data) = resp.json::<Vec<DataDragonRune>>().await {
            return Ok(data);
        }
    }

    Err(FetchError::Failed)
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
