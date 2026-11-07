use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::key::{self, Config, Probe, Source};
use crate::protocol::HostMessage;

const CARD: &str = "pocket";
const LICENSES_DIR: &str = "pocket/Licenses";
const PLUGIN_DIR: &str = "pocket/Plugins/jotego-beta-key";
const CONFIG_FILE: &str = "config.json";
const LEGACY_CACHED_ZIP: &str = "jtbeta.zip";
const DEFAULT_GITHUB_API: &str = "https://api.github.com/";
const DEFAULT_PATREON: &str = "https://www.patreon.com/";
const GITHUB_API_CONFIG: &str = "github_api_url";
const PATREON_CONFIG: &str = "patreon_url";
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const STATE_KEY: &str = "state";

const Q_SOURCE: &str = "source";
const Q_TOKEN: &str = "github_token";
const Q_COOKIE: &str = "patreon_cookie";
const Q_RETRY_GITHUB: &str = "retry_github";
const Q_RETRY_PATREON: &str = "retry_patreon";
const RETRY: &str = "Enter it again";
const SKIP: &str = "Skip";

#[derive(Clone, Serialize, ToBytes)]
#[encoding(Json)]
enum PluginMessage {
    Choice { name: String, query: String, choices: Vec<String> },
    Text { name: String, query: String },
    Exit,
}

#[host_fn]
extern "ExtismHost" {
    fn open_url(url: &str) -> ();
    fn print_msg(message: &str) -> ();
}

