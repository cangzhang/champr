use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::str;
use std::sync::{Arc, Mutex};

#[allow(unused_imports)]
use kv_log_macro::{error, info};

const APP_PORT_KEY: &str = "--app-port=";
const TOKEN_KEY: &str = "--remoting-auth-token=";
const REGION_KEY: &str = "--region=";
const DIR_KEY: &str = "--install-directory=";
#[allow(dead_code)]
const LCU_COMMAND: &str = "Get-CimInstance Win32_Process -Filter \"name = 'LeagueClientUx.exe'\" | Select-Object -ExpandProperty CommandLine";

lazy_static! {
    static ref PORT_REGEXP: regex::Regex = regex::Regex::new(r"--app-port=\d+").unwrap();
    static ref TOKEN_REGEXP: regex::Regex =
        regex::Regex::new(r"--remoting-auth-token=\S+").unwrap();
    static ref REGION_REGEXP: regex::Regex = regex::Regex::new(r"--region=\S+").unwrap();
    static ref DIR_REGEXP: regex::Regex =
        regex::Regex::new(r#"--install-directory=(.*?)""#).unwrap();
    static ref MAC_DIR_REGEXP: regex::Regex =
        regex::Regex::new(r"--install-directory=([^\s]+).*?--").unwrap();
}

pub fn make_auth_url(token: &String, port: &String) -> String {
    format!("riot:{token}@127.0.0.1:{port}")
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandLineOutput {
    pub auth_url: String,
    pub is_tencent: bool,
    pub token: String,
    pub port: String,
    pub dir: String,
}

#[cfg(target_os = "windows")]
pub fn get_cmd_output() -> Result<CommandLineOutput, ()> {
    use powershell_script::PsScriptBuilder;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    match ps.run(LCU_COMMAND) {
        Ok(out) => {
            #[cfg(not(debug_assertions))]
            info!("output: {:?}", out.stdout());

            if let Some(output) = out.stdout() {
                return Ok(match_stdout(output.as_str()));
            }
        }
        Err(err) => error!("cmd error: {:?}", err),
    }

    Err(())
}

#[cfg(not(target_os = "windows"))]
pub fn get_cmd_output() -> Result<CommandLineOutput, ()> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let cmd_str = r#"ps -A | grep LeagueClientUx | grep remoting-auth-token="#;
    let mut cmd = Command::new("sh")
        .args(["-c", cmd_str])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut auth_url = String::new();
    let mut token = String::new();
    let mut port = String::new();
    let mut dir = String::new();
    let mut is_tencent = false;
    {
        let stdout = cmd.stdout.as_mut().unwrap();
        let stdout_reader = BufReader::new(stdout);
        let stdout_lines = stdout_reader.lines();

        for line in stdout_lines {
            match line {
                Ok(s) => {
                    if s.contains("--app-port=") {
                        CommandLineOutput {
                            auth_url,
                            is_tencent,
                            token,
                            port,
                            ..
                        } = match_stdout(&s);
                        dir = if let Some(dir_match) = MAC_DIR_REGEXP.find(&s) {
                            dir_match.as_str().replace(DIR_KEY, "").replace(" --", "")
                        } else {
                            "".to_string()
                        };
                        break;
                    }
                }
                Err(e) => {
                    info!("[cmd::get_cmd_output] {:?}", e);
                    return Err(());
                }
            }
        }
    }
    cmd.wait().unwrap();

    Ok(CommandLineOutput {
        auth_url,
        is_tencent,
        token,
        port,
        dir,
    })
}

pub fn match_stdout(stdout: &str) -> CommandLineOutput {
    let port = if let Some(port_match) = PORT_REGEXP.find(stdout) {
        port_match.as_str().replace(APP_PORT_KEY, "")
    } else {
        "0".to_string()
    };
    let token = if let Some(token_match) = TOKEN_REGEXP.find(stdout) {
        token_match
            .as_str()
            .replace(TOKEN_KEY, "")
            .replace(['\\', '\"'], "")
    } else {
        "".to_string()
    };

    let auth_url = make_auth_url(&token, &port);

    let is_tencent = if let Some(region_match) = REGION_REGEXP.find(stdout) {
        let region = region_match
            .as_str()
            .replace(REGION_KEY, "")
            .replace(['\\', '\"'], "");
        region.eq("TENCENT")
    } else {
        false
    };

    let raw_dir = if let Some(dir_match) = DIR_REGEXP.find(stdout) {
        dir_match.as_str().replace(DIR_KEY, "")
    } else {
        "".to_string()
    };
    let output_dir = raw_dir.replace('\"', "");
    let dir = if is_tencent {
        format!("{output_dir}/..")
    } else {
        format!("{output_dir}/")
    };

    CommandLineOutput {
        auth_url,
        is_tencent,
        token,
        port,
        dir,
    }
}

#[cfg(target_os = "windows")]
pub async fn spawn_apply_rune(token: &String, port: &String, perk: &String) -> anyhow::Result<()> {
    use base64::{engine::general_purpose, Engine as _};
    use std::{os::windows::process::CommandExt, process::Command};

    let perk = general_purpose::STANDARD_NO_PAD.encode(perk);
    Command::new("./LeagueClient.exe")
        .args(["rune", token, port, &perk])
        .creation_flags(0x08000000)
        .spawn()
        .expect("[spawn_apply_rune] failed");

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub async fn spawn_apply_rune(_token: &String, _port: &String, _perk: &String) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub async fn check_if_server_ready() -> anyhow::Result<bool> {
    Ok(true)
}

#[cfg(target_os = "windows")]
pub async fn check_if_server_ready() -> anyhow::Result<bool> {
    use std::io::{BufRead, BufReader, Error, ErrorKind};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let CommandLineOutput {
        dir, is_tencent, ..
    } = get_cmd_output().map_err(|_| Error::new(ErrorKind::Other, "Could not read League client command line."))?;

    if dir.is_empty() {
        info!("[cmd::check_if_tencent_server_ready] cannot get lcu install dir");
        return Ok(false);
    }

    let is_tencent_arg = if is_tencent { "1" } else { "0" };
    let stdout = Command::new("./LeagueClient.exe")
        .args(["check", &dir, is_tencent_arg])
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .spawn()?
        .stdout
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let mut ready = true;
    let reader = BufReader::new(stdout);
    reader.lines().map_while(Result::ok).for_each(|line| {
        if line.starts_with("=== tencent_sucks") {
            ready = false;
        }
    });

    Ok(ready)
}

#[cfg(target_os = "windows")]
pub async fn test_connectivity() -> anyhow::Result<bool> {
    use std::io::{BufRead, BufReader, Error, ErrorKind};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let CommandLineOutput { port, token, .. } =
        get_cmd_output().map_err(|_| Error::new(ErrorKind::Other, "Could not read League client command line."))?;

    let stdout = Command::new("./LeagueClient.exe")
        .args(["test", &token, &port])
        .creation_flags(0x08000000)
        .stdout(Stdio::piped())
        .spawn()?
        .stdout
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let mut connected = false;
    let reader = BufReader::new(stdout);
    reader.lines().map_while(Result::ok).for_each(|line| {
        if line.starts_with("=== connected") {
            connected = true;
        }
    });

    Ok(connected)
}

#[cfg(not(target_os = "windows"))]
pub async fn test_connectivity() -> anyhow::Result<bool> {
    Ok(true)
}

pub fn check_if_lol_running() -> bool {
    if let Ok(auth) = get_cmd_output() {
        return !auth.auth_url.is_empty();
    }

    false
}

pub fn start_check_cmd_task() {}

pub fn update_cmd_output_task(output: &Arc<Mutex<CommandLineOutput>>) {
    if let Ok(result) = get_cmd_output() {
        *output.lock().unwrap() = result;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_cmd() {
        let ret = get_cmd_output();
        println!("{:?}", ret);
    }
}
