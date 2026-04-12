#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod settings;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use kv_log_macro::{info, warn};
use slint::{
    ComponentHandle, Image, Model, ModelRc, SharedPixelBuffer, SharedString, VecModel, Weak,
};

use lcu::{
    builds::Rune,
    cmd::get_cmd_output,
    lcu_api::{self, make_sub_msg},
    reqwest_websocket::Message,
    serde_json::{from_str, Value},
    web::{self, ChampionsMap},
};

use settings::Settings;

slint::include_modules!();

// ---------------------------------------------------------------------------
//  Shared state accessible from both the UI thread and tokio tasks
// ---------------------------------------------------------------------------

/// Auth URL for the running League Client (e.g. "riot:token@127.0.0.1:port").
/// Empty string means no client detected.
struct AppState {
    auth_url: String,
    is_tencent: bool,
    lol_dir: String,
    champions_map: ChampionsMap,
    /// Runes for the currently displayed champion + source, kept so we can index into them.
    current_runes: Vec<Rune>,
    /// All selected source values (kept in sync with the UI model).
    selected_sources: Vec<String>,
    /// The source value currently chosen in the runes window combo box.
    rune_source: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            auth_url: String::new(),
            is_tencent: false,
            lol_dir: String::new(),
            champions_map: ChampionsMap::new(),
            current_runes: Vec::new(),
            selected_sources: Vec::new(),
            rune_source: String::new(),
        }
    }
}

type SharedState = Arc<Mutex<AppState>>;

// ---------------------------------------------------------------------------
//  main
// ---------------------------------------------------------------------------