fn println(msg: &str) {
    unsafe {
        let _ = print_msg(&format!("{msg}\n"));
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    // Working settings, including credentials entered this run. Written to config.json only by
    // finish(), i.e. once a download has worked.
    config: Config,
    // config.json as it was loaded: what a failed credential is reverted to, so a token that
    // worked before is never thrown away because of one bad response
    saved: Config,
    // config.json exists but could not be read or parsed; never overwrite it
    config_unreadable: bool,
    // GitHub was already attempted (or skipped) this run. don't try it again after falling back.
    github_tried: bool,
}

fn load_state() -> State {
    var::get::<Json<State>>(STATE_KEY).ok().flatten().map(|j| j.0).unwrap_or_default()
}

fn save_state(state: &State) {
    let _ = var::set(STATE_KEY, Json(state.clone()));
}

fn setting(key: &str, default: &str) -> String {
    config::get(key).ok().flatten().filter(|s| !s.is_empty()).unwrap_or_else(|| default.into())
}

fn config_path() -> std::path::PathBuf {
    Path::new(PLUGIN_DIR).join(CONFIG_FILE)
}

//returns the config and whether config.json is unreadable (and so must not be overwritten).
fn load_config() -> (Config, bool) {
    let path = format!("Plugins/jotego-beta-key/{CONFIG_FILE}");
    match fs::read_to_string(config_path()) {
        Ok(text) => match Config::parse(&text) {
            Ok(cfg) => (cfg, false),
            Err(e) => {
                println(&format!("{path} could not be parsed ({e}). It won't be changed. fix or delete it to save settings again."));
                (Config::default(), true)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), false),
        Err(e) => {
            println(&format!("Could not read {path} ({e}). It will not be changed."));
            (Config::default(), true)
        }
    }
}

fn save_config(cfg: &Config) {
    if let Err(e) = fs::create_dir_all(PLUGIN_DIR) {
        println(&format!("Could not create Plugins/jotego-beta-key: {e}"));
    } else {
        if let Err(e) = fs::write(config_path(), cfg.to_json()) {
            println(&format!("Could not save Plugins/jotego-beta-key/{CONFIG_FILE}: {e}"));
        }
    }
}

// `name` may contain '/'-separated sub folders. returns false when the file already held `bytes`.
fn write_file(dir: &str, name: &str, bytes: &[u8]) -> Result<bool, String> {
    let dest = Path::new(dir).join(name);
    if fs::read(&dest).map(|existing| existing == bytes).unwrap_or(false) {
        return Ok(false);
    }
    let parent = dest.parent().unwrap_or(Path::new(dir));
    fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let result = fs::write(&part, bytes)
        .map_err(|e| format!("could not write {}: {e}", part.display()))
        .and_then(|()| fs::rename(&part, &dest).map_err(|e| format!("could not replace {}: {e}", dest.display())));
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result.map(|()| true)
}

// Unpack the key files into Licenses/; returns how many changed.
fn install_zip(zip: &[u8]) -> Result<usize, String> {
    let mut changed = 0;
    for (name, data) in key::extract_zip(zip)? {
        if write_file(LICENSES_DIR, &name, &data)? {
            println(&format!("Extracted Licenses/{name} ({} bytes).", data.len()));
            changed += 1;
        }
    }
    Ok(changed)
}

// A jtbeta.zip or beta.bin in the root of the card wins over any download, same behavior from pupdate
// (CoresService.RetrieveKeys only auto fetches when ExtractJTBetaKey found nothing).
fn try_manual_key() -> Option<Result<(), String>> {
    let zip = Path::new(CARD).join(key::KEY_ZIP);
    if let Ok(bytes) = fs::read(&zip) {
        println(&format!("{} found in the root of the SD card, using it (nothing downloaded).", key::KEY_ZIP));
        return Some(install_zip(&bytes).map(|_| ()));
    }
    let bin = Path::new(CARD).join(key::KEY_BIN);
    if let Ok(bytes) = fs::read(&bin) {
        println(&format!("{} found in the root of the SD card, using it (nothing downloaded).", key::KEY_BIN));
        return Some(write_file(LICENSES_DIR, key::KEY_BIN, &bytes).map(|_| ()));
    }
    None
}

enum FetchError {
    Auth(String),
    Other(String),
}

fn send(req: &HttpRequest) -> Result<HttpResponse, String> {
    http::request::<()>(req, None).map_err(|e| format!("request failed: {e}"))
}

fn get(req: HttpRequest) -> Result<(u16, Vec<u8>), String> {
    let res = send(&req)?;
    Ok((res.status_code(), res.body()))
}

fn header_is(res: &HttpResponse, name: &str, value: &str) -> bool {
    res.headers().iter().any(|(k, v)| k.eq_ignore_ascii_case(name) && v.trim() == value)
}

fn fetch_github(token: &str) -> Result<Vec<u8>, FetchError> {
    let url = key::github_contents_url(&setting(GITHUB_API_CONFIG, DEFAULT_GITHUB_API));
    let req = HttpRequest::new(&url)
        .with_header("User-Agent", "pocket-plugin-jotego-beta-key/0.1")
        .with_header("Accept", "application/vnd.github.raw")
        .with_header("Authorization", format!("token {}", token.trim()));
    let res = send(&req).map_err(FetchError::Other)?;
    let repo = format!("{}/{}", key::GITHUB_OWNER, key::GITHUB_REPO);
    match res.status_code() {
        200 => {
            let body = res.body();
            if body.is_empty() {
                Err(FetchError::Other("GitHub returned an empty file".into()))
            } else {
                Ok(body)
            }
        }
        // A rate limit says nothing about the token, so it must not be treated as a bad one.
        403 | 429 if header_is(&res, "x-ratelimit-remaining", "0") => {
            Err(FetchError::Other("GitHub's API rate limit was reached, try again later".into()))
        }
        429 => Err(FetchError::Other("GitHub is rate limiting requests, try again later".into())),
        401 => Err(FetchError::Auth("GitHub rejected the token: it may be wrong, expired or revoked".into())),
        403 => Err(FetchError::Auth(format!(
            "GitHub refused access to {repo}: the token may lack the needed scope or SSO authorisation"
        ))),
        // GitHub reports a private repo the token can't see as 404, so this is usually missing
        // sponsor access; it can also mean the file moved.
        404 => Err(FetchError::Auth(format!(
            "{} was not found in {repo}: the token's account may not have sponsor access (or the file has moved)",
            key::KEY_ZIP
        ))),
        status => Err(FetchError::Other(format!("GitHub returned HTTP {status}"))),
    }
}

fn patreon_req(url: &str, cookie: &str) -> HttpRequest {
    HttpRequest::new(url)
        .with_header("User-Agent", BROWSER_UA)
        .with_header("Accept", "application/vnd.api+json, application/json, text/html")
        .with_header("Cookie", format!("session_id={}", cookie.trim()))
}

fn fetch_patreon(cookie: &str) -> Result<(Vec<u8>, String), FetchError> {
    let base = setting(PATREON_CONFIG, DEFAULT_PATREON);
    let (status, html) = get(patreon_req(&key::patreon_page_url(&base), cookie)).map_err(FetchError::Other)?;
    if status != 200 {
        return Err(FetchError::Other(format!("failed to load the Patreon page for '{}' (HTTP {status})", key::PATREON_VANITY)));
    }
    let campaign = key::campaign_id(&String::from_utf8_lossy(&html))
        .ok_or_else(|| FetchError::Other("could not find the campaign id on the Patreon page. Patreon may have changed its page structure".into()))?;

    let mut next = Some(key::patreon_posts_url(&base, &campaign));
    for _ in 0..key::MAX_PAGES {
        let Some(url) = next.take() else { break };
        let (status, body) = get(patreon_req(&url, cookie)).map_err(FetchError::Other)?;
        match status {
            200 => {}
            401 | 403 => return Err(FetchError::Auth("Patreon rejected the session cookie, it may be expired. copy a fresh one from your browser".into())),
            s => return Err(FetchError::Other(format!("Patreon posts request failed (HTTP {s})"))),
        }
        match key::probe_posts_page(&String::from_utf8_lossy(&body), key::KEY_ZIP).map_err(FetchError::Other)? {
            Probe::Found(att) => {
                let (status, zip) = get(patreon_req(&att.download_url, cookie)).map_err(FetchError::Other)?;
                if status != 200 || zip.is_empty() {
                    return Err(FetchError::Other(format!("attachment download failed (HTTP {status})")));
                }
                return Ok((zip, att.post_url));
            }
            // The cookie worked; re-entering it wouldn't help, so this isn't an Auth failure.
            Probe::Gated(post) => {
                return Err(FetchError::Other(format!(
                    "found a post with {} ({post}) but this account can't view it. your tier may not include beta access",
                    key::KEY_ZIP
                )))
            }
            Probe::NotFound { next: n } => next = n,
        }
    }
    Err(FetchError::Other(format!(
        "no recent '{}' Patreon post with {} was found in the last {} posts",
        key::PATREON_VANITY,
        key::KEY_ZIP,
        key::MAX_PAGES * key::PAGE_SIZE
    )))
}

fn finish(state: &mut State, zip: &[u8], from: &str) -> PluginMessage {
    match install_zip(zip) {
        Ok(0) => println(&format!("Licenses/ is already up to date (checked {from}).")),
        Ok(_) => println(&format!("Jotego beta key installed from {from}.")),
        Err(e) => {
            println(&format!("Downloaded {} from {from} but could not extract it: {e}", key::KEY_ZIP));
            return PluginMessage::Exit;
        }
    }
    let _ = fs::remove_file(Path::new(PLUGIN_DIR).join(LEGACY_CACHED_ZIP));
    if state.config_unreadable {
        println(&format!("Settings were not saved because Plugins/jotego-beta-key/{CONFIG_FILE} could not be parsed."));
    } else if state.config != state.saved {
        save_config(&state.config);
    }
    PluginMessage::Exit
}

fn print_lines(lines: &[&str]) {
    println(&lines.join("\n"));
}

fn ask_source() -> PluginMessage {
    print_lines(&[
        "",
        "The beta key can be downloaded from either place Jotego publishes it:",
        "  GitHub  - the private JTFPGA/jtbeta repo. Needs a GitHub account with sponsor access and a token.",
        "            The most reliable option.",
        "  Patreon - the newest Jotego post with jtbeta.zip. Needs your Patreon login cookie, which expires",
        "            every few weeks, and Patreon can change its site without notice.",
        "  Both    - tries GitHub first, then Patreon.",
        "Your answers are saved on the SD card in Plugins/jotego-beta-key/config.json.",
    ]);
    PluginMessage::Choice {
        name: Q_SOURCE.into(),
        query: format!("Where should {} come from?", key::KEY_ZIP),
        choices: Source::CHOICES.iter().map(|(_, label)| label.to_string()).collect(),
    }
}

fn ask_token() -> PluginMessage {
    print_lines(&[
        "",
        "Getting a GitHub token for the beta key:",
        "  1. Have an active paid Jotego subscription, either on GitHub Sponsors (https://github.com/sponsors/jotego)",
        "     or on Patreon with your Patreon account linked to GitHub so GitHub counts you as a sponsor:",
        "     https://docs.github.com/en/sponsors/sponsoring-open-source-contributors/sponsoring-an-open-source-contributor-through-patreon",
        "  2. Accept the invite to the JTFPGA/jtbeta repository once it arrives (check your GitHub notifications",
        "     and email).",
        "  3. Create a classic token at https://github.com/settings/tokens/new with the 'repo' scope ticked.",
        "  4. Copy the token (it starts with ghp_) and paste it into the prompt.",
        "An empty answer skips GitHub, or stops without changes if GitHub is the only source.",
    ]);
    PluginMessage::Text {
        name: Q_TOKEN.into(),
        query: format!("GitHub token with access to {}/{} (see the steps in the output):", key::GITHUB_OWNER, key::GITHUB_REPO),
    }
}

fn ask_cookie() -> PluginMessage {
    print_lines(&[
        "",
        "Getting your Patreon session cookie:",
        "  1. Open https://www.patreon.com in your browser and log in.",
        "  2. Open the developer tools (F12, or Cmd+Option+I on a Mac).",
        "  3. Go to the Application tab (Chrome, Edge, Brave) or the Storage tab (Firefox, Safari).",
        "  4. Expand Cookies and select https://www.patreon.com.",
        "  5. Find the row named session_id and copy its Value.",
        "  6. Paste it into the prompt.",
        "The cookie stops working if you log out of Patreon in that browser or when it expires (usually within",
        "a few weeks); run this plugin again and paste a fresh one. It is only ever sent to patreon.com.",
        "An empty answer stops without changes.",
    ]);
    PluginMessage::Text {
        name: Q_COOKIE.into(),
        query: "Patreon session_id cookie value (see the steps in the output):".into(),
    }
}

fn ask_retry(name: &str, what: &str) -> PluginMessage {
    PluginMessage::Choice {
        name: name.into(),
        query: format!("{what} didn't work. What now?"),
        choices: vec![RETRY.into(), SKIP.into()],
    }
}

// Drive the flow from whatever the state has so far.
fn advance(state: &mut State) -> PluginMessage {
    let Some(source) = state.config.source else { return ask_source() };

    if source.uses_github() && !state.github_tried {
        let token = state.config.github_token.clone().filter(|t| !t.trim().is_empty());
        let Some(token) = token else {
            if source == Source::Both && state.config.patreon_session_id.is_some() {
                println("No GitHub token is saved; trying Patreon.");
                state.github_tried = true;
                return advance(state);
            }
            return ask_token();
        };
        println(&format!("Fetching {} from GitHub ({}/{})...", key::KEY_ZIP, key::GITHUB_OWNER, key::GITHUB_REPO));
        state.github_tried = true;
        match fetch_github(&token) {
            Ok(zip) => return finish(state, &zip, "GitHub"),
            Err(FetchError::Auth(msg)) => {
                println(&format!("GitHub fetch failed: {msg}."));
                //drop a token entered this run, but keep one that was already saved.
                state.config.github_token = state.saved.github_token.clone();
                if source == Source::Github {
                    return ask_retry(Q_RETRY_GITHUB, "The GitHub token");
                }
                println("Falling back to Patreon.");
            }
            Err(FetchError::Other(msg)) => {
                println(&format!("GitHub fetch failed: {msg}."));
                if source == Source::Github {
                    return PluginMessage::Exit;
                }
                println("Falling back to Patreon.");
            }
        }
    }

    if source.uses_patreon() {
        let Some(cookie) = state.config.patreon_session_id.clone() else { return ask_cookie() };
        println(&format!("Fetching {} from Patreon...", key::KEY_ZIP));
        match fetch_patreon(&cookie) {
            Ok((zip, post)) => return finish(state, &zip, &format!("Patreon ({post})")),
            Err(FetchError::Auth(msg)) => {
                println(&format!("Patreon fetch failed: {msg}."));
                state.config.patreon_session_id = state.saved.patreon_session_id.clone();
                return ask_retry(Q_RETRY_PATREON, "The Patreon session cookie");
            }
            Err(FetchError::Other(msg)) => println(&format!("Patreon fetch failed: {msg}.")),
        }
    }

    PluginMessage::Exit
}

#[plugin_fn]
pub fn start() -> FnResult<PluginMessage> {
    println("Jotego beta key");
    if let Some(result) = try_manual_key() {
        if let Err(e) = result {
            println(&format!("Failed: {e}"));
        }
        return Ok(PluginMessage::Exit);
    }
    let (config, config_unreadable) = load_config();
    let mut state = State { saved: config.clone(), config, config_unreadable, github_tried: false };
    let message = advance(&mut state);
    save_state(&state);
    Ok(message)
}

#[plugin_fn]
pub fn handle_response(Json(input): Json<HostMessage>) -> FnResult<PluginMessage> {
    let mut state = load_state();
    let message = match input {
        HostMessage::Kill => {
            println("Cancelled; nothing changed.");
            PluginMessage::Exit
        }
        HostMessage::Answer { name, value } => match name.as_str() {
            Q_SOURCE => match Source::from_choice(&value) {
                Some(source) => {
                    state.config.source = Some(source);
                    advance(&mut state)
                }
                None => ask_source(),
            },
            Q_TOKEN if value.trim().is_empty() => {
                if state.config.source == Some(Source::Both) {
                    println("No token given; skipping GitHub.");
                    state.github_tried = true;
                    advance(&mut state)
                } else {
                    println("No token given; nothing changed.");
                    PluginMessage::Exit
                }
            }
            Q_TOKEN => {
                state.config.github_token = Some(value.trim().to_string());
                state.github_tried = false;
                advance(&mut state)
            }
            Q_COOKIE if value.trim().is_empty() => {
                println("No cookie given; nothing changed.");
                PluginMessage::Exit
            }
            Q_COOKIE => {
                state.config.patreon_session_id = Some(value.trim().to_string());
                advance(&mut state)
            }
            Q_RETRY_GITHUB if value == RETRY => {
                state.github_tried = false;
                ask_token()
            }
            Q_RETRY_PATREON if value == RETRY => ask_cookie(),
            Q_RETRY_GITHUB | Q_RETRY_PATREON => {
                println("Skipped; nothing changed.");
                PluginMessage::Exit
            }
            other => {
                println(&format!("Unexpected answer for '{other}'; nothing changed."));
                PluginMessage::Exit
            }
        },
    };
    save_state(&state);
    Ok(message)
}
