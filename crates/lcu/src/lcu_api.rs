use bytes::Bytes;
use lazy_static::lazy_static;
use reqwest::{Client, Url};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde::de::DeserializeOwned;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, time::Duration};

use crate::{
    builds::{ItemBuild, Rune},
    lcu_error::LcuError,
    web::FetchError,
};

lazy_static! {
    static ref CLIENT: reqwest::Client = {
        reqwest::Client::builder()
            .use_rustls_tls()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(2))
            .no_proxy()
            .build()
            .unwrap()
    };
}

pub fn make_client() -> &'static reqwest::Client {
    &CLIENT
}

pub async fn make_get_request<T: DeserializeOwned>(endpoint: &String) -> Result<T, LcuError> {
    let client = make_client();
    client
        .get(endpoint)
        .version(reqwest::Version::HTTP_2)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(LcuError::from)?
        .json()
        .await
        .map_err(LcuError::from)
}

pub async fn get_session(auth_url: &String) -> Result<Option<i64>, LcuError> {
    let endpoint = format!("{auth_url}/lol-champ-select/v1/session");
    let resp: Value = make_get_request(&endpoint).await?;

    if let Some(cell_id) = resp["localPlayerCellId"].as_i64() {
        let my_team = resp["myTeam"].as_array().unwrap();
        for i in my_team {
            let id = i["cellId"].as_i64().unwrap();
            if id == cell_id {
                let champ_id = i.get("championId").unwrap().as_i64().unwrap();
                return Ok(Some(champ_id));
            }
        }

        let actions = resp["actions"].as_array().unwrap();
        for row in actions {
            for i in row.as_array().unwrap() {
                let id = i["actorCellId"].as_i64().unwrap();
                if id == cell_id && i["type"].as_str().unwrap() != "ban" {
                    let champ_id = i.get("championId").unwrap().as_i64().unwrap();
                    return Ok(Some(champ_id));
                }
            }
        }
    };

    Ok(None)
}

pub async fn apply_rune(endpoint: String, rune: Rune) -> Result<(), LcuError> {
    let runes: Value = make_get_request(&format!("{endpoint}/lol-perks/v1/pages")).await?;

    let mut id = 0;
    for r in runes.as_array().unwrap() {
        if r["current"].as_bool().unwrap() {
            id = r["id"].as_i64().unwrap();
            break;
        }
        if r["isDeletable"].as_bool().unwrap() {
            id = r["id"].as_i64().unwrap();
        }
    }

    let client = make_client();
    if id > 0 {
        let _ = client
            .delete(format!("{endpoint}/lol-perks/v1/pages/{id}"))
            .version(reqwest::Version::HTTP_2)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
    }

    let _ = client
        .post(format!("{endpoint}/lol-perks/v1/pages"))
        .version(reqwest::Version::HTTP_2)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&rune)
        .send()
        .await?;
    Ok(())
}

pub async fn appy_rune_and_builds(
    _endpoint: String,
    _rune: Rune,
    _builds: Vec<ItemBuild>,
) -> Result<(), LcuError> {
    Ok(())
}