fn main() {
    femme::with_level(femme::LevelFilter::Info);

    let saved = Settings::load();

    // -- Create windows --
    let sources_window = SourcesWindow::new().unwrap();
    let runes_window = RunesWindow::new().unwrap();

    let state: SharedState = Arc::new(Mutex::new(AppState::default()));

    // Pre-populate selected sources from settings
    {
        let mut s = state.lock().unwrap();
        s.selected_sources = saved.selected_sources.clone();
        s.rune_source = saved.rune_source.clone();
    }

    // -- Source toggle callback --
    let state_c = state.clone();
    let sources_weak = sources_window.as_weak();
    sources_window.on_source_toggled(move |index, checked| {
        if let Some(win) = sources_weak.upgrade() {
            let sources = win.get_sources();
            if let Some(mut item) = sources.row_data(index as usize) {
                item.selected = checked;
                sources.set_row_data(index as usize, item.clone());

                // Update shared state + persist
                let mut s = state_c.lock().unwrap();
                if checked {
                    if !s.selected_sources.contains(&item.value.to_string()) {
                        s.selected_sources.push(item.value.to_string());
                    }
                } else {
                    s.selected_sources.retain(|v| v != item.value.as_str());
                }
                let settings = Settings {
                    selected_sources: s.selected_sources.clone(),
                    rune_source: s.rune_source.clone(),
                };
                drop(s);
                settings.save();
            }
        }
    });

    // -- Apply Builds button --
    let state_c = state.clone();
    let sources_weak = sources_window.as_weak();
    let rt_handle = tokio::runtime::Runtime::new().unwrap();
    // We need the runtime handle to spawn from callbacks
    let rt_handle_ref = rt_handle.handle().clone();

    sources_window.on_apply_builds_clicked({
        let state_c = state_c.clone();
        let weak = sources_weak.clone();
        let handle = rt_handle_ref.clone();
        move || {
            let s = state_c.lock().unwrap();
            let selected: Vec<String> = s.selected_sources.clone();
            let champions = s.champions_map.clone();
            let dir = s.lol_dir.clone();
            let is_tencent = s.is_tencent;
            drop(s);

            if selected.is_empty() || dir.is_empty() {
                let w = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() {
                        win.set_apply_status(SharedString::from(if selected.is_empty() {
                            "Select at least one source"
                        } else {
                            "League Client directory not found"
                        }));
                    }
                });
                return;
            }

            // Set applying state
            let w = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() {
                    win.set_applying_builds(true);
                    win.set_apply_status(SharedString::from("Applying builds…"));
                }
            });

            let weak2 = weak.clone();
            handle.spawn(async move {
                let logs = Arc::new(Mutex::new(Vec::new()));
                let result =
                    lcu::builds::batch_apply(selected, champions, dir, is_tencent, logs.clone())
                        .await;

                let count = logs.lock().unwrap().len();
                let msg = match result {
                    Ok(()) => format!("Done! Applied builds for {} champions", count),
                    Err(()) => "Error applying builds".to_string(),
                };

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = weak2.upgrade() {
                        win.set_applying_builds(false);
                        win.set_apply_status(SharedString::from(&msg));
                    }
                });
            });
        }
    });

    // -- Runes window: close --
    let runes_weak = runes_window.as_weak();
    runes_window.on_close_requested(move || {
        if let Some(win) = runes_weak.upgrade() {
            win.hide().unwrap();
        }
    });

    // -- Runes window: source changed --
    let runes_weak = runes_window.as_weak();
    let state_c = state.clone();
    let handle_c = rt_handle_ref.clone();
    runes_window.on_source_changed({
        move |source_idx| {
            let s = state_c.lock().unwrap();
            let sources: Vec<String> = s.selected_sources.clone();
            drop(s);

            if (source_idx as usize) >= sources.len() {
                return;
            }
            let source = sources[source_idx as usize].clone();

            // Save preference
            {
                let mut s = state_c.lock().unwrap();
                s.rune_source = source.clone();
                let settings = Settings {
                    selected_sources: s.selected_sources.clone(),
                    rune_source: s.rune_source.clone(),
                };
                drop(s);
                settings.save();
            }

            // Re-fetch runes for the current champion
            let weak = runes_weak.clone();
            let state2 = state_c.clone();
            handle_c.spawn(async move {
                let champ_id = {
                    let w_ref = weak.clone();
                    // Read champion_id from UI — need invoke_from_event_loop
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w_ref.upgrade() {
                            let _ = tx.send(win.get_champion_id() as i64);
                        }
                    });
                    rx.await.unwrap_or(0)
                };
                if champ_id > 0 {
                    fetch_and_show_runes(weak, state2, source, champ_id).await;
                }
            });
        }
    });

    // -- Runes window: apply rune --
    let runes_weak = runes_window.as_weak();
    let state_c = state.clone();
    let handle_c = rt_handle_ref.clone();
    runes_window.on_apply_rune_clicked({
        move |rune_idx| {
            let s = state_c.lock().unwrap();
            let auth = s.auth_url.clone();
            let rune = s.current_runes.get(rune_idx as usize).cloned();
            drop(s);

            if auth.is_empty() {
                return;
            }
            let Some(rune) = rune else { return };

            let weak = runes_weak.clone();
            let endpoint = format!("https://{auth}");
            handle_c.spawn(async move {
                let _ = slint::invoke_from_event_loop({
                    let weak = weak.clone();
                    move || {
                        if let Some(win) = weak.upgrade() {
                            win.set_apply_rune_status(SharedString::from("Applying rune…"));
                        }
                    }
                });

                let msg = match lcu_api::apply_rune(endpoint, rune).await {
                    Ok(()) => "Rune applied!".to_string(),
                    Err(e) => format!("Failed: {:?}", e),
                };

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = weak.upgrade() {
                        win.set_apply_rune_status(SharedString::from(&msg));
                    }
                });
            });
        }
    });

    // -- Spawn background tasks --
    let sources_weak2 = sources_window.as_weak();
    let state_c2 = state.clone();
    rt_handle.spawn(fetch_sources_task(sources_weak2, state_c2, saved));

    let runes_weak2 = runes_window.as_weak();
    let sources_weak3 = sources_window.as_weak();
    let state_c3 = state.clone();
    rt_handle.spawn(lcu_monitor_task(sources_weak3, runes_weak2, state_c3));

    // -- Show sources window and run event loop --
    sources_window.show().unwrap();
    slint::run_event_loop().unwrap();
}

// ---------------------------------------------------------------------------
//  Task: fetch sources + champions + runes metadata at startup
// ---------------------------------------------------------------------------

async fn fetch_sources_task(
    sources_weak: Weak<SourcesWindow>,
    state: SharedState,
    saved: Settings,
) {
    match web::init_for_ui().await {
        Ok((sources, champions_map, _runes_meta)) => {
            // Store champions map in shared state
            {
                let mut s = state.lock().unwrap();
                s.champions_map = champions_map;
            }

            let selected_set: std::collections::HashSet<&str> =
                saved.selected_sources.iter().map(|s| s.as_str()).collect();

            let items: Vec<SourceItemModel> = sources
                .iter()
                .map(|s| SourceItemModel {
                    label: SharedString::from(&s.label),
                    value: SharedString::from(&s.value),
                    selected: selected_set.contains(s.value.as_str()),
                })
                .collect();

            slint::invoke_from_event_loop(move || {
                if let Some(win) = sources_weak.upgrade() {
                    let model = ModelRc::new(VecModel::from(items));
                    win.set_sources(model);
                    win.set_status(SharedString::from("success"));
                }
            })
            .unwrap();
        }
        Err(_) => {
            slint::invoke_from_event_loop(move || {
                if let Some(win) = sources_weak.upgrade() {
                    win.set_status(SharedString::from("error"));
                }
            })
            .unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
//  Task: LCU process polling + WebSocket champion-select monitoring
// ---------------------------------------------------------------------------

async fn lcu_monitor_task(
    sources_weak: Weak<SourcesWindow>,
    runes_weak: Weak<RunesWindow>,
    state: SharedState,
) {
    let mut current_auth_url = String::new();
    let mut current_champion_id: i64 = 0;

    loop {
        // Poll for the League Client process
        let cmd_output = match get_cmd_output() {
            Ok(ret) if !ret.auth_url.is_empty() => ret,
            _ => {
                // No League Client found
                if !current_auth_url.is_empty() {
                    current_auth_url.clear();
                    current_champion_id = 0;

                    {
                        let mut s = state.lock().unwrap();
                        s.auth_url.clear();
                        s.lol_dir.clear();
                        s.is_tencent = false;
                    }

                    let sw = sources_weak.clone();
                    let rw = runes_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = sw.upgrade() {
                            win.set_lcu_status(SharedString::from("disconnected"));
                            win.set_lcu_summoner(SharedString::from(""));
                        }
                        if let Some(win) = rw.upgrade() {
                            win.set_has_champion(false);
                            win.set_champion_id(0);
                            win.hide().unwrap();
                        }
                    });
                }
                tokio::time::sleep(Duration::from_millis(2500)).await;
                continue;
            }
        };

        let auth_url = cmd_output.auth_url.clone();

        // If auth URL changed, update state and fetch summoner
        if auth_url != current_auth_url {
            current_auth_url = auth_url.clone();
            current_champion_id = 0;
            info!("LCU auth URL changed: {}", &current_auth_url);

            {
                let mut s = state.lock().unwrap();
                s.auth_url = auth_url.clone();
                s.lol_dir = cmd_output.dir.clone();
                s.is_tencent = cmd_output.is_tencent;
            }

            // Try to get summoner name
            let endpoint = format!("https://{auth_url}");
            let summoner_name = match lcu_api::get_current_summoner(&endpoint).await {
                Ok(summoner) => {
                    if !summoner.game_name.is_empty() {
                        format!("{}#{}", summoner.game_name, summoner.tag_line)
                    } else {
                        summoner.display_name
                    }
                }
                Err(_) => "Connected".to_string(),
            };

            let sw = sources_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = sw.upgrade() {
                    win.set_lcu_status(SharedString::from("connected"));
                    win.set_lcu_summoner(SharedString::from(&summoner_name));
                }
            });
        }

        // Connect via WebSocket and listen for champion select events
        match make_ws_client_tls(&current_auth_url).await {
            Ok(ws) => {
                let (mut tx, mut rx) = ws.split();

                if let Err(e) = tx.send(make_sub_msg()).await {
                    warn!("error sending WS subscribe message: {}", e);
                    tokio::time::sleep(Duration::from_millis(2500)).await;
                    continue;
                }

                while let Some(msg) = rx.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if text.is_empty() {
                                continue;
                            }
                            let parsed: Value = match from_str(&text) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };

                            let data = parsed.get(2).and_then(|v| v.as_object());
                            let uri = data.and_then(|v| v.get("uri")).and_then(|v| v.as_str());

                            // Champion select session changes
                            if uri == Some("/lol-champ-select/v1/session") {
                                let event_type = data
                                    .and_then(|v| v.get("eventType"))
                                    .and_then(|v| v.as_str());

                                if event_type == Some("Delete") {
                                    // Session ended
                                    if current_champion_id != 0 {
                                        current_champion_id = 0;
                                        let rw = runes_weak.clone();
                                        let _ = slint::invoke_from_event_loop(move || {
                                            if let Some(win) = rw.upgrade() {
                                                win.set_has_champion(false);
                                                win.set_champion_id(0);
                                                win.hide().unwrap();
                                            }
                                        });
                                    }
                                    continue;
                                }

                                // Extract champion ID from session data
                                let session_data = data.and_then(|v| v.get("data"));
                                let cid = extract_champion_id_from_session(session_data);

                                if cid != current_champion_id && cid > 0 {
                                    current_champion_id = cid;
                                    info!("champion id changed: {}", cid);

                                    // Update runes window
                                    let rw = runes_weak.clone();
                                    let auth = current_auth_url.clone();
                                    let st = state.clone();

                                    show_champion_runes(rw, st, auth, cid).await;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warn!("WS receive error: {}", e);
                            break;
                        }
                    }
                }

                info!("WebSocket disconnected, will retry");
                current_auth_url.clear();
            }
            Err(e) => {
                warn!("error creating WebSocket client: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_millis(2500)).await;
    }
}