pub async fn get_rune_image(endpoint: String, icon_path: String) -> Result<Bytes, FetchError> {
    let client = make_client();
    let url = format!("{endpoint}/lol-game-data/assets/v1/{icon_path}");
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(bytes) = resp.bytes().await {
            return Ok(bytes);
        }
    }

    Err(FetchError::Failed)
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summoner {
    pub account_id: i64,
    pub display_name: String,
    pub game_name: String,
    pub internal_name: String,
    pub name_change_flag: bool,
    pub percent_complete_for_next_level: i64,
    pub privacy: String,
    pub profile_icon_id: i64,
    pub puuid: String,
    pub reroll_points: RerollPoints,
    pub summoner_id: i64,
    pub summoner_level: i64,
    pub tag_line: String,
    pub unnamed: bool,
    pub xp_since_last_level: i64,
    pub xp_until_next_level: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerollPoints {
    pub current_points: i64,
    pub max_rolls: i64,
    pub number_of_rolls: i64,
    pub points_cost_to_roll: i64,
    pub points_to_reroll: i64,
}

pub async fn get_current_summoner(endpoint: &String) -> Result<Summoner, LcuError> {
    let url = format!("{endpoint}/lol-summoner/v1/current-summoner");
    make_get_request(&url).await
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerChampion {
    pub active: bool,
    pub alias: String,
    pub ban_vo_path: String,
    pub base_load_screen_path: String,
    pub base_splash_path: String,
    pub bot_enabled: bool,
    pub choose_vo_path: String,
    pub disabled_queues: Vec<Value>,
    pub free_to_play: bool,
    pub id: i64,
    pub name: String,
    pub ownership: Ownership,
    pub passive: Passive,
    // pub purchased: i64,
    pub ranked_play_enabled: bool,
    pub roles: Vec<Value>,
    pub skins: Vec<Value>,
    pub spells: Vec<Value>,
    pub square_portrait_path: String,
    pub stinger_sfx_path: String,
    pub tactical_info: TacticalInfo,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ownership {
    pub loyalty_reward: bool,
    pub owned: bool,
    pub rental: Rental,
    #[serde(rename = "xboxGPReward")]
    pub xbox_gpreward: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rental {
    pub end_date: i64,
    // pub purchase_date: i64,
    pub rented: bool,
    pub win_count_remaining: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passive {
    pub description: String,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TacticalInfo {
    pub damage_type: String,
    pub difficulty: i64,
    pub style: i64,
}

pub async fn list_available_champions(
    endpoint: &String,
    summoner_id: i64,
) -> Result<Vec<SummonerChampion>, LcuError> {
    let url = format!("{endpoint}/lol-champions/v1/inventories/{summoner_id}/champions");
    make_get_request(&url).await
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Perk {
    pub icon_path: String,
    pub id: i64,
    pub long_desc: String,
    pub name: String,
    pub recommendation_descriptor: String,
    pub short_desc: String,
    pub slot_type: String,
    pub style_id: i64,
    pub style_id_name: String,
    pub tooltip: String,
}

pub async fn list_all_perks(endpoint: &String) -> Result<Vec<Perk>, LcuError> {
    let url = format!("{endpoint}/lol-perks/v1/perks");
    make_get_request(&url).await
}

pub async fn fetch_image_data(url: &String) -> Result<Bytes, FetchError> {
    let client = make_client();
    match client.get(url).send().await.map_err(|_| FetchError::Failed) {
        Ok(res) => {
            if res.status().is_success() {
                return res.bytes().await.map_err(|_| FetchError::Failed);
            }
            Err(FetchError::Failed)
        }
        Err(err) => {
            println!("Error fetching rune image: {:?}", err);
            Err(FetchError::Failed)
        }
    }
}

pub async fn fetch_rune_image(endpoint: &String, icon_path: &String) -> Result<Bytes, FetchError> {
    let url = format!("{endpoint}{icon_path}");
    fetch_image_data(&url).await
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuneStyle {
    pub allowed_sub_styles: Vec<i64>,
    pub asset_map: HashMap<String, String>,
    pub default_page_name: String,
    pub default_perks: Vec<i64>,
    pub default_sub_style: i64,
    pub icon_path: String,
    pub id: i64,
    pub id_name: String,
    pub name: String,
    pub slots: Vec<Slot>,
    pub sub_style_bonus: Vec<SubStyleBonu>,
    pub tooltip: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub perks: Vec<i64>,
    pub slot_label: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubStyleBonu {
    pub perk_id: i64,
    pub style_id: i64,
}

pub async fn list_all_styles(endpoint: &String) -> Result<Vec<RuneStyle>, LcuError> {
    let url = format!("{endpoint}/lol-perks/v1/styles");
    make_get_request(&url).await
}

pub async fn get_champion_icon_by_id(endpoint: &String, id: i64) -> Result<Bytes, FetchError> {
    let url = format!("{endpoint}/lol-game-data/assets/v1/champion-icons/{id}.png");
    fetch_image_data(&url).await
}

pub async fn make_ws_client(endpoint: &String) -> Result<WebSocket, reqwest_websocket::Error> {
    let url = format!("wss://{endpoint}");
    let client = Client::default();
    let response = client.get(url).upgrade().send().await?;
    let ws = response.into_websocket().await?;
    Ok(ws)
}

pub fn make_sub_msg() -> Message {
    Message::Text("[5, \"OnJsonApiEvent\"]".into())
}

pub fn make_champion_avatar_url(endpoint: &String, id: u64) -> Url {
    format!("https://{endpoint}/lol-game-data/assets/v1/champion-icons/{id}.png")
        .parse::<Url>()
        .unwrap()
}