// ---------------------------------------------------------------------------
//  Extract champion ID from a champ-select session JSON
// ---------------------------------------------------------------------------

fn extract_champion_id_from_session(session: Option<&Value>) -> i64 {
    let session = match session {
        Some(v) => v,
        None => return 0,
    };

    let cell_id = match session.get("localPlayerCellId").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return 0,
    };

    // Check myTeam first
    if let Some(team) = session.get("myTeam").and_then(|v| v.as_array()) {
        for member in team {
            if member.get("cellId").and_then(|v| v.as_i64()) == Some(cell_id) {
                if let Some(cid) = member.get("championId").and_then(|v| v.as_i64()) {
                    if cid > 0 {
                        return cid;
                    }
                }
            }
        }
    }

    // Check actions
    if let Some(actions) = session.get("actions").and_then(|v| v.as_array()) {
        for row in actions {
            if let Some(arr) = row.as_array() {
                for action in arr {
                    let actor = action.get("actorCellId").and_then(|v| v.as_i64());
                    let action_type = action.get("type").and_then(|v| v.as_str());
                    if actor == Some(cell_id) && action_type != Some("ban") {
                        if let Some(cid) = action.get("championId").and_then(|v| v.as_i64()) {
                            if cid > 0 {
                                return cid;
                            }
                        }
                    }
                }
            }
        }
    }

    0
}

// ---------------------------------------------------------------------------
//  Show champion runes: fetch avatar, populate source list, fetch runes
// ---------------------------------------------------------------------------

async fn show_champion_runes(
    runes_weak: Weak<RunesWindow>,
    state: SharedState,
    auth_url: String,
    champion_id: i64,
) {
    // Fetch champion avatar pixels (off UI thread)
    let avatar_pixels = fetch_champion_avatar_pixels(&auth_url, champion_id as u64).await;

    // Determine champion name from champions_map
    let champion_name = {
        let s = state.lock().unwrap();
        s.champions_map
            .values()
            .find(|c| c.key == champion_id.to_string())
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };

    // Determine which sources are selected and which is the rune source
    let (source_names, rune_source_idx, rune_source_value) = {
        let s = state.lock().unwrap();
        let sources = s.selected_sources.clone();
        let rune_src = s.rune_source.clone();
        let idx = sources.iter().position(|v| v == &rune_src).unwrap_or(0);
        let value = sources.get(idx).cloned().unwrap_or_default();
        (sources, idx, value)
    };

    // Update the runes window with champion info + source list
    let weak = runes_weak.clone();
    let names_for_ui: Vec<SharedString> = source_names
        .iter()
        .map(|s| SharedString::from(s.as_str()))
        .collect();
    let champ_name = SharedString::from(&champion_name);
    let src_idx = rune_source_idx as i32;

    let _ = slint::invoke_from_event_loop(move || {
        if let Some(win) = weak.upgrade() {
            win.set_champion_id(champion_id as i32);
            win.set_champion_name(champ_name);
            win.set_has_champion(true);

            if let Some(px) = avatar_pixels {
                let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &px.rgba_data,
                    px.width,
                    px.height,
                );
                win.set_champion_avatar(Image::from_rgba8(buffer));
            }

            let model = ModelRc::new(VecModel::from(names_for_ui));
            win.set_source_names(model);
            win.set_current_source_index(src_idx);

            win.show().unwrap();
        }
    });

    // Fetch runes for the chosen source
    if !rune_source_value.is_empty() {
        fetch_and_show_runes(runes_weak, state, rune_source_value, champion_id).await;
    }
}

// ---------------------------------------------------------------------------
//  Fetch runes for a champion from a source and display them
// ---------------------------------------------------------------------------

async fn fetch_and_show_runes(
    runes_weak: Weak<RunesWindow>,
    state: SharedState,
    source: String,
    champion_id: i64,
) {
    // Set loading state
    let weak = runes_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(win) = weak.upgrade() {
            win.set_rune_status(SharedString::from("loading"));
            win.set_apply_rune_status(SharedString::from(""));
        }
    });

    match web::list_builds_by_id(&source, champion_id).await {
        Ok(sections) => {
            let runes: Vec<Rune> = sections.iter().flat_map(|s| s.runes.clone()).collect();

            let rune_models: Vec<RuneModel> = runes
                .iter()
                .enumerate()
                .map(|(i, r)| RuneModel {
                    index: i as i32,
                    name: SharedString::from(&r.name),
                    position: SharedString::from(&r.position),
                    pick_count: r.pick_count as i32,
                    win_rate: SharedString::from(&r.win_rate),
                    primary_style_id: r.primary_style_id as i32,
                    sub_style_id: r.sub_style_id as i32,
                })
                .collect();

            // Store runes in shared state so we can apply them
            {
                let mut s = state.lock().unwrap();
                s.current_runes = runes;
            }

            let weak = runes_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = weak.upgrade() {
                    let model = ModelRc::new(VecModel::from(rune_models));
                    win.set_runes(model);
                    win.set_rune_status(SharedString::from("success"));
                }
            });
        }
        Err(_) => {
            let weak = runes_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = weak.upgrade() {
                    win.set_rune_status(SharedString::from("error"));
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------
//  WebSocket client that accepts the LCU's self-signed certificate
// ---------------------------------------------------------------------------

async fn make_ws_client_tls(
    endpoint: &str,
) -> Result<lcu::reqwest_websocket::WebSocket, lcu::reqwest_websocket::Error> {
    use lcu::reqwest_websocket::RequestBuilderExt;

    let url = format!("wss://{endpoint}");
    let client = lcu::reqwest::Client::builder()
        .use_rustls_tls()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .unwrap();
    let response = client.get(url).upgrade().send().await?;
    let ws = response.into_websocket().await?;
    Ok(ws)
}

// ---------------------------------------------------------------------------
//  Champion avatar pixel fetching (decode PNG → RGBA on tokio thread)
// ---------------------------------------------------------------------------

struct AvatarPixels {
    width: u32,
    height: u32,
    rgba_data: Vec<u8>,
}

async fn fetch_champion_avatar_pixels(auth_url: &str, champion_id: u64) -> Option<AvatarPixels> {
    let url = format!(
        "https://{}/lol-game-data/assets/v1/champion-icons/{}.png",
        auth_url, champion_id
    );

    let client = lcu_api::make_client();
    let resp = client.get(&url).send().await.ok()?;
    let bytes = resp.bytes().await.ok()?;

    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    Some(AvatarPixels {
        width,
        height,
        rgba_data: rgba.into_raw(),
    })
}
