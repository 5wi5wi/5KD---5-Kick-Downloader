#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{
    self, Color32, FontFamily, FontId, Pos2, Rect, RichText, Rounding, Sense, Shape, Stroke, Vec2,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{BufReader, Read, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const APP_VERSION: &str = env!("FIVEKD_APP_VERSION");
const GITHUB_REPO: &str = env!("FIVEKD_GITHUB_REPO");
const INSTALLER_ASSET_NAME: &str = env!("FIVEKD_INSTALLER_ASSET");

const AWS_ID: &str = "196233775518";
const VIDDASTRAGE_PROXY: &str = "https://cors.viddastrage.workers.dev/corsproxy/?apiurl=";
const CORS_LOL_PROXY: &str = "https://api.cors.lol/?url=";
const PREFIXES_URL: &str =
    "https://raw.githubusercontent.com/mortyobnoxious/kickvod-extractor/refs/heads/main/prefixes.json";
const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const MAX_TERMINAL_LOGS: usize = 1500;

// Refined pure-black palette with 60-30-10 discipline
const BG_CANVAS: Color32 = Color32::BLACK; // #000000
const BG_SURFACE: Color32 = Color32::from_rgb(13, 14, 16); // #0D0E10
const BG_ELEVATED: Color32 = Color32::from_rgb(21, 23, 26); // #15171A
const BORDER_SUBTLE: Color32 = Color32::from_rgb(31, 35, 40); // #1F2328
const BORDER_HOVER: Color32 = Color32::from_rgb(46, 52, 60); // #2E343C
const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 246, 248); // #F4F6F8
const TEXT_MUTED: Color32 = Color32::from_rgb(139, 148, 158); // #8B949E
const ACCENT_GREEN: Color32 = Color32::from_rgb(0, 231, 1); // #00E701
const ACCENT_TINT: Color32 = Color32::from_rgb(0, 22, 3); // Subtle active green tint
const ACCENT_HOVER_BG: Color32 = Color32::from_rgb(6, 26, 8);
const STATUS_FAILED: Color32 = Color32::from_rgb(248, 113, 113); // #F87171
const STATUS_CANCELED: Color32 = Color32::from_rgb(251, 191, 36); // #FBBF24

// Shared geometric "5" polyline used for both the in-app brand mark and OS window/taskbar icon
const BRAND_5_PTS: [(f32, f32); 8] = [
    (0.69, 0.27),
    (0.34, 0.27),
    (0.34, 0.49),
    (0.61, 0.49),
    (0.69, 0.56),
    (0.69, 0.68),
    (0.61, 0.74),
    (0.33, 0.74),
];

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Downloads,
    Terminal,
    Completed,
    About,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Resolution {
    R1080p60,
    R1080p,
    R720p60,
    R720p,
    R480p,
    R360p,
    AudioOnly,
}

impl Resolution {
    const ALL: [Resolution; 7] = [
        Resolution::R1080p60,
        Resolution::R1080p,
        Resolution::R720p60,
        Resolution::R720p,
        Resolution::R480p,
        Resolution::R360p,
        Resolution::AudioOnly,
    ];

    fn label(self) -> &'static str {
        match self {
            Resolution::R1080p60 => "1080p60 (Source)",
            Resolution::R1080p => "1080p",
            Resolution::R720p60 => "720p60",
            Resolution::R720p => "720p",
            Resolution::R480p => "480p",
            Resolution::R360p => "360p",
            Resolution::AudioOnly => "Audio Only",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Resolution::R1080p60 => "1080p60",
            Resolution::R1080p => "1080p",
            Resolution::R720p60 => "720p60",
            Resolution::R720p => "720p",
            Resolution::R480p => "480p",
            Resolution::R360p => "360p",
            Resolution::AudioOnly => "Audio",
        }
    }

    fn from_short_label(s: &str) -> Self {
        match s {
            "1080p" => Resolution::R1080p,
            "720p60" => Resolution::R720p60,
            "720p" => Resolution::R720p,
            "480p" => Resolution::R480p,
            "360p" => Resolution::R360p,
            "Audio" => Resolution::AudioOnly,
            _ => Resolution::R1080p60,
        }
    }

    fn ytdlp_format(self) -> &'static str {
        match self {
            Resolution::R1080p60 => {
                "bestvideo[height<=1080][fps<=60]+bestaudio/best[height<=1080][fps<=60]/best"
            }
            Resolution::R1080p => "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best",
            Resolution::R720p60 => {
                "bestvideo[height<=720][fps<=60]+bestaudio/best[height<=720][fps<=60]/best"
            }
            Resolution::R720p => "bestvideo[height<=720]+bestaudio/best[height<=720]/best",
            Resolution::R480p => "bestvideo[height<=480]+bestaudio/best[height<=480]/best",
            Resolution::R360p => "bestvideo[height<=360]+bestaudio/best[height<=360]/best",
            Resolution::AudioOnly => "bestaudio/best",
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum ContainerExt {
    Mkv,
    Mp4,
    Ts,
}

impl ContainerExt {
    const ALL: [ContainerExt; 3] = [ContainerExt::Mkv, ContainerExt::Mp4, ContainerExt::Ts];

    fn as_str(self) -> &'static str {
        match self {
            ContainerExt::Mkv => "mkv",
            ContainerExt::Mp4 => "mp4",
            ContainerExt::Ts => "ts",
        }
    }

    fn dot_upper(self) -> &'static str {
        match self {
            ContainerExt::Mkv => ".MKV",
            ContainerExt::Mp4 => ".MP4",
            ContainerExt::Ts => ".TS",
        }
    }

    fn upper(self) -> &'static str {
        match self {
            ContainerExt::Mkv => "MKV",
            ContainerExt::Mp4 => "MP4",
            ContainerExt::Ts => "TS",
        }
    }

    fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "mp4" => ContainerExt::Mp4,
            "ts" => ContainerExt::Ts,
            _ => ContainerExt::Mkv,
        }
    }
}

fn default_zoom_percent() -> u32 {
    100
}

fn default_check_updates() -> bool {
    true
}

/// Persistent user configuration saved across app restarts.
#[derive(Serialize, Deserialize)]
struct AppConfig {
    #[serde(default)]
    output_path: String,
    #[serde(default = "default_zoom_percent")]
    zoom_percent: u32,
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    container_ext: String,
    #[serde(default = "default_check_updates")]
    check_updates_on_launch: bool,
    #[serde(default)]
    github_repo: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_path: String::new(),
            zoom_percent: 100,
            resolution: "1080p60".to_string(),
            container_ext: "mkv".to_string(),
            check_updates_on_launch: true,
            github_repo: String::new(),
        }
    }
}

fn config_file_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = env::var_os("APPDATA").or_else(|| env::var_os("LOCALAPPDATA")) {
            return Some(PathBuf::from(appdata).join("5KD").join("config.json"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return Some(
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("5KD")
                    .join("config.json"),
            );
        }
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg).join("5kd").join("config.json"));
        }
        if let Some(home) = env::var_os("HOME") {
            return Some(PathBuf::from(home).join(".config").join("5kd").join("config.json"));
        }
    }
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("5kd_config.json")))
}

fn load_app_config() -> AppConfig {
    let Some(path) = config_file_path() else {
        return AppConfig::default();
    };
    let Ok(bytes) = fs::read(&path) else {
        return AppConfig::default();
    };
    let mut cfg: AppConfig = serde_json::from_slice(&bytes).unwrap_or_default();
    cfg.output_path = cfg.output_path.trim().to_string();
    cfg.zoom_percent = (cfg.zoom_percent.clamp(100, 150) / 10) * 10;
    cfg
}

fn save_app_config(cfg: &AppConfig) {
    let Some(path) = config_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(cfg) {
        let _ = fs::write(path, json);
    }
}

#[derive(Clone)]
struct ExtractedVodMeta {
    m3u8_url: String,
    channel_slug: String,
    session_title: String,
    duration_formatted: String,
    duration_secs: Option<f64>,
    source_type: String,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum DownloadStatus {
    Active,
    Completed,
    Failed,
    Canceled,
}

impl DownloadStatus {
    fn label(self) -> &'static str {
        match self {
            DownloadStatus::Active => "Downloading",
            DownloadStatus::Completed => "Completed",
            DownloadStatus::Failed => "Failed",
            DownloadStatus::Canceled => "Canceled",
        }
    }

    fn color(self) -> Color32 {
        match self {
            DownloadStatus::Active | DownloadStatus::Completed => ACCENT_GREEN,
            DownloadStatus::Failed => STATUS_FAILED,
            DownloadStatus::Canceled => STATUS_CANCELED,
        }
    }
}

#[derive(Clone)]
struct DownloadItem {
    id: usize,
    title: String,
    url: String,
    output_path: String,
    saved_file_path: Option<String>,
    resolution: &'static str,
    format: &'static str,
    progress: f32,
    status_text: String,
    status: DownloadStatus,
    cancel_flag: Arc<AtomicBool>,
    child_pid: Arc<Mutex<Option<u32>>>,
}

#[derive(Clone)]
struct UpdateReleaseInfo {
    tag_name: String,
    asset_name: String,
    download_url: String,
    asset_size: u64,
    release_notes: String,
}

#[derive(Clone)]
enum UpdateState {
    Idle,
    Checking,
    UpToDate(String),
    Available(UpdateReleaseInfo),
    Downloading {
        tag_name: String,
        asset_name: String,
        downloaded: u64,
        total: u64,
    },
    Installing(String),
    Error(String),
}

enum WorkerEvent {
    Log(String),
    Extracted(usize, ExtractedVodMeta),
    Progress(usize, f32, String),
    Finished(usize, bool, String, Option<String>),
    UpdateChecked(Result<Option<UpdateReleaseInfo>, String>),
    UpdateProgress {
        tag_name: String,
        asset_name: String,
        downloaded: u64,
        total: u64,
    },
    UpdateReadyToRestart(String),
    UpdateFailed(String),
}

/// Event-driven UI wakeup handle: keeps idle CPU/GPU at 0% while throttling rapid worker output.
#[derive(Clone)]
struct UiNotifier {
    ctx: egui::Context,
    last_repaint: Arc<Mutex<Instant>>,
}

impl UiNotifier {
    fn new(ctx: egui::Context) -> Self {
        let initial = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        Self {
            ctx,
            last_repaint: Arc::new(Mutex::new(initial)),
        }
    }

    fn wake_immediate(&self) {
        if let Ok(mut guard) = self.last_repaint.lock() {
            *guard = Instant::now();
        }
        self.ctx.request_repaint();
    }

    fn wake_throttled(&self) {
        let min_interval = Duration::from_millis(50);
        let should_now = if let Ok(mut guard) = self.last_repaint.lock() {
            if guard.elapsed() >= min_interval {
                *guard = Instant::now();
                true
            } else {
                false
            }
        } else {
            true
        };
        if should_now {
            self.ctx.request_repaint();
        } else {
            self.ctx.request_repaint_after(min_interval);
        }
    }
}

/// Shared connection-pooled HTTP client initialized once on first extraction.
fn shared_http_client() -> Result<&'static reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent(CHROME_UA)
                .timeout(Duration::from_secs(12))
                .pool_max_idle_per_host(4)
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| e.clone())
}

fn encode_uri_component(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len() + 32);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0F) as usize] as char);
            }
        }
    }
    out
}

fn is_valid_kick_payload(val: &Value) -> bool {
    match val {
        Value::Array(_) => true,
        Value::Object(map) => {
            if map.contains_key("error") {
                return false;
            }
            if let Some(msg) = map.get("message").and_then(|m| m.as_str()) {
                if msg.contains("No query results") || msg.contains("Not Found") {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn curl_fetch_text(url: &str) -> Option<String> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-L",
        "--max-time",
        "12",
        "-H",
        "Accept: application/json",
        "-A",
        CHROME_UA,
        url,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn get_kick_json(
    client: &reqwest::blocking::Client,
    target_url: &str,
    tx: Option<&Sender<WorkerEvent>>,
) -> Result<Value, String> {
    let encoded = encode_uri_component(target_url);
    let candidates = [
        ("viddastrage", format!("{}{}", VIDDASTRAGE_PROXY, encoded)),
        ("cors.lol", format!("{}{}", CORS_LOL_PROXY, encoded)),
        ("direct", target_url.to_string()),
    ];

    let mut errors: Vec<String> = Vec::with_capacity(3);

    for (label, candidate) in &candidates {
        match client
            .get(candidate)
            .header("Accept", "application/json")
            .send()
        {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    match resp.text() {
                        Ok(body) => match serde_json::from_str::<Value>(&body) {
                            Ok(val) => {
                                if is_valid_kick_payload(&val) {
                                    return Ok(val);
                                } else {
                                    errors.push(format!("{}: API error payload", label));
                                }
                            }
                            Err(_) => {
                                errors.push(format!("{}: non-JSON response", label));
                            }
                        },
                        Err(e) => {
                            errors.push(format!("{}: read error ({})", label, e));
                        }
                    }
                } else {
                    errors.push(format!("{}: HTTP {}", label, status.as_u16()));
                }
            }
            Err(e) => {
                errors.push(format!("{}: {}", label, e));
            }
        }
    }

    // Fallback to native curl if reqwest hits TLS/Cloudflare edge filtering
    for (label, candidate) in &candidates {
        if let Some(body) = curl_fetch_text(candidate) {
            if let Ok(val) = serde_json::from_str::<Value>(&body) {
                if is_valid_kick_payload(&val) {
                    if let Some(sender) = tx {
                        let _ = sender.send(WorkerEvent::Log(format!(
                            "[extractor] Fetched metadata via curl ({})",
                            label
                        )));
                    }
                    return Ok(val);
                }
            }
        }
    }

    let summary = if errors.is_empty() {
        format!("No response from Kick API ({})", target_url)
    } else {
        errors.join(" | ")
    };
    Err(summary)
}

fn test_m3u8_url(client: &reqwest::blocking::Client, m3u8_url: &str) -> bool {
    if let Ok(resp) = client
        .get(m3u8_url)
        .header("Range", "bytes=0-511")
        .send()
    {
        if resp.status().is_success() {
            if let Ok(body) = resp.text() {
                if body.contains("#EXTM3U") {
                    return true;
                }
            }
        }
    }
    let proxied = format!("{}{}", VIDDASTRAGE_PROXY, encode_uri_component(m3u8_url));
    if let Ok(resp) = client.head(&proxied).send() {
        if resp.status().is_success() {
            return true;
        }
    }
    if let Some(body) = curl_fetch_text(m3u8_url) {
        if body.contains("#EXTM3U") {
            return true;
        }
    }
    false
}

/// Zero-dependency UTC timestamp parser replacing chrono.
#[derive(Clone, Copy)]
struct ParsedUtc {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    timestamp_ms: i64,
}

impl ParsedUtc {
    fn ivs_time_path(self) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// Howard Hinnant's civil-to-days algorithm (days since Unix epoch 1970-01-01).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Parses Kick UTC strings like "2026-11-13 04:48:06" or "2026-11-13T04:48:07.000000Z".
fn parse_kick_utc(s: &str) -> Option<ParsedUtc> {
    let b = s.trim().as_bytes();
    if b.len() < 19 {
        return None;
    }
    if b[4] != b'-' || b[7] != b'-' || (b[10] != b' ' && b[10] != b'T' && b[10] != b't') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let year: i32 = std::str::from_utf8(&b[0..4]).ok()?.parse().ok()?;
    let month: u32 = std::str::from_utf8(&b[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&b[8..10]).ok()?.parse().ok()?;
    let hour: u32 = std::str::from_utf8(&b[11..13]).ok()?.parse().ok()?;
    let minute: u32 = std::str::from_utf8(&b[14..16]).ok()?.parse().ok()?;
    let second: u32 = std::str::from_utf8(&b[17..19]).ok()?.parse().ok()?;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + (hour as i64) * 3_600 + (minute as i64) * 60 + (second as i64);
    Some(ParsedUtc {
        year,
        month,
        day,
        hour,
        minute,
        timestamp_ms: secs * 1000,
    })
}

/// Extracts the 48-bit Unix millisecond timestamp from a UUIDv7 string without allocating.
fn uuid_prefix_timestamp_ms(uuid: &str) -> i64 {
    let mut acc: i64 = 0;
    let mut digits = 0usize;
    for b in uuid.bytes() {
        if b == b'-' {
            continue;
        }
        let val = match b {
            b'0'..=b'9' => (b - b'0') as i64,
            b'a'..=b'f' => (b - b'a' + 10) as i64,
            b'A'..=b'F' => (b - b'A' + 10) as i64,
            _ => return 0,
        };
        acc = (acc << 4) | val;
        digits += 1;
        if digits == 12 {
            return acc;
        }
    }
    0
}

fn extract_thumb_ids(thumb: &str) -> Option<(&str, &str)> {
    let idx = thumb.find("video_thumbnails/")?;
    let rest = &thumb[idx + "video_thumbnails/".len()..];
    let mut parts = rest.split('/');
    let session = parts.next().filter(|s| !s.is_empty())?;
    let segment = parts.next().filter(|s| !s.is_empty())?;
    Some((session, segment))
}

fn get_cached_prefixes(
    client: &reqwest::blocking::Client,
    tx: &Sender<WorkerEvent>,
) -> Result<&'static [String], String> {
    static CACHED_PREFIXES: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(list) = CACHED_PREFIXES.get() {
        return Ok(list.as_slice());
    }
    let _ = tx.send(WorkerEvent::Log(
        "[extractor] Fetching CDN prefixes list from kickvod-extractor...".to_string(),
    ));
    let val = get_kick_json(client, PREFIXES_URL, Some(tx))?;
    let mut parsed = Vec::new();
    if let Some(arr) = val.as_array() {
        parsed.reserve(arr.len());
        for item in arr {
            if let Some(s) = item.as_str() {
                parsed.push(s.to_string());
            }
        }
    }
    let _ = CACHED_PREFIXES.set(parsed);
    Ok(CACHED_PREFIXES.get().map(|v| v.as_slice()).unwrap_or(&[]))
}

/// Sanitizes a VOD or Clip title into a safe cross-platform filename stem.
fn sanitize_filename(raw_title: &str, fallback: &str) -> String {
    let mut cleaned = String::with_capacity(raw_title.len());
    let mut last_was_space = false;

    for ch in raw_title.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            if !last_was_space && !cleaned.is_empty() {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else if ch.is_whitespace() {
            if !last_was_space && !cleaned.is_empty() {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else {
            cleaned.push(ch);
            last_was_space = false;
        }
    }

    let trimmed = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.');
    let chosen = if trimmed.is_empty() {
        fallback.trim_matches(|c: char| c.is_whitespace() || c == '.')
    } else {
        trimmed
    };

    let mut final_stem: String = chosen.chars().take(150).collect();
    while final_stem.ends_with('.') || final_stem.ends_with(' ') {
        final_stem.pop();
    }

    // Guard against reserved Windows device filenames
    let upper = final_stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "" | "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        return "Kick_VOD".to_string();
    }

    final_stem
}

/// Builds the full output file path using the VOD/Clip title and disambiguates if the file already exists.
fn build_output_filepath(
    output_dir: &str,
    session_title: &str,
    channel_slug: &str,
    ext_str: &str,
) -> String {
    let trimmed_dir = output_dir.trim();
    let lower_dir = trimmed_dir.to_ascii_lowercase();
    if lower_dir.ends_with(".mkv") || lower_dir.ends_with(".mp4") || lower_dir.ends_with(".ts") {
        return trimmed_dir.to_string();
    }

    let fallback_name = if channel_slug.is_empty() {
        "Kick_VOD".to_string()
    } else {
        format!("{}_VOD", channel_slug)
    };
    let stem = sanitize_filename(session_title, &fallback_name);
    let base_dir = Path::new(trimmed_dir);

    let first_candidate = base_dir.join(format!("{}.{}", stem, ext_str));
    if !first_candidate.exists() {
        return first_candidate.display().to_string();
    }

    for idx in 1..1000 {
        let candidate = base_dir.join(format!("{} ({}).{}", stem, idx, ext_str));
        if !candidate.exists() {
            return candidate.display().to_string();
        }
    }

    first_candidate.display().to_string()
}

fn non_empty_str<'a>(val: Option<&'a Value>) -> Option<&'a str> {
    val.and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn ytdlp_extract_kick_fallback(
    raw_url: &str,
    tx: &Sender<WorkerEvent>,
) -> Option<ExtractedVodMeta> {
    let _ = tx.send(WorkerEvent::Log(
        "[extractor] Trying yt-dlp metadata extraction fallback...".to_string(),
    ));
    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "--dump-single-json",
        "--no-warnings",
        "--no-playlist",
        raw_url,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let val: Value = serde_json::from_slice(&out.stdout).ok()?;
    let m3u8_url = non_empty_str(val.get("manifest_url"))
        .or_else(|| non_empty_str(val.get("url")))?
        .to_string();

    let channel_slug = non_empty_str(val.get("channel"))
        .or_else(|| non_empty_str(val.get("uploader")))
        .or_else(|| non_empty_str(val.get("creator")))
        .unwrap_or("kick_vod")
        .to_string();
    let session_title = non_empty_str(val.get("title"))
        .or_else(|| non_empty_str(val.get("fulltitle")))
        .unwrap_or(&channel_slug)
        .to_string();
    let duration_f64 = val
        .get("duration")
        .and_then(|d| d.as_f64().or_else(|| d.as_u64().map(|u| u as f64)))
        .filter(|&d| d.is_finite() && d > 0.0);
    let duration_secs = duration_f64.map(|f| f as u64).unwrap_or(0);
    let duration_formatted = if duration_secs > 0 {
        format!(
            "{:02}:{:02}:{:02}",
            duration_secs / 3600,
            (duration_secs % 3600) / 60,
            duration_secs % 60
        )
    } else {
        "Full VOD".to_string()
    };

    Some(ExtractedVodMeta {
        m3u8_url,
        channel_slug,
        session_title,
        duration_formatted,
        duration_secs: duration_f64,
        source_type: "yt-dlp fallback".to_string(),
    })
}

/// Extracts Clip metadata and stream URL from Kick API v2 (`https://kick.com/api/v2/clips/{clip_id}`).
fn extract_kick_clip(
    client: &reqwest::blocking::Client,
    user_hint: &str,
    clip_id: &str,
    raw_url: &str,
    tx: &Sender<WorkerEvent>,
) -> Result<ExtractedVodMeta, String> {
    let _ = tx.send(WorkerEvent::Log(format!(
        "[extractor] Fetching Kick clip metadata for '{}'...",
        clip_id
    )));
    let clip_api = format!("https://kick.com/api/v2/clips/{}", clip_id);
    if let Ok(val) = get_kick_json(client, &clip_api, Some(tx)) {
        let clip_obj = val.get("clip").unwrap_or(&val);
        let stream_url = non_empty_str(clip_obj.get("clip_url"))
            .or_else(|| non_empty_str(clip_obj.get("video_url")))
            .or_else(|| non_empty_str(clip_obj.get("source")));

        if let Some(m3u8_url) = stream_url {
            let channel_slug =
                non_empty_str(clip_obj.get("channel").and_then(|c| c.get("slug")))
                    .or_else(|| {
                        non_empty_str(clip_obj.get("channel").and_then(|c| c.get("username")))
                    })
                    .unwrap_or(if user_hint.is_empty() {
                        "kick_clip"
                    } else {
                        user_hint
                    })
                    .to_string();

            let session_title = non_empty_str(clip_obj.get("title"))
                .unwrap_or(clip_id)
                .to_string();

            let duration_f64 = clip_obj
                .get("duration")
                .and_then(|d| d.as_f64().or_else(|| d.as_u64().map(|u| u as f64)))
                .filter(|&d| d.is_finite() && d > 0.0);
            let duration_secs = duration_f64.map(|f| f as u64).unwrap_or(0);
            let duration_formatted = if duration_secs > 0 {
                format!(
                    "{:02}:{:02}:{:02}",
                    duration_secs / 3600,
                    (duration_secs % 3600) / 60,
                    duration_secs % 60
                )
            } else {
                "Clip".to_string()
            };

            return Ok(ExtractedVodMeta {
                m3u8_url: m3u8_url.to_string(),
                channel_slug,
                session_title,
                duration_formatted,
                duration_secs: duration_f64,
                source_type: "Kick Clip API".to_string(),
            });
        }
    }

    if let Some(meta) = ytdlp_extract_kick_fallback(raw_url, tx) {
        return Ok(meta);
    }
    Err(format!("Could not extract Kick clip '{}'", clip_id))
}

/// Extracts Master M3U8 and metadata from a Kick VOD or Clip URL.
/// Full port of https://github.com/mortyobnoxious/kickvod-extractor
fn extract_kick_vod(raw_url: &str, tx: &Sender<WorkerEvent>) -> Result<ExtractedVodMeta, String> {
    let trimmed = raw_url.trim();
    if trimmed.contains(".m3u8") {
        let fallback_title = trimmed
            .rsplit('/')
            .next()
            .and_then(|s| s.split(&['?', '#'][..]).next())
            .and_then(|s| s.strip_suffix(".m3u8"))
            .filter(|s| !s.is_empty() && *s != "master" && *s != "playlist")
            .unwrap_or("Kick_HLS_Stream")
            .to_string();
        return Ok(ExtractedVodMeta {
            m3u8_url: trimmed.to_string(),
            channel_slug: "kick_stream".to_string(),
            session_title: fallback_title,
            duration_formatted: "Live/VOD".to_string(),
            duration_secs: None,
            source_type: "Direct M3U8 URL".to_string(),
        });
    }

    let mut user = "";
    let mut uuid = "";
    let mut clip_id = "";

    // Check for ?clip= query param first
    if let Some(q_idx) = trimmed.find("clip=") {
        let after = &trimmed[q_idx + 5..];
        clip_id = after.split(&['&', '#'][..]).next().unwrap_or("");
    }

    if let Some(idx) = trimmed.find("kick.com/") {
        let rest = &trimmed[idx + "kick.com/".len()..];
        let mut parts = rest.split('/');
        if let Some(p0) = parts.next() {
            let clean_p0 = p0.split(&['?', '#'][..]).next().unwrap_or("");
            if let Some(p1) = parts.next() {
                let clean_p1 = p1.split(&['?', '#'][..]).next().unwrap_or("");
                if clean_p1 == "videos" {
                    user = clean_p0;
                    uuid = parts
                        .next()
                        .unwrap_or("")
                        .split(&['?', '#'][..])
                        .next()
                        .unwrap_or("");
                } else if clean_p0 == "video" {
                    uuid = clean_p1;
                } else if clean_p1 == "clips" {
                    user = clean_p0;
                    clip_id = parts
                        .next()
                        .unwrap_or("")
                        .split(&['?', '#'][..])
                        .next()
                        .unwrap_or("");
                } else if clean_p0 == "clips" || clean_p0 == "clip" {
                    clip_id = clean_p1;
                }
            } else if !clean_p0.is_empty() {
                user = clean_p0;
            }
        }
    }

    let client = shared_http_client()?;

    if !clip_id.is_empty() {
        return extract_kick_clip(client, user, clip_id, trimmed, tx);
    }

    if uuid.is_empty() {
        if let Some(meta) = ytdlp_extract_kick_fallback(trimmed, tx) {
            return Ok(meta);
        }
        return Err(
            "Invalid Kick VOD/Clip URL (expected https://kick.com/{channel}/videos/{uuid})".to_string(),
        );
    }

    // 1. Try v1/video/{uuid} first, then fallback to v2/channels/{user}/videos and v1/channels/{user}
    let v1_url = format!("https://kick.com/api/v1/video/{}", uuid);
    let mut vod_obj: Option<Value> = get_kick_json(client, &v1_url, None).ok();
    let mut v2_matched_obj: Option<Value> = None;
    let mut api_err_opt: Option<String> = None;

    if vod_obj.is_none() && !user.is_empty() {
        let _ = tx.send(WorkerEvent::Log(format!(
            "[extractor] Querying v2 channel videos for '{}' (UUIDv7 lookup)...",
            user
        )));

        let mut video_items: Vec<Value> = Vec::new();
        let v2_url = format!("https://kick.com/api/v2/channels/{}/videos", user);
        match get_kick_json(client, &v2_url, Some(tx)) {
            Ok(Value::Array(arr)) => {
                video_items = arr;
            }
            Ok(_) => {}
            Err(e) => {
                api_err_opt = Some(e);
                let _ = tx.send(WorkerEvent::Log(format!(
                    "[extractor] v2 lookup failed, trying v1 channel endpoint for '{}'...",
                    user
                )));
                let v1_ch_url = format!("https://kick.com/api/v1/channels/{}", user);
                if let Ok(mut ch_val) = get_kick_json(client, &v1_ch_url, Some(tx)) {
                    if let Some(Value::Array(arr)) = ch_val
                        .as_object_mut()
                        .and_then(|m| m.remove("previous_livestreams"))
                    {
                        video_items = arr;
                    }
                }
            }
        }

        if !video_items.is_empty() {
            let uuid_time_ms = uuid_prefix_timestamp_ms(uuid);

            for v in video_items {
                let vod_id_match = v.get("vod_id").and_then(|s| s.as_str()) == Some(uuid);
                let video_uuid_match = v
                    .get("video")
                    .and_then(|vid| vid.get("uuid"))
                    .and_then(|s| s.as_str())
                    == Some(uuid);

                let time_match = if uuid_time_ms > 0 {
                    let date_str = v
                        .get("video")
                        .and_then(|vid| vid.get("created_at"))
                        .and_then(|s| s.as_str())
                        .or_else(|| v.get("created_at").and_then(|s| s.as_str()))
                        .or_else(|| v.get("start_time").and_then(|s| s.as_str()))
                        .unwrap_or("");
                    if let Some(dt) = parse_kick_utc(date_str) {
                        (dt.timestamp_ms - uuid_time_ms).abs() < 30_000
                    } else {
                        false
                    }
                } else {
                    false
                };

                if vod_id_match || video_uuid_match || time_match {
                    let has_source = v
                        .get("source")
                        .and_then(|s| s.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    if !has_source {
                        if let Some(inner_uuid) = v
                            .get("video")
                            .and_then(|vid| vid.get("uuid"))
                            .and_then(|s| s.as_str())
                        {
                            let inner_v1 = format!("https://kick.com/api/v1/video/{}", inner_uuid);
                            if let Ok(v1_res) = get_kick_json(client, &inner_v1, None) {
                                vod_obj = Some(v1_res);
                            }
                        }
                    }
                    v2_matched_obj = Some(v);
                    break;
                }
            }
        }
    }

    let vod = match vod_obj.as_ref().or(v2_matched_obj.as_ref()) {
        Some(v) => v,
        None => {
            if let Some(meta) = ytdlp_extract_kick_fallback(trimmed, tx) {
                return Ok(meta);
            }
            return Err(api_err_opt.unwrap_or_else(|| "VOD not found in Kick database".to_string()));
        }
    };

    let ls = vod.get("livestream").or(v2_matched_obj.as_ref());

    let channel_slug = non_empty_str(ls.and_then(|l| l.get("channel")).and_then(|c| c.get("slug")))
        .or_else(|| non_empty_str(vod.get("channel").and_then(|c| c.get("slug"))))
        .unwrap_or(if user.is_empty() { "kick_vod" } else { user })
        .to_string();

    // Pull the actual VOD/Clip title from all possible Kick v1 & v2 fields
    let session_title = non_empty_str(ls.and_then(|l| l.get("session_title")))
        .or_else(|| non_empty_str(ls.and_then(|l| l.get("title"))))
        .or_else(|| non_empty_str(vod.get("session_title")))
        .or_else(|| non_empty_str(vod.get("title")))
        .or_else(|| {
            non_empty_str(
                v2_matched_obj
                    .as_ref()
                    .and_then(|m| m.get("session_title").or_else(|| m.get("title"))),
            )
        })
        .or_else(|| {
            non_empty_str(
                vod.get("video")
                    .and_then(|v| v.get("session_title").or_else(|| v.get("title"))),
            )
        })
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} - {}", channel_slug, uuid));

    let duration_ms = ls
        .and_then(|l| l.get("duration"))
        .or_else(|| vod.get("duration"))
        .and_then(|d| d.as_u64())
        .unwrap_or(0);
    let total_secs = if duration_ms > 100_000 {
        duration_ms / 1000
    } else {
        duration_ms
    };
    let duration_formatted = if total_secs > 0 {
        format!(
            "{:02}:{:02}:{:02}",
            total_secs / 3600,
            (total_secs % 3600) / 60,
            total_secs % 60
        )
    } else {
        "Full VOD".to_string()
    };

    let duration_secs_opt = if total_secs > 0 {
        Some(total_secs as f64)
    } else {
        None
    };

    // 2. Direct source URL from Kick API (v1 or v2)
    let direct_source = non_empty_str(vod.get("source")).or_else(|| {
        non_empty_str(v2_matched_obj.as_ref().and_then(|m| m.get("source")))
    });

    if let Some(src) = direct_source {
        return Ok(ExtractedVodMeta {
            m3u8_url: src.to_string(),
            channel_slug,
            session_title,
            duration_formatted,
            duration_secs: duration_secs_opt,
            source_type: "Source URL from API".to_string(),
        });
    }

    // 3. Reconstruct AWS IVS path from thumbnail & UTC timestamp + prefixes.json
    let thumb_str = ls
        .and_then(|l| l.get("thumbnail"))
        .and_then(|t| t.as_str().or_else(|| t.get("src").and_then(|s| s.as_str())))
        .or_else(|| {
            vod.get("thumbnail").and_then(|t| {
                t.as_str()
                    .or_else(|| t.get("src").and_then(|s| s.as_str()))
                    .or_else(|| t.get("url").and_then(|s| s.as_str()))
            })
        })
        .unwrap_or("");

    let (session, segment) = extract_thumb_ids(thumb_str)
        .ok_or_else(|| "Thumbnail session/segment extraction failed".to_string())?;

    let mut candidate_paths: Vec<String> = Vec::with_capacity(4);
    for raw_ts in [
        ls.and_then(|l| l.get("start_time")).and_then(|s| s.as_str()),
        ls.and_then(|l| l.get("created_at")).and_then(|s| s.as_str()),
        vod.get("start_time").and_then(|s| s.as_str()),
        vod.get("created_at").and_then(|s| s.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(dt) = parse_kick_utc(raw_ts) {
            let ivs_path = format!(
                "ivs/v1/{}/{}/{}/{}/media/hls/master.m3u8",
                AWS_ID,
                session,
                dt.ivs_time_path(),
                segment
            );
            if !candidate_paths.contains(&ivs_path) {
                candidate_paths.push(ivs_path);
            }
        }
    }

    if candidate_paths.is_empty() {
        return Err("Could not parse VOD creation timestamp".to_string());
    }

    let _ = tx.send(WorkerEvent::Log(
        "[extractor] Testing direct AWS IVS stream URL...".to_string(),
    ));
    for ivs_path in &candidate_paths {
        let direct_url = format!("https://stream.kick.com/{}", ivs_path);
        if test_m3u8_url(client, &direct_url) {
            return Ok(ExtractedVodMeta {
                m3u8_url: direct_url,
                channel_slug,
                session_title,
                duration_formatted,
                duration_secs: duration_secs_opt,
                source_type: "Direct URL (no prefix)".to_string(),
            });
        }
    }

    let prefixes = get_cached_prefixes(client, tx)?;
    for prefix in prefixes {
        for ivs_path in &candidate_paths {
            let candidate_url = if prefix.is_empty() {
                format!("https://stream.kick.com/{}", ivs_path)
            } else {
                format!("https://stream.kick.com/{}/{}", prefix, ivs_path)
            };
            if test_m3u8_url(client, &candidate_url) {
                return Ok(ExtractedVodMeta {
                    m3u8_url: candidate_url,
                    channel_slug,
                    session_title,
                    duration_formatted,
                    duration_secs: duration_secs_opt,
                    source_type: format!("Found with prefix: {}", prefix),
                });
            }
        }
    }

    Err("No working M3U8 URL found with available prefixes".to_string())
}

#[derive(Clone, Copy, Default)]
struct StreamProbeInfo {
    duration_secs: Option<f64>,
    fps: Option<f64>,
    total_frames: Option<u64>,
    exact_frame_count: bool,
}

#[derive(Clone, Copy)]
struct ProgressTarget {
    duration_secs: Option<f64>,
    fps: Option<f64>,
    total_frames: Option<u64>,
    exact_frame_count: bool,
    start_secs: Option<f64>,
    end_secs: Option<f64>,
    audio_only: bool,
}

impl ProgressTarget {
    fn update_from_stream_duration(&mut self, full_secs: f64) {
        if !full_secs.is_finite() || full_secs <= 0.0 {
            return;
        }
        let target = match (self.start_secs, self.end_secs) {
            (Some(s), Some(e)) if e > s => Some((e - s).min(full_secs)),
            (None, Some(e)) if e > 0.0 => Some(e.min(full_secs)),
            (Some(s), None) if full_secs > s => Some(full_secs - s),
            (None, None) => Some(full_secs),
            _ => Some(full_secs),
        };
        if let Some(d) = target.filter(|&v| v > 0.0) {
            self.duration_secs = Some(d);
            if !self.audio_only && !self.exact_frame_count {
                if let Some(fps) = self.fps.filter(|&f| f > 0.0) {
                    self.total_frames = Some((d * fps).round() as u64);
                }
            }
        }
    }

    fn update_fps(&mut self, fps: f64) {
        if !fps.is_finite() || fps <= 0.0 || fps > 240.0 || self.audio_only {
            return;
        }
        self.fps = Some(fps);
        if !self.exact_frame_count {
            if let Some(d) = self.duration_secs.filter(|&v| v > 0.0) {
                self.total_frames = Some((d * fps).round() as u64);
            }
        }
    }
}

fn parse_hms_to_secs(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("N/A") || s.starts_with('-') {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    let secs = match parts.len() {
        3 => {
            let h: f64 = parts[0].parse().ok()?;
            let m: f64 = parts[1].parse().ok()?;
            let sec: f64 = parts[2].parse().ok()?;
            h * 3600.0 + m * 60.0 + sec
        }
        2 => {
            let m: f64 = parts[0].parse().ok()?;
            let sec: f64 = parts[1].parse().ok()?;
            m * 60.0 + sec
        }
        1 => parts[0].parse::<f64>().ok()?,
        _ => return None,
    };
    if secs.is_finite() && secs >= 0.0 {
        Some(secs)
    } else {
        None
    }
}

fn format_secs_hms(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

fn parse_rational_fps(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() || s == "0/0" || s.eq_ignore_ascii_case("N/A") {
        return None;
    }
    let fps = if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.trim().parse().ok()?;
        let d: f64 = den.trim().parse().ok()?;
        if d <= 0.0 {
            return None;
        }
        n / d
    } else {
        s.parse::<f64>().ok()?
    };
    if fps.is_finite() && fps > 0.5 && fps <= 240.0 {
        Some(fps)
    } else {
        None
    }
}

fn resolve_relative_url(base_url: &str, relative: &str) -> String {
    let rel = relative.trim();
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    let base_no_q = base_url.split(&['?', '#'][..]).next().unwrap_or(base_url);
    if rel.starts_with('/') {
        if let Some(scheme_pos) = base_no_q.find("://") {
            let after_scheme = &base_no_q[scheme_pos + 3..];
            let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
            let origin = &base_no_q[..scheme_pos + 3 + host_end];
            return format!("{}{}", origin, rel);
        }
    }
    if let Some(slash) = base_no_q.rfind('/') {
        format!("{}{}", &base_no_q[..=slash], rel)
    } else {
        rel.to_string()
    }
}

fn fetch_text_with_kick_headers(url: &str) -> Option<String> {
    if let Ok(client) = shared_http_client() {
        if let Ok(resp) = client
            .get(url)
            .header("Origin", "https://kick.com")
            .header("Referer", "https://kick.com/")
            .send()
        {
            if resp.status().is_success() {
                if let Ok(body) = resp.text() {
                    if !body.trim().is_empty() {
                        return Some(body);
                    }
                }
            }
        }
        let proxied = format!("{}{}", VIDDASTRAGE_PROXY, encode_uri_component(url));
        if let Ok(resp) = client.get(&proxied).send() {
            if resp.status().is_success() {
                if let Ok(body) = resp.text() {
                    if !body.trim().is_empty() {
                        return Some(body);
                    }
                }
            }
        }
    }
    curl_fetch_text(url)
}

fn parse_m3u8_attr<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{}=", key);
    let mut search = attrs;
    while let Some(pos) = search.find(&needle) {
        let boundary_ok = pos == 0 || matches!(search.as_bytes()[pos - 1], b',' | b' ' | b':');
        let rest = &search[pos + needle.len()..];
        if boundary_ok {
            if let Some(stripped) = rest.strip_prefix('"') {
                let end = stripped.find('"').unwrap_or(stripped.len());
                return Some(&stripped[..end]);
            }
            let end = rest.find(',').unwrap_or(rest.len());
            return Some(rest[..end].trim());
        }
        search = rest;
    }
    None
}

fn infer_fps_from_variant_uri(uri: &str) -> Option<f64> {
    let lower = uri.to_ascii_lowercase();
    if lower.contains("p60") || lower.contains("60fps") {
        Some(60.0)
    } else if lower.contains("p50") || lower.contains("50fps") {
        Some(50.0)
    } else if lower.contains("p30") || lower.contains("30fps") {
        Some(30.0)
    } else {
        None
    }
}

fn probe_hls_stream(m3u8_url: &str, res: Resolution) -> StreamProbeInfo {
    let mut info = StreamProbeInfo::default();
    let Some(master_text) = fetch_text_with_kick_headers(m3u8_url) else {
        return info;
    };
    if !master_text.contains("#EXTM3U") {
        return info;
    }

    let media_text = if master_text.contains("#EXT-X-STREAM-INF:") {
        struct Variant {
            height: u32,
            fps: Option<f64>,
            bandwidth: u64,
            uri: String,
        }
        let mut variants: Vec<Variant> = Vec::new();
        let mut pending: Option<(u32, Option<f64>, u64)> = None;

        for raw_line in master_text.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                let height = parse_m3u8_attr(attrs, "RESOLUTION")
                    .and_then(|r| r.split('x').nth(1))
                    .and_then(|h| h.parse::<u32>().ok())
                    .unwrap_or(0);
                let fps = parse_m3u8_attr(attrs, "FRAME-RATE")
                    .and_then(|f| f.parse::<f64>().ok())
                    .filter(|&f| f.is_finite() && f > 0.5 && f <= 240.0);
                let bandwidth = parse_m3u8_attr(attrs, "BANDWIDTH")
                    .and_then(|b| b.parse::<u64>().ok())
                    .unwrap_or(0);
                pending = Some((height, fps, bandwidth));
            } else if !line.starts_with('#') {
                if let Some((height, fps, bandwidth)) = pending.take() {
                    variants.push(Variant {
                        height,
                        fps: fps.or_else(|| infer_fps_from_variant_uri(line)),
                        bandwidth,
                        uri: line.to_string(),
                    });
                }
            }
        }

        let (max_h, max_fps): (Option<u32>, Option<f64>) = match res {
            Resolution::R1080p60 => (Some(1080), Some(60.5)),
            Resolution::R1080p => (Some(1080), None),
            Resolution::R720p60 => (Some(720), Some(60.5)),
            Resolution::R720p => (Some(720), None),
            Resolution::R480p => (Some(480), None),
            Resolution::R360p => (Some(360), None),
            Resolution::AudioOnly => (None, None),
        };

        fn pick_best<'a>(items: impl Iterator<Item = &'a Variant>) -> Option<&'a Variant> {
            items.max_by_key(|v| {
                (
                    v.height,
                    v.fps.map(|f| (f * 100.0) as u64).unwrap_or(0),
                    v.bandwidth,
                )
            })
        }

        let filtered = variants.iter().filter(|v| {
            let h_ok = max_h.map(|mh| v.height == 0 || v.height <= mh).unwrap_or(true);
            let f_ok = match (max_fps, v.fps) {
                (Some(mf), Some(vf)) => vf <= mf,
                _ => true,
            };
            h_ok && f_ok
        });

        let chosen = pick_best(filtered).or_else(|| pick_best(variants.iter()));
        if let Some(variant) = chosen {
            info.fps = variant.fps;
            let media_url = resolve_relative_url(m3u8_url, &variant.uri);
            fetch_text_with_kick_headers(&media_url).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        info.fps = infer_fps_from_variant_uri(m3u8_url);
        master_text
    };

    let mut total_dur = 0.0_f64;
    let mut seg_count = 0u64;
    for line in media_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#EXTINF:") {
            let num_part = rest.split(',').next().unwrap_or("").trim();
            if let Ok(sec) = num_part.parse::<f64>() {
                if sec.is_finite() && sec > 0.0 {
                    total_dur += sec;
                    seg_count += 1;
                }
            }
        }
    }

    if seg_count > 0 && total_dur > 0.0 {
        info.duration_secs = Some(total_dur);
    }
    info
}

fn probe_with_ffprobe(url: &str) -> StreamProbeInfo {
    let mut info = StreamProbeInfo::default();
    let mut cmd = Command::new("ffprobe");
    cmd.args([
        "-v",
        "error",
        "-user_agent",
        CHROME_UA,
        "-headers",
        "Origin: https://kick.com\r\nReferer: https://kick.com/\r\n",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=nb_frames,avg_frame_rate,r_frame_rate,duration:format=duration",
        "-of",
        "default=noprint_wrappers=1",
        url,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let Ok(out) = cmd.output() else {
        return info;
    };
    if !out.status.success() {
        return info;
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut avg_fps: Option<f64> = None;
    let mut r_fps: Option<f64> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "nb_frames" => {
                    if let Ok(n) = v.trim().parse::<u64>() {
                        if n > 0 {
                            info.total_frames = Some(n);
                            info.exact_frame_count = true;
                        }
                    }
                }
                "avg_frame_rate" => {
                    if avg_fps.is_none() {
                        avg_fps = parse_rational_fps(v);
                    }
                }
                "r_frame_rate" => {
                    if r_fps.is_none() {
                        r_fps = parse_rational_fps(v);
                    }
                }
                "duration" => {
                    if let Ok(d) = v.trim().parse::<f64>() {
                        if d.is_finite() && d > 0.0 && info.duration_secs.is_none() {
                            info.duration_secs = Some(d);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    info.fps = avg_fps.or(r_fps);
    info
}

fn probe_stream_for_progress(
    stream_url: &str,
    res: Resolution,
    api_duration_secs: Option<f64>,
    start_t: &str,
    end_t: &str,
    tx: &Sender<WorkerEvent>,
) -> ProgressTarget {
    let audio_only = res == Resolution::AudioOnly;
    let mut probe = if stream_url.to_ascii_lowercase().contains(".m3u8") {
        probe_hls_stream(stream_url, res)
    } else {
        StreamProbeInfo::default()
    };

    let needs_ffprobe = probe.duration_secs.is_none()
        || (!audio_only && probe.fps.is_none() && probe.total_frames.is_none());
    if needs_ffprobe {
        let fp = probe_with_ffprobe(stream_url);
        if probe.duration_secs.is_none() {
            probe.duration_secs = fp.duration_secs;
        }
        if probe.fps.is_none() {
            probe.fps = fp.fps;
        }
        if probe.total_frames.is_none() {
            probe.total_frames = fp.total_frames;
            probe.exact_frame_count = fp.exact_frame_count;
        }
    }

    let start_secs = parse_hms_to_secs(start_t);
    let end_secs = parse_hms_to_secs(end_t);
    let is_sliced = start_secs.is_some() || end_secs.is_some();

    let full_duration = probe.duration_secs.or(api_duration_secs);
    let target_duration_secs = match (start_secs, end_secs, full_duration) {
        (Some(s), Some(e), _) if e > s => Some(e - s),
        (None, Some(e), Some(full)) if e > 0.0 => Some(e.min(full)),
        (None, Some(e), None) if e > 0.0 => Some(e),
        (Some(s), None, Some(full)) if full > s => Some(full - s),
        (None, None, Some(full)) if full > 0.0 => Some(full),
        _ => full_duration.filter(|&d| d > 0.0),
    };

    let exact_frame_count = !audio_only && !is_sliced && probe.exact_frame_count;
    let total_frames = if audio_only {
        None
    } else if exact_frame_count {
        probe.total_frames
    } else {
        match (target_duration_secs, probe.fps) {
            (Some(d), Some(fps)) if d > 0.0 && fps > 0.0 => Some((d * fps).round() as u64),
            _ => None,
        }
    };

    let label = if is_sliced {
        "Slice target"
    } else {
        "Stream target"
    };
    match (target_duration_secs, total_frames, probe.fps) {
        (Some(d), Some(tf), Some(fps)) => {
            let _ = tx.send(WorkerEvent::Log(format!(
                "[progress] {}: {} ({} frames @ {:.2} fps)",
                label,
                format_secs_hms(d),
                format_with_commas(tf),
                fps
            )));
        }
        (Some(d), Some(tf), None) => {
            let _ = tx.send(WorkerEvent::Log(format!(
                "[progress] {}: {} ({} frames)",
                label,
                format_secs_hms(d),
                format_with_commas(tf)
            )));
        }
        (Some(d), None, _) => {
            let _ = tx.send(WorkerEvent::Log(format!(
                "[progress] {}: {}",
                label,
                format_secs_hms(d)
            )));
        }
        (None, Some(tf), _) => {
            let _ = tx.send(WorkerEvent::Log(format!(
                "[progress] {}: {} frames",
                label,
                format_with_commas(tf)
            )));
        }
        (None, None, _) => {}
    }

    ProgressTarget {
        duration_secs: target_duration_secs,
        fps: probe.fps,
        total_frames,
        exact_frame_count,
        start_secs,
        end_secs,
        audio_only,
    }
}

fn read_cr_lf_lines<R: Read, F: FnMut(String) -> bool>(reader: R, mut on_line: F) {
    let mut br = BufReader::new(reader);
    let mut buf = [0u8; 4096];
    let mut line_bytes: Vec<u8> = Vec::with_capacity(256);

    while let Ok(n) = br.read(&mut buf) {
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            if b == b'\r' || b == b'\n' {
                if !line_bytes.is_empty() {
                    let s = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    line_bytes.clear();
                    if !s.is_empty() && !on_line(s) {
                        return;
                    }
                }
            } else {
                line_bytes.push(b);
            }
        }
    }
    if !line_bytes.is_empty() {
        let s = String::from_utf8_lossy(&line_bytes).trim().to_string();
        if !s.is_empty() {
            let _ = on_line(s);
        }
    }
}

fn extract_ffmpeg_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut search = line;
    while let Some(pos) = search.find(key) {
        let boundary_ok = pos == 0 || search.as_bytes()[pos - 1].is_ascii_whitespace();
        let after = &search[pos + key.len()..];
        if boundary_ok {
            let val = after.trim_start().split_whitespace().next()?;
            return Some(val);
        }
        search = after;
    }
    None
}

fn parse_ffmpeg_header_duration(line: &str) -> Option<f64> {
    let idx = line.find("Duration:")?;
    let rest = line[idx + "Duration:".len()..].trim_start();
    let token = rest.split(&[',', ' '][..]).next()?;
    parse_hms_to_secs(token)
}

fn parse_ffmpeg_header_fps(line: &str) -> Option<f64> {
    if !line.contains("Video:") {
        return None;
    }
    for marker in [" fps", " tbr"] {
        if let Some(pos) = line.find(marker) {
            let before = line[..pos].trim_end();
            let token = before
                .rsplit(&[',', ' '][..])
                .next()
                .unwrap_or("")
                .trim();
            if let Ok(fps) = token.parse::<f64>() {
                if fps.is_finite() && fps > 0.5 && fps <= 240.0 {
                    return Some(fps);
                }
            }
        }
    }
    None
}

fn compute_ffmpeg_progress(
    line: &str,
    target: &ProgressTarget,
) -> (Option<f32>, String) {
    let cur_frame = extract_ffmpeg_field(line, "frame=").and_then(|v| v.parse::<u64>().ok());
    let cur_time = extract_ffmpeg_field(line, "time=").and_then(parse_hms_to_secs);
    let speed = extract_ffmpeg_field(line, "speed=")
        .filter(|s| !s.eq_ignore_ascii_case("N/A"));

    let frame_ratio = match (cur_frame, target.total_frames) {
        (Some(f), Some(tf)) if tf > 0 => Some((f as f64 / tf as f64).clamp(0.0, 0.999)),
        _ => None,
    };
    let time_ratio = match (cur_time, target.duration_secs) {
        (Some(t), Some(td)) if td > 0.0 => Some((t / td).clamp(0.0, 0.999)),
        _ => None,
    };

    let ratio = if target.exact_frame_count {
        frame_ratio.or(time_ratio)
    } else {
        time_ratio
            .filter(|&r| r > 0.0)
            .or(frame_ratio)
            .or(time_ratio)
    }
    .map(|r| r as f32);

    let mut parts: Vec<String> = Vec::with_capacity(3);
    match (cur_frame, target.total_frames) {
        (Some(f), Some(tf)) if tf > 0 => {
            parts.push(format!(
                "Frame {} / {}",
                format_with_commas(f),
                format_with_commas(tf)
            ));
        }
        (Some(f), None) if f > 0 => {
            parts.push(format!("Frame {}", format_with_commas(f)));
        }
        _ => {}
    }

    match (cur_time, target.duration_secs) {
        (Some(t), Some(td)) if td > 0.0 => {
            parts.push(format!("{} / {}", format_secs_hms(t), format_secs_hms(td)));
        }
        (Some(t), None) => {
            parts.push(format_secs_hms(t));
        }
        _ => {}
    }

    if let Some(sp) = speed {
        parts.push(sp.to_string());
    }

    let status_text = if parts.is_empty() {
        line.to_string()
    } else {
        parts.join(" · ")
    };

    (ratio, status_text)
}

fn read_clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let text = clipboard.get_text().ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn open_output_folder(folder: &str, saved_file: Option<&str>) {
    #[cfg(target_os = "windows")]
    {
        if let Some(file) = saved_file {
            if !file.is_empty() && Path::new(file).exists() {
                let _ = Command::new("explorer")
                    .arg(format!("/select,{}", file))
                    .spawn();
                return;
            }
        }
        if !folder.is_empty() {
            let _ = Command::new("explorer").arg(folder).spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(file) = saved_file {
            if !file.is_empty() && Path::new(file).exists() {
                let _ = Command::new("open").args(["-R", file]).spawn();
                return;
            }
        }
        if !folder.is_empty() {
            let _ = Command::new("open").arg(folder).spawn();
        }
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = saved_file;
        if !folder.is_empty() {
            let _ = Command::new("xdg-open").arg(folder).spawn();
        }
    }
}

/// Lightweight geometric vector brand mark (5 primitives total instead of 288 pixel quads).
fn draw_brand_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(7.0), BG_SURFACE);
    painter.rect_stroke(rect, Rounding::same(7.0), Stroke::new(1.25, ACCENT_GREEN));

    let s = rect.width();
    let ox = rect.min.x;
    let oy = rect.min.y;
    let stroke = Stroke::new((s * 0.105).max(2.0), ACCENT_GREEN);

    let pts: Vec<Pos2> = BRAND_5_PTS
        .iter()
        .map(|&(nx, ny)| Pos2::new(ox + s * nx, oy + s * ny))
        .collect();
    painter.add(Shape::line(pts, stroke));
}

/// Lightweight modern geometric vector icons for sidebar navigation (2–4 primitives each).
fn draw_nav_icon(ui: &mut egui::Ui, tab: Tab, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let s = rect.width();
    let ox = rect.min.x;
    let oy = rect.min.y;
    let stroke = Stroke::new(1.6, color);

    match tab {
        Tab::Downloads => {
            painter.line_segment(
                [
                    Pos2::new(ox + s * 0.50, oy + s * 0.16),
                    Pos2::new(ox + s * 0.50, oy + s * 0.62),
                ],
                stroke,
            );
            painter.add(Shape::line(
                vec![
                    Pos2::new(ox + s * 0.31, oy + s * 0.44),
                    Pos2::new(ox + s * 0.50, oy + s * 0.64),
                    Pos2::new(ox + s * 0.69, oy + s * 0.44),
                ],
                stroke,
            ));
            painter.add(Shape::line(
                vec![
                    Pos2::new(ox + s * 0.18, oy + s * 0.68),
                    Pos2::new(ox + s * 0.18, oy + s * 0.84),
                    Pos2::new(ox + s * 0.82, oy + s * 0.84),
                    Pos2::new(ox + s * 0.82, oy + s * 0.68),
                ],
                stroke,
            ));
        }
        Tab::Terminal => {
            let box_rect = Rect::from_min_max(
                Pos2::new(ox + s * 0.12, oy + s * 0.18),
                Pos2::new(ox + s * 0.88, oy + s * 0.82),
            );
            painter.rect_stroke(box_rect, Rounding::same(3.0), stroke);
            painter.add(Shape::line(
                vec![
                    Pos2::new(ox + s * 0.28, oy + s * 0.38),
                    Pos2::new(ox + s * 0.44, oy + s * 0.50),
                    Pos2::new(ox + s * 0.28, oy + s * 0.62),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    Pos2::new(ox + s * 0.52, oy + s * 0.62),
                    Pos2::new(ox + s * 0.72, oy + s * 0.62),
                ],
                stroke,
            );
        }
        Tab::Completed => {
            let center = rect.center();
            painter.circle_stroke(center, s * 0.38, stroke);
            painter.add(Shape::line(
                vec![
                    Pos2::new(ox + s * 0.33, oy + s * 0.51),
                    Pos2::new(ox + s * 0.45, oy + s * 0.63),
                    Pos2::new(ox + s * 0.69, oy + s * 0.39),
                ],
                stroke,
            ));
        }
        Tab::About => {
            let center = rect.center();
            painter.circle_stroke(center, s * 0.38, stroke);
            painter.circle_filled(Pos2::new(ox + s * 0.50, oy + s * 0.34), s * 0.06, color);
            painter.line_segment(
                [
                    Pos2::new(ox + s * 0.50, oy + s * 0.47),
                    Pos2::new(ox + s * 0.50, oy + s * 0.69),
                ],
                stroke,
            );
        }
    }
}

/// Sleek 8px progress bar inside a single-line horizontal layout (never expands vertically).
fn draw_progress_row(ui: &mut egui::Ui, progress: f32) {
    let clamped = progress.clamp(0.0, 1.0);
    let pct_val = clamped * 100.0;
    let pct_label = if pct_val >= 99.95 {
        "100%".to_string()
    } else {
        format!("{:>5.1}%", pct_val)
    };
    ui.horizontal(|ui| {
        let pct_label_w = 50.0;
        let track_w = (ui.available_width() - pct_label_w - 10.0).max(40.0);
        let (track_rect, _) = ui.allocate_exact_size(Vec2::new(track_w, 8.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(track_rect, Rounding::same(4.0), BG_ELEVATED);
        painter.rect_stroke(track_rect, Rounding::same(4.0), Stroke::new(1.0, BORDER_SUBTLE));

        let fill_w = (track_rect.width() * clamped).max(if clamped > 0.0 { 6.0 } else { 0.0 });
        if fill_w > 0.0 {
            let fill_rect =
                Rect::from_min_size(track_rect.min, Vec2::new(fill_w, track_rect.height()));
            painter.rect_filled(fill_rect, Rounding::same(4.0), ACCENT_GREEN);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(pct_label)
                    .size(12.0)
                    .family(FontFamily::Monospace)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
        });
    });
}

fn styled_secondary_btn(
    label: &str,
    text_color: Color32,
    border: Color32,
    min_size: Vec2,
) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).size(12.5).strong().color(text_color))
        .fill(BG_ELEVATED)
        .stroke(Stroke::new(1.0, border))
        .rounding(Rounding::same(7.0))
        .min_size(min_size)
}

fn updater_temp_dir() -> PathBuf {
    env::temp_dir().join("5kd_updater")
}

/// Deletes any leftover downloaded installers, staging folders, or updater helper binaries.
fn cleanup_leftover_update_files() {
    let dir = updater_temp_dir();
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Retries cleanup briefly in the background so a just-exited Windows update helper
/// has time to release its executable image lock before its folder is removed.
fn cleanup_leftover_update_files_async() {
    let dir = updater_temp_dir();
    if !dir.exists() {
        return;
    }
    if fs::remove_dir_all(&dir).is_ok() {
        return;
    }
    thread::spawn(move || {
        for _ in 0..16 {
            thread::sleep(Duration::from_millis(250));
            if !dir.exists() || fs::remove_dir_all(&dir).is_ok() {
                break;
            }
        }
    });
}

fn parse_version_numbers(tag: &str) -> Vec<u32> {
    let trimmed = tag
        .trim()
        .trim_start_matches(['v', 'V'])
        .trim_end_matches(['v', 'V']);
    let mut nums = Vec::new();
    for part in trimmed.split(['.', '-', '_']) {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            if let Ok(n) = digits.parse::<u32>() {
                nums.push(n);
            }
        }
    }
    nums
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let a = parse_version_numbers(latest);
    let b = parse_version_numbers(current);
    if !a.is_empty() && !b.is_empty() {
        let len = a.len().max(b.len());
        for i in 0..len {
            let av = *a.get(i).unwrap_or(&0);
            let bv = *b.get(i).unwrap_or(&0);
            if av != bv {
                return av > bv;
            }
        }
        return false;
    }
    latest.trim().to_ascii_lowercase() != current.trim().to_ascii_lowercase()
}

fn is_allowed_github_download_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
        || url.starts_with("https://objects.githubusercontent.com/")
        || url.starts_with("https://release-assets.githubusercontent.com/")
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn check_github_latest_release(repo: &str) -> Result<Option<UpdateReleaseInfo>, String> {
    let clean_repo = repo.trim().trim_matches('/');
    if clean_repo.is_empty() || !clean_repo.contains('/') {
        return Err(
            "GitHub repository is not configured (set owner/repo in build or About tab)."
                .to_string(),
        );
    }

    let client = shared_http_client()?;
    let latest_url = format!("https://api.github.com/repos/{}/releases/latest", clean_repo);
    let ua = format!("5KD-Updater/{}", APP_VERSION);

    let resp = client
        .get(&latest_url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", &ua)
        .send()
        .map_err(|e| format!("Network error checking GitHub: {}", e))?;

    let release_val: Value = if resp.status().is_success() {
        resp.json()
            .map_err(|e| format!("Invalid GitHub release JSON: {}", e))?
    } else if resp.status().as_u16() == 404 {
        // Fallback if latest release is prerelease or tag-only list
        let list_url = format!(
            "https://api.github.com/repos/{}/releases?per_page=1",
            clean_repo
        );
        let list_resp = client
            .get(&list_url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", &ua)
            .send()
            .map_err(|e| format!("Network error checking GitHub releases: {}", e))?;
        if !list_resp.status().is_success() {
            return Err(format!("GitHub API returned HTTP {}", list_resp.status().as_u16()));
        }
        let arr: Value = list_resp
            .json()
            .map_err(|e| format!("Invalid GitHub releases list JSON: {}", e))?;
        arr.as_array()
            .and_then(|items| items.first())
            .cloned()
            .ok_or_else(|| "No GitHub releases found for this repository".to_string())?
    } else {
        return Err(format!("GitHub API returned HTTP {}", resp.status().as_u16()));
    };

    let tag_name = non_empty_str(release_val.get("tag_name"))
        .ok_or_else(|| "GitHub release missing tag_name".to_string())?
        .to_string();

    if !is_newer_version(&tag_name, APP_VERSION) {
        return Ok(None);
    }

    let assets = release_val
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| format!("Release {} has no downloadable assets", tag_name))?;

    // Match the exact same installer/package used for this platform
    let matched_asset = assets
        .iter()
        .find(|a| {
            non_empty_str(a.get("name"))
                .map(|n| n.eq_ignore_ascii_case(INSTALLER_ASSET_NAME))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            format!(
                "Release {} does not contain matching installer '{}'",
                tag_name, INSTALLER_ASSET_NAME
            )
        })?;

    let asset_name = non_empty_str(matched_asset.get("name"))
        .unwrap_or(INSTALLER_ASSET_NAME)
        .to_string();
    let download_url = non_empty_str(matched_asset.get("browser_download_url"))
        .ok_or_else(|| "Release asset missing download URL".to_string())?
        .to_string();
    if !is_allowed_github_download_url(&download_url) {
        return Err("Refused untrusted update download URL".to_string());
    }
    let asset_size = matched_asset
        .get("size")
        .and_then(|s| s.as_u64())
        .unwrap_or(0);
    let release_notes = non_empty_str(release_val.get("body"))
        .unwrap_or("")
        .lines()
        .take(6)
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Some(UpdateReleaseInfo {
        tag_name,
        asset_name,
        download_url,
        asset_size,
        release_notes,
    }))
}

#[cfg(target_os = "windows")]
const UPDATE_HELPER_FLAG: &str = "--5kd-apply-update";

#[cfg(target_os = "windows")]
fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        if !stripped.starts_with("UNC\\") {
            return PathBuf::from(stripped);
        }
    }
    path.to_path_buf()
}

#[cfg(target_os = "windows")]
fn wait_for_windows_pid_exit(pid: u32, timeout_ms: u32) {
    use std::ffi::c_void;

    const SYNCHRONIZE: u32 = 0x0010_0000;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dw_desired_access: u32,
            b_inherit_handle: i32,
            dw_process_id: u32,
        ) -> *mut c_void;
        fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
    }

    // SAFETY: Standard Win32 synchronization calls on a process handle checked for null
    // before use and always closed before returning.
    unsafe {
        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if !handle.is_null() {
            let _ = WaitForSingleObject(handle, timeout_ms);
            let _ = CloseHandle(handle);
        }
    }
}

/// Runs headless when this GUI-subsystem binary is re-invoked from the temp folder
/// as the Windows update helper. Avoids spawning cmd.exe/tasklist/findstr entirely,
/// preventing any console or Windows Terminal window from appearing or hanging.
#[cfg(target_os = "windows")]
fn maybe_run_windows_update_helper() -> bool {
    use std::ffi::OsString;

    let mut args = env::args_os();
    let _self_bin = args.next();
    let Some(first) = args.next() else {
        return false;
    };
    if first.to_str() != Some(UPDATE_HELPER_FLAG) {
        return false;
    }

    let mut parent_pid: Option<u32> = None;
    let mut installer_path: Option<PathBuf> = None;
    let mut install_dir: Option<PathBuf> = None;
    let mut exe_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--pid") => {
                if let Some(val) = args.next() {
                    parent_pid = val.to_string_lossy().parse::<u32>().ok();
                }
            }
            Some("--installer") => {
                if let Some(val) = args.next() {
                    installer_path = Some(PathBuf::from(val));
                }
            }
            Some("--install-dir") => {
                if let Some(val) = args.next() {
                    install_dir = Some(PathBuf::from(val));
                }
            }
            Some("--exe") => {
                if let Some(val) = args.next() {
                    exe_path = Some(PathBuf::from(val));
                }
            }
            _ => {}
        }
    }

    // 1. Wait cleanly for the parent 5KD.exe process to exit
    if let Some(pid) = parent_pid {
        wait_for_windows_pid_exit(pid, 20_000);
    }
    // Brief grace period so Windows finishes releasing the executable image lock
    thread::sleep(Duration::from_millis(350));

    // 2. Run the downloaded Inno Setup installer silently and wait for completion
    if let Some(ref installer) = installer_path {
        let mut cmd = Command::new(installer);
        cmd.args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/FORCECLOSEAPPLICATIONS",
        ]);
        if let Some(ref dir) = install_dir {
            let mut dir_arg = OsString::from("/DIR=");
            dir_arg.push(dir.as_os_str());
            cmd.arg(dir_arg);
        }
        cmd.current_dir(env::temp_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let _ = cmd.status();

        // Remove the downloaded setup executable right after installation completes
        for _ in 0..10 {
            if !installer.exists() || fs::remove_file(installer).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    // 3. Relaunch the updated 5KD.exe (retries briefly while filesystem/AV finalizes)
    let fallback_exe = install_dir.as_ref().map(|d| d.join("5KD.exe"));
    for _ in 0..16 {
        let candidate = exe_path
            .as_deref()
            .filter(|p| p.exists())
            .or_else(|| fallback_exe.as_deref().filter(|p| p.exists()));
        if let Some(target) = candidate {
            let mut launch = Command::new(target);
            if let Some(parent) = target.parent() {
                launch.current_dir(parent);
            }
            if launch
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
            {
                break;
            }
        }
        thread::sleep(Duration::from_millis(250));
    }

    true
}

#[cfg(target_os = "windows")]
fn spawn_platform_installer_and_restart(
    installer_path: &Path,
    updater_dir: &Path,
) -> Result<(), String> {
    let current_exe =
        env::current_exe().map_err(|e| format!("Could not resolve current executable: {}", e))?;
    let current_exe_clean = strip_windows_verbatim_prefix(&current_exe);
    let install_dir = current_exe_clean
        .parent()
        .ok_or_else(|| "Could not resolve installation directory".to_string())?;

    let install_dir_clean = {
        let s = install_dir.to_string_lossy();
        if s.len() > 3 {
            PathBuf::from(s.trim_end_matches('\\'))
        } else {
            install_dir.to_path_buf()
        }
    };

    // Copy this GUI-subsystem executable into the temp updater folder so it can run
    // completely invisibly without locking the installed 5KD.exe or spawning cmd.exe.
    let helper_exe = updater_dir.join("5KD-updater-helper.exe");
    fs::copy(&current_exe_clean, &helper_exe)
        .map_err(|e| format!("Failed to prepare Windows update helper: {}", e))?;

    let pid_str = std::process::id().to_string();

    Command::new(&helper_exe)
        .arg(UPDATE_HELPER_FLAG)
        .arg("--pid")
        .arg(&pid_str)
        .arg("--installer")
        .arg(installer_path)
        .arg("--install-dir")
        .arg(&install_dir_clean)
        .arg("--exe")
        .arg(&current_exe_clean)
        .current_dir(env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch update helper: {}", e))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(not(target_os = "windows"))]
fn spawn_platform_installer_and_restart(
    archive_path: &Path,
    updater_dir: &Path,
) -> Result<(), String> {
    let current_exe =
        env::current_exe().map_err(|e| format!("Could not resolve current executable: {}", e))?;
    let extract_dir = updater_dir.join("extract");
    fs::create_dir_all(&extract_dir)
        .map_err(|e| format!("Failed to create extract directory: {}", e))?;

    let status = Command::new("tar")
        .args([
            "-xzf",
            &archive_path.display().to_string(),
            "-C",
            &extract_dir.display().to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to extract update archive: {}", e))?;

    if !status.success() {
        return Err("Failed to extract downloaded update archive".to_string());
    }

    // Locate extracted 5KD binary
    let mut extracted_bin: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&extract_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("5KD") {
                        extracted_bin = Some(path);
                        break;
                    }
                }
            }
        }
    }
    let new_bin = extracted_bin
        .ok_or_else(|| "Extracted update archive did not contain the 5KD binary".to_string())?;

    let script_path = updater_dir.join("apply_update.sh");
    let pid = std::process::id();
    let exe_q = shell_quote(&current_exe.display().to_string());
    let new_bin_q = shell_quote(&new_bin.display().to_string());
    let updater_dir_q = shell_quote(&updater_dir.display().to_string());

    let script_content = format!(
        "#!/bin/sh\n\
         PID={pid}\n\
         i=0\n\
         while kill -0 \"$PID\" 2>/dev/null && [ \"$i\" -lt 15 ]; do\n\
           sleep 1\n\
           i=$((i + 1))\n\
         done\n\
         cp -f {new_bin} {exe}.new && chmod +x {exe}.new && mv -f {exe}.new {exe}\n\
         rm -rf {updater_dir}\n\
         nohup {exe} >/dev/null 2>&1 &\n",
        pid = pid,
        new_bin = new_bin_q,
        exe = exe_q,
        updater_dir = updater_dir_q,
    );

    fs::write(&script_path, script_content)
        .map_err(|e| format!("Failed to write Unix update script: {}", e))?;

    Command::new("sh")
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch update helper: {}", e))?;

    Ok(())
}

/// Streams the matching GitHub installer directly inside the app, runs it silently,
/// removes all temporary/unused installer files, and restarts the app automatically.
fn download_and_install_update(
    info: &UpdateReleaseInfo,
    tx: &Sender<WorkerEvent>,
    notifier: &UiNotifier,
) -> Result<(), String> {
    if !is_allowed_github_download_url(&info.download_url) {
        return Err("Refused untrusted download URL".to_string());
    }

    cleanup_leftover_update_files();
    let updater_dir = updater_temp_dir();
    fs::create_dir_all(&updater_dir)
        .map_err(|e| format!("Failed to create updater folder: {}", e))?;

    let safe_asset = sanitize_filename(&info.asset_name, INSTALLER_ASSET_NAME);
    let part_path = updater_dir.join(format!("{}.part", safe_asset));
    let final_path = updater_dir.join(&safe_asset);

    let dl_client = reqwest::blocking::Client::builder()
        .user_agent(format!("5KD-Updater/{}", APP_VERSION))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to initialize download client: {}", e))?;

    let mut resp = dl_client
        .get(&info.download_url)
        .header("Accept", "application/octet-stream")
        .send()
        .map_err(|e| format!("Failed to download update: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Update download failed with HTTP {}",
            resp.status().as_u16()
        ));
    }

    let total_bytes = resp.content_length().unwrap_or(info.asset_size);
    let mut out_file = fs::File::create(&part_path)
        .map_err(|e| format!("Failed to create temporary installer file: {}", e))?;

    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);

    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("Interrupted while downloading update: {}", e))?;
        if n == 0 {
            break;
        }
        out_file
            .write_all(&buf[..n])
            .map_err(|e| format!("Failed writing update file: {}", e))?;
        downloaded += n as u64;

        if last_emit.elapsed() >= Duration::from_millis(80)
            || (total_bytes > 0 && downloaded >= total_bytes)
        {
            last_emit = Instant::now();
            let _ = tx.send(WorkerEvent::UpdateProgress {
                tag_name: info.tag_name.clone(),
                asset_name: info.asset_name.clone(),
                downloaded,
                total: total_bytes,
            });
            notifier.wake_throttled();
        }
    }

    out_file
        .flush()
        .map_err(|e| format!("Failed flushing installer file: {}", e))?;
    drop(out_file);

    fs::rename(&part_path, &final_path)
        .map_err(|e| format!("Failed finalizing installer file: {}", e))?;

    let _ = tx.send(WorkerEvent::Log(format!(
        "[updater] Downloaded {} ({}), installing silently and restarting...",
        info.asset_name,
        format_bytes(downloaded)
    )));
    notifier.wake_immediate();

    spawn_platform_installer_and_restart(&final_path, &updater_dir)?;
    Ok(())
}

struct FiveKdApp {
    current_tab: Tab,
    zoom_percent: u32,         // 100 to 150
    applied_zoom_percent: u32, // Avoids redundant set_zoom_factor calls every frame
    url_input: String,
    start_time: String,
    end_time: String,
    resolution: Resolution,
    container_ext: ContainerExt,
    output_path: String,
    paste_notice: Option<(String, Instant)>,

    check_updates_on_launch: bool,
    github_repo: String,
    update_state: UpdateState,
    update_banner_dismissed: bool,
    should_exit_for_update: bool,

    active_downloads: Vec<DownloadItem>,
    completed_downloads: Vec<DownloadItem>,
    terminal_logs: VecDeque<String>,
    next_id: usize,

    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
    notifier: UiNotifier,
}

impl FiveKdApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_CANVAS;
        visuals.window_fill = BG_SURFACE;
        visuals.window_stroke = Stroke::new(1.0, BORDER_HOVER);
        visuals.window_rounding = Rounding::same(8.0);
        visuals.extreme_bg_color = BG_ELEVATED;
        visuals.faint_bg_color = BG_SURFACE;
        visuals.selection.bg_fill = ACCENT_TINT;
        visuals.selection.stroke = Stroke::new(1.25, ACCENT_GREEN);
        visuals.hyperlink_color = ACCENT_GREEN;
        visuals.override_text_color = Some(TEXT_PRIMARY);

        // Calm, modern interactive widget states
        visuals.widgets.noninteractive.bg_fill = BG_SURFACE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
        visuals.widgets.noninteractive.rounding = Rounding::same(6.0);

        visuals.widgets.inactive.bg_fill = BG_ELEVATED;
        visuals.widgets.inactive.weak_bg_fill = BG_ELEVATED;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.inactive.rounding = Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = BG_ELEVATED;
        visuals.widgets.hovered.weak_bg_fill = BG_ELEVATED;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_GREEN);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.hovered.rounding = Rounding::same(6.0);

        visuals.widgets.active.bg_fill = ACCENT_HOVER_BG;
        visuals.widgets.active.weak_bg_fill = ACCENT_HOVER_BG;
        visuals.widgets.active.bg_stroke = Stroke::new(1.25, ACCENT_GREEN);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT_GREEN);
        visuals.widgets.active.rounding = Rounding::same(6.0);

        visuals.widgets.open.bg_fill = BG_ELEVATED;
        visuals.widgets.open.weak_bg_fill = BG_ELEVATED;
        visuals.widgets.open.bg_stroke = Stroke::new(1.25, ACCENT_GREEN);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, ACCENT_GREEN);
        visuals.widgets.open.rounding = Rounding::same(6.0);

        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(12.0, 6.0);
            style.spacing.scroll.bar_width = 6.0;
        });

        // Remove any leftover installer or temporary update files from a prior update
        cleanup_leftover_update_files_async();

        let cfg = load_app_config();
        let (tx, rx) = channel();
        let notifier = UiNotifier::new(cc.egui_ctx.clone());

        let mut logs = VecDeque::with_capacity(64);
        logs.push_back(format!(
            "[info] 5KD - 5wi's Kick Downloader ({}) initialized.",
            APP_VERSION
        ));
        logs.push_back("[info] Integrated Kick VOD Extractor ready.".to_string());
        if !cfg.output_path.is_empty() {
            logs.push_back(format!(
                "[config] Restored output destination: {}",
                cfg.output_path
            ));
        }

        let effective_repo = if !cfg.github_repo.trim().is_empty() {
            cfg.github_repo.trim().to_string()
        } else {
            GITHUB_REPO.to_string()
        };

        let mut app = Self {
            current_tab: Tab::Downloads,
            zoom_percent: cfg.zoom_percent,
            applied_zoom_percent: 0, // Applies saved zoom_percent on the very first frame
            url_input: String::new(),
            start_time: String::new(),
            end_time: String::new(),
            resolution: Resolution::from_short_label(&cfg.resolution),
            container_ext: ContainerExt::from_str(&cfg.container_ext),
            output_path: cfg.output_path,
            paste_notice: None,
            check_updates_on_launch: cfg.check_updates_on_launch,
            github_repo: effective_repo,
            update_state: UpdateState::Idle,
            update_banner_dismissed: false,
            should_exit_for_update: false,
            active_downloads: Vec::new(),
            completed_downloads: Vec::new(),
            terminal_logs: logs,
            next_id: 1,
            tx,
            rx,
            notifier,
        };

        if app.check_updates_on_launch && !app.github_repo.trim().is_empty() {
            app.check_for_updates();
        }

        app
    }

    fn persist_config(&self) {
        save_app_config(&AppConfig {
            output_path: self.output_path.clone(),
            zoom_percent: self.zoom_percent,
            resolution: self.resolution.short_label().to_string(),
            container_ext: self.container_ext.as_str().to_string(),
            check_updates_on_launch: self.check_updates_on_launch,
            github_repo: if self.github_repo.trim() == GITHUB_REPO {
                String::new()
            } else {
                self.github_repo.trim().to_string()
            },
        });
    }

    fn check_for_updates(&mut self) {
        if matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing(_)
        ) {
            return;
        }
        let repo = self.github_repo.trim().to_string();
        if repo.is_empty() {
            self.update_state = UpdateState::Error(
                "GitHub repository not configured yet. Enter owner/repo below.".to_string(),
            );
            return;
        }
        self.update_state = UpdateState::Checking;
        self.update_banner_dismissed = false;
        self.push_log(format!(
            "[updater] Checking GitHub ({}) for newer releases...",
            repo
        ));

        let tx = self.tx.clone();
        let notifier = self.notifier.clone();
        thread::spawn(move || {
            let res = check_github_latest_release(&repo);
            match &res {
                Ok(Some(info)) => {
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "[updater] Update available: {} ({})",
                        info.tag_name, info.asset_name
                    )));
                }
                Ok(None) => {
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "[updater] 5KD is up to date ({}).",
                        APP_VERSION
                    )));
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::Log(format!("[updater error] {}", e)));
                }
            }
            let _ = tx.send(WorkerEvent::UpdateChecked(res));
            notifier.wake_immediate();
        });
    }

    fn start_in_app_update(&mut self, info: UpdateReleaseInfo) {
        if matches!(
            self.update_state,
            UpdateState::Downloading { .. } | UpdateState::Installing(_)
        ) {
            return;
        }
        self.persist_config();
        self.update_state = UpdateState::Downloading {
            tag_name: info.tag_name.clone(),
            asset_name: info.asset_name.clone(),
            downloaded: 0,
            total: info.asset_size,
        };
        self.push_log(format!(
            "[updater] Downloading {} ({}) directly inside 5KD...",
            info.asset_name, info.tag_name
        ));

        let tx = self.tx.clone();
        let notifier = self.notifier.clone();
        thread::spawn(move || match download_and_install_update(&info, &tx, &notifier) {
            Ok(()) => {
                let _ = tx.send(WorkerEvent::UpdateReadyToRestart(info.tag_name));
                notifier.wake_immediate();
            }
            Err(e) => {
                cleanup_leftover_update_files();
                let _ = tx.send(WorkerEvent::Log(format!("[updater error] {}", e)));
                let _ = tx.send(WorkerEvent::UpdateFailed(e));
                notifier.wake_immediate();
            }
        });
    }

    fn push_log(&mut self, line: String) {
        if self.terminal_logs.len() >= MAX_TERMINAL_LOGS {
            self.terminal_logs.pop_front();
        }
        self.terminal_logs.push_back(line);
    }

    fn has_chosen_folder(&self) -> bool {
        !self.output_path.trim().is_empty()
    }

    fn cancel_download(&mut self, id: usize) {
        if let Some(pos) = self.active_downloads.iter().position(|i| i.id == id) {
            let mut item = self.active_downloads.remove(pos);
            item.cancel_flag.store(true, Ordering::SeqCst);

            if let Ok(guard) = item.child_pid.lock() {
                if let Some(pid) = *guard {
                    let pid_str = pid.to_string();
                    #[cfg(target_os = "windows")]
                    {
                        let _ = Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid_str])
                            .creation_flags(CREATE_NO_WINDOW)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let _ = Command::new("kill")
                            .args(["-9", &pid_str])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                    }
                }
            }

            self.push_log(format!(
                "[cancel] Download #{} canceled by user: {}",
                id, item.title
            ));
            item.status = DownloadStatus::Canceled;
            item.status_text = "Canceled by user".to_string();
            self.completed_downloads.insert(0, item);
        }
    }

    fn start_download(&mut self) {
        let raw_url = self.url_input.trim().to_string();
        if raw_url.is_empty() || !self.has_chosen_folder() {
            return;
        }

        let id = self.next_id;
        self.next_id += 1;

        let start_t = self.start_time.trim().to_string();
        let end_t = self.end_time.trim().to_string();
        let res = self.resolution;
        let ext = self.container_ext;
        let custom_out = self.output_path.trim().to_string();

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(Mutex::new(None));

        let item = DownloadItem {
            id,
            title: format!("Extracting VOD: {}", raw_url),
            url: raw_url.clone(),
            output_path: custom_out.clone(),
            saved_file_path: None,
            resolution: res.short_label(),
            format: ext.upper(),
            progress: 0.0,
            status_text: "Extracting Master M3U8 URL from Kick...".to_string(),
            status: DownloadStatus::Active,
            cancel_flag: cancel_flag.clone(),
            child_pid: child_pid.clone(),
        };

        self.active_downloads.insert(0, item);
        self.url_input.clear();

        let tx = self.tx.clone();
        let notifier = self.notifier.clone();
        thread::spawn(move || {
            let _ = tx.send(WorkerEvent::Log(format!(
                "[extractor] Analyzing URL: {}",
                raw_url
            )));
            notifier.wake_immediate();

            let extracted = match extract_kick_vod(&raw_url, &tx) {
                Ok(meta) => {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "[extractor] Extracted '{}' ({}): {}",
                        meta.session_title, meta.source_type, meta.m3u8_url
                    )));
                    let _ = tx.send(WorkerEvent::Extracted(id, meta.clone()));
                    notifier.wake_immediate();
                    meta
                }
                Err(e) => {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = tx.send(WorkerEvent::Log(format!("[extractor error] {}", e)));
                    let _ = tx.send(WorkerEvent::Finished(id, false, e, None));
                    notifier.wake_immediate();
                    return;
                }
            };

            if cancel_flag.load(Ordering::SeqCst) {
                return;
            }

            let progress_target = probe_stream_for_progress(
                &extracted.m3u8_url,
                res,
                extracted.duration_secs,
                &start_t,
                &end_t,
                &tx,
            );
            if cancel_flag.load(Ordering::SeqCst) {
                return;
            }

            // Pull the VOD/Clip title and make it the name of the output file
            let final_output = build_output_filepath(
                &custom_out,
                &extracted.session_title,
                &extracted.channel_slug,
                ext.as_str(),
            );

            let mut cmd = Command::new("yt-dlp");
            cmd.args([
                "-f",
                res.ytdlp_format(),
                "-N",
                "8",
                "--user-agent",
                CHROME_UA,
                "--add-header",
                "Origin:https://kick.com",
                "--add-header",
                "Referer:https://kick.com/",
                "--downloader",
                "ffmpeg",
            ]);

            if !start_t.is_empty() || !end_t.is_empty() {
                let mut ff_args = String::with_capacity(40);
                ff_args.push_str("ffmpeg_i:");
                if !start_t.is_empty() {
                    ff_args.push_str("-ss ");
                    ff_args.push_str(&start_t);
                }
                if !end_t.is_empty() {
                    if !start_t.is_empty() {
                        ff_args.push(' ');
                    }
                    ff_args.push_str("-to ");
                    ff_args.push_str(&end_t);
                }
                cmd.arg("--downloader-args").arg(ff_args);
            }

            cmd.arg("-o")
                .arg(&final_output)
                .arg(&extracted.m3u8_url)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            #[cfg(target_os = "windows")]
            cmd.creation_flags(CREATE_NO_WINDOW);

            let _ = tx.send(WorkerEvent::Log(format!(
                ">>> Saving VOD/Clip as -> {}",
                final_output
            )));
            notifier.wake_immediate();

            match cmd.spawn() {
                Ok(mut child) => {
                    if let Ok(mut guard) = child_pid.lock() {
                        *guard = Some(child.id());
                    }
                    if cancel_flag.load(Ordering::SeqCst) {
                        let _ = child.kill();
                        return;
                    }

                    if let Some(stderr) = child.stderr.take() {
                        let tx_err = tx.clone();
                        let cancel_err = cancel_flag.clone();
                        let notifier_err = notifier.clone();
                        let mut target_err = progress_target;
                        thread::spawn(move || {
                            let mut last_prog = Instant::now()
                                .checked_sub(Duration::from_secs(1))
                                .unwrap_or_else(Instant::now);
                            let mut last_prog_log = Instant::now()
                                .checked_sub(Duration::from_secs(1))
                                .unwrap_or_else(Instant::now);

                            read_cr_lf_lines(stderr, |line| {
                                if cancel_err.load(Ordering::SeqCst) {
                                    return false;
                                }

                                if target_err.duration_secs.is_none() {
                                    if let Some(dur) = parse_ffmpeg_header_duration(&line) {
                                        target_err.update_from_stream_duration(dur);
                                    }
                                }
                                if target_err.fps.is_none() {
                                    if let Some(fps) = parse_ffmpeg_header_fps(&line) {
                                        target_err.update_fps(fps);
                                    }
                                }

                                let is_progress_line =
                                    line.contains("frame=") || line.contains("time=");
                                if is_progress_line {
                                    if last_prog.elapsed() >= Duration::from_millis(80) {
                                        last_prog = Instant::now();
                                        let (ratio_opt, status_text) =
                                            compute_ffmpeg_progress(&line, &target_err);
                                        let prog_val = ratio_opt.unwrap_or(-1.0);
                                        let _ = tx_err.send(WorkerEvent::Progress(
                                            id,
                                            prog_val,
                                            status_text,
                                        ));
                                    }
                                    if last_prog_log.elapsed() >= Duration::from_millis(500) {
                                        last_prog_log = Instant::now();
                                        let _ = tx_err.send(WorkerEvent::Log(line));
                                    }
                                } else {
                                    let _ = tx_err.send(WorkerEvent::Log(line));
                                }
                                notifier_err.wake_throttled();
                                true
                            });
                        });
                    }

                    if let Some(stdout) = child.stdout.take() {
                        read_cr_lf_lines(stdout, |line| {
                            if cancel_flag.load(Ordering::SeqCst) {
                                return false;
                            }
                            if let Some(pct_idx) = line.find('%') {
                                let before = &line[..pct_idx];
                                if let Some(num_str) = before.split_whitespace().last() {
                                    if let Ok(pct) = num_str.parse::<f32>() {
                                        let _ = tx.send(WorkerEvent::Progress(
                                            id,
                                            (pct / 100.0).clamp(0.0, 0.999),
                                            line.clone(),
                                        ));
                                    }
                                }
                            }
                            let _ = tx.send(WorkerEvent::Log(line));
                            notifier.wake_throttled();
                            true
                        });
                    }

                    let status = child.wait();
                    if cancel_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    match status {
                        Ok(s) if s.success() => {
                            let _ = tx.send(WorkerEvent::Finished(
                                id,
                                true,
                                format!("Saved to {}", final_output),
                                Some(final_output),
                            ));
                        }
                        Ok(s) => {
                            let _ = tx.send(WorkerEvent::Finished(
                                id,
                                false,
                                format!("yt-dlp exited with code {:?}", s.code()),
                                None,
                            ));
                        }
                        Err(e) => {
                            let _ = tx.send(WorkerEvent::Finished(id, false, e.to_string(), None));
                        }
                    }
                    notifier.wake_immediate();
                }
                Err(e) => {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "[error] Failed to spawn yt-dlp: {} (Ensure yt-dlp & ffmpeg are in PATH)",
                        e
                    )));
                    let _ = tx.send(WorkerEvent::Finished(
                        id,
                        false,
                        "yt-dlp or ffmpeg not found in PATH".to_string(),
                        None,
                    ));
                    notifier.wake_immediate();
                }
            }
        });
    }

    fn poll_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                WorkerEvent::Log(line) => {
                    self.push_log(line);
                }
                WorkerEvent::Extracted(id, meta) => {
                    if let Some(item) = self.active_downloads.iter_mut().find(|i| i.id == id) {
                        item.title = format!(
                            "{} ({})",
                            meta.session_title, meta.duration_formatted
                        );
                        item.url = meta.m3u8_url;
                        item.status_text = "Downloading stream via yt-dlp + ffmpeg...".to_string();
                    }
                }
                WorkerEvent::Progress(id, prog, status) => {
                    if let Some(item) = self.active_downloads.iter_mut().find(|i| i.id == id) {
                        if prog >= 0.0 && prog > item.progress {
                            item.progress = prog;
                        }
                        item.status_text = status;
                    }
                }
                WorkerEvent::Finished(id, success, msg, saved_path) => {
                    if let Some(pos) = self.active_downloads.iter().position(|i| i.id == id) {
                        let mut item = self.active_downloads.remove(pos);
                        item.progress = if success { 1.0 } else { item.progress };
                        item.status = if success {
                            DownloadStatus::Completed
                        } else {
                            DownloadStatus::Failed
                        };
                        item.saved_file_path = saved_path;
                        item.status_text = msg;
                        self.completed_downloads.insert(0, item);
                    }
                }
                WorkerEvent::UpdateChecked(result) => match result {
                    Ok(Some(info)) => {
                        self.update_state = UpdateState::Available(info);
                    }
                    Ok(None) => {
                        self.update_state = UpdateState::UpToDate(APP_VERSION.to_string());
                    }
                    Err(e) => {
                        self.update_state = UpdateState::Error(e);
                    }
                },
                WorkerEvent::UpdateProgress {
                    tag_name,
                    asset_name,
                    downloaded,
                    total,
                } => {
                    self.update_state = UpdateState::Downloading {
                        tag_name,
                        asset_name,
                        downloaded,
                        total,
                    };
                }
                WorkerEvent::UpdateReadyToRestart(tag_name) => {
                    self.update_state = UpdateState::Installing(tag_name);
                    self.persist_config();
                    self.should_exit_for_update = true;
                }
                WorkerEvent::UpdateFailed(err) => {
                    self.update_state = UpdateState::Error(err);
                }
            }
        }
    }

    fn render_history_card(&self, ui: &mut egui::Ui, item: &DownloadItem) {
        egui::Frame::none()
            .fill(BG_SURFACE)
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .inner_margin(egui::Margin::symmetric(16.0, 12.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let show_open_btn =
                        !item.output_path.is_empty() && item.status == DownloadStatus::Completed;
                    let open_btn_w = if show_open_btn { 96.0 + 12.0 } else { 0.0 };
                    let info_w = (ui.available_width() - open_btn_w).max(80.0);

                    ui.allocate_ui_with_layout(
                        Vec2::new(info_w, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_max_width(info_w);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&item.title)
                                        .size(13.5)
                                        .strong()
                                        .color(TEXT_PRIMARY),
                                )
                                .truncate(),
                            );
                            ui.add_space(3.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(
                                    RichText::new(item.status.label())
                                        .size(12.0)
                                        .strong()
                                        .color(item.status.color()),
                                );
                                ui.label(RichText::new("·").size(12.0).color(TEXT_MUTED));
                                ui.label(
                                    RichText::new(format!("{} · {}", item.resolution, item.format))
                                        .size(11.5)
                                        .family(FontFamily::Monospace)
                                        .color(TEXT_MUTED),
                                );
                                ui.label(RichText::new("·").size(12.0).color(TEXT_MUTED));
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&item.status_text)
                                            .size(11.5)
                                            .family(FontFamily::Monospace)
                                            .color(TEXT_MUTED),
                                    )
                                    .truncate(),
                                );
                            });
                        },
                    );

                    if show_open_btn {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let open_btn = styled_secondary_btn(
                                "Open Folder",
                                TEXT_PRIMARY,
                                BORDER_HOVER,
                                Vec2::new(96.0, 30.0),
                            );
                            if ui.add(open_btn).clicked() {
                                open_output_folder(
                                    &item.output_path,
                                    item.saved_file_path.as_deref(),
                                );
                            }
                        });
                    }
                });
            });
    }
}

impl eframe::App for FiveKdApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist_config();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events();

        if self.should_exit_for_update {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Only mutate zoom factor when user changes zoom (zero overhead on steady frames)
        if self.zoom_percent != self.applied_zoom_percent {
            ctx.set_zoom_factor(self.zoom_percent as f32 / 100.0);
            self.applied_zoom_percent = self.zoom_percent;
        }

        // Expire transient paste feedback cleanly without continuous polling
        if let Some((_, created_at)) = &self.paste_notice {
            if created_at.elapsed() >= Duration::from_secs(2) {
                self.paste_notice = None;
            }
        }

        // Left Navigation Rail (#000000 with subtle hairline divider & footer zoom controls)
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(220.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_CANVAS)
                    .inner_margin(egui::Margin::symmetric(14.0, 18.0))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                // Header Brand Block (single source of branding — no redundant top bar)
                ui.horizontal(|ui| {
                    draw_brand_mark(ui, 32.0);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(
                            RichText::new("5KD")
                                .size(17.5)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new("Kick Downloader")
                                .size(11.0)
                                .color(TEXT_MUTED),
                        );
                    });
                });

                ui.add_space(22.0);

                // Primary Navigation Tabs
                let has_update_badge = usize::from(matches!(
                    self.update_state,
                    UpdateState::Available(_) | UpdateState::Downloading { .. }
                ));
                let nav_items = [
                    (Tab::Downloads, "Downloads", self.active_downloads.len()),
                    (Tab::Terminal, "Terminal", self.terminal_logs.len()),
                    (Tab::Completed, "Completed", self.completed_downloads.len()),
                    (Tab::About, "About", has_update_badge),
                ];

                for (tab, label, count) in nav_items {
                    let is_selected = self.current_tab == tab;
                    let fg = if is_selected { ACCENT_GREEN } else { TEXT_MUTED };
                    let bg_fill = if is_selected {
                        ACCENT_TINT
                    } else {
                        Color32::TRANSPARENT
                    };

                    let btn_res = egui::Frame::none()
                        .fill(bg_fill)
                        .rounding(Rounding {
                            nw: 0.0,
                            sw: 0.0,
                            ne: 7.0,
                            se: 7.0,
                        })
                        .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                draw_nav_icon(ui, tab, 17.0, fg);
                                ui.add_space(4.0);
                                let text = RichText::new(label).size(13.5).color(fg);
                                ui.label(if is_selected { text.strong() } else { text });

                                if count > 0 {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(count.to_string())
                                                    .size(11.0)
                                                    .family(FontFamily::Monospace)
                                                    .color(if is_selected {
                                                        ACCENT_GREEN
                                                    } else {
                                                        TEXT_MUTED
                                                    }),
                                            );
                                        },
                                    );
                                }
                            });
                        });

                    let interact = btn_res.response.interact(Sense::click());
                    if interact.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }

                    if is_selected {
                        ui.painter().vline(
                            btn_res.response.rect.left(),
                            btn_res.response.rect.y_range(),
                            Stroke::new(2.5, ACCENT_GREEN),
                        );
                    } else if interact.hovered() {
                        ui.painter().vline(
                            btn_res.response.rect.left(),
                            btn_res.response.rect.y_range(),
                            Stroke::new(2.0, BORDER_HOVER),
                        );
                    }

                    if interact.clicked() {
                        self.current_tab = tab;
                    }
                    ui.add_space(2.0);
                }

                // Sidebar Footer: Compact Zoom Stepper pinned to bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .rounding(Rounding::same(8.0))
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Zoom").size(12.0).color(TEXT_MUTED));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let zoom_in = egui::Button::new(
                                            RichText::new("+")
                                                .size(13.0)
                                                .family(FontFamily::Monospace)
                                                .strong()
                                                .color(if self.zoom_percent < 150 {
                                                    TEXT_PRIMARY
                                                } else {
                                                    TEXT_MUTED
                                                }),
                                        )
                                        .fill(BG_ELEVATED)
                                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                        .rounding(Rounding::same(5.0))
                                        .min_size(Vec2::new(24.0, 22.0));

                                        if ui.add_enabled(self.zoom_percent < 150, zoom_in).clicked()
                                        {
                                            self.zoom_percent = (self.zoom_percent + 10).min(150);
                                            self.persist_config();
                                        }

                                        ui.label(
                                            RichText::new(format!("{}%", self.zoom_percent))
                                                .size(11.5)
                                                .family(FontFamily::Monospace)
                                                .color(ACCENT_GREEN),
                                        );

                                        let zoom_out = egui::Button::new(
                                            RichText::new("-")
                                                .size(13.0)
                                                .family(FontFamily::Monospace)
                                                .strong()
                                                .color(if self.zoom_percent > 100 {
                                                    TEXT_PRIMARY
                                                } else {
                                                    TEXT_MUTED
                                                }),
                                        )
                                        .fill(BG_ELEVATED)
                                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                        .rounding(Rounding::same(5.0))
                                        .min_size(Vec2::new(24.0, 22.0));

                                        if ui
                                            .add_enabled(self.zoom_percent > 100, zoom_out)
                                            .clicked()
                                        {
                                            self.zoom_percent =
                                                self.zoom_percent.saturating_sub(10).max(100);
                                            self.persist_config();
                                        }
                                    },
                                );
                            });
                        });
                });
            });

        // Main Content Canvas (#000000)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG_CANVAS)
                    .inner_margin(egui::Margin::symmetric(28.0, 24.0)),
            )
            .show(ctx, |ui| {
                // Compact in-app update banner when a newer release is found outside the About tab
                if self.current_tab != Tab::About && !self.update_banner_dismissed {
                    if let UpdateState::Available(info) = self.update_state.clone() {
                        let mut trigger_install = false;
                        egui::Frame::none()
                            .fill(ACCENT_TINT)
                            .rounding(Rounding::same(10.0))
                            .stroke(Stroke::new(1.0, ACCENT_GREEN))
                            .inner_margin(egui::Margin::symmetric(14.0, 9.0))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Update {} available",
                                            info.tag_name
                                        ))
                                        .size(13.0)
                                        .strong()
                                        .color(ACCENT_GREEN),
                                    );
                                    ui.label(
                                        RichText::new(format!("· {}", info.asset_name))
                                            .size(11.5)
                                            .family(FontFamily::Monospace)
                                            .color(TEXT_MUTED),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let later_btn = styled_secondary_btn(
                                                "Later",
                                                TEXT_MUTED,
                                                BORDER_HOVER,
                                                Vec2::new(56.0, 28.0),
                                            );
                                            if ui.add(later_btn).clicked() {
                                                self.update_banner_dismissed = true;
                                            }
                                            let upd_btn = egui::Button::new(
                                                RichText::new("Update & Restart")
                                                    .size(12.0)
                                                    .strong()
                                                    .color(Color32::BLACK),
                                            )
                                            .fill(ACCENT_GREEN)
                                            .stroke(Stroke::new(1.0, ACCENT_GREEN))
                                            .rounding(Rounding::same(7.0))
                                            .min_size(Vec2::new(124.0, 28.0));
                                            if ui.add(upd_btn).clicked() {
                                                trigger_install = true;
                                            }
                                        },
                                    );
                                });
                            });
                        if trigger_install {
                            self.current_tab = Tab::About;
                            self.start_in_app_update(info);
                        }
                        ui.add_space(12.0);
                    }
                }

                match self.current_tab {
                Tab::Downloads => {
                    let folder_selected = self.has_chosen_folder();
                    let can_download = folder_selected && !self.url_input.trim().is_empty();

                    // 1. Primary URL Bar [Input | Paste | Download] inside single-row horizontal layout
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let paste_w = 72.0;
                                let dl_w = 104.0;
                                let spacing = ui.spacing().item_spacing.x;
                                let input_w =
                                    (ui.available_width() - paste_w - dl_w - spacing * 2.0)
                                        .max(100.0);

                                let text_edit = egui::TextEdit::singleline(&mut self.url_input)
                                    .hint_text("Paste Kick VOD or clip URL here...")
                                    .frame(false)
                                    .font(FontId::proportional(14.5))
                                    .desired_width(input_w)
                                    .min_size(Vec2::new(input_w, 34.0))
                                    .vertical_align(egui::Align::Center);

                                let resp = ui.add(text_edit);
                                if resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    && can_download
                                {
                                    self.start_download();
                                }

                                // One-Click Paste button
                                let paste_btn = styled_secondary_btn(
                                    "Paste",
                                    TEXT_PRIMARY,
                                    BORDER_HOVER,
                                    Vec2::new(paste_w, 34.0),
                                );
                                if ui
                                    .add(paste_btn)
                                    .on_hover_text("Paste URL from clipboard")
                                    .clicked()
                                {
                                    if let Some(clip) = read_clipboard_text() {
                                        self.url_input = clip;
                                        self.paste_notice = None;
                                    } else {
                                        self.paste_notice =
                                            Some(("Clipboard is empty".to_string(), Instant::now()));
                                        ctx.request_repaint_after(Duration::from_secs(2));
                                    }
                                }

                                // Primary CTA: Download
                                let dl_clicked = ui
                                    .scope(|ui| {
                                        if can_download {
                                            let w = &mut ui.visuals_mut().widgets;
                                            w.hovered.bg_fill = Color32::from_rgb(38, 255, 39);
                                            w.hovered.weak_bg_fill = Color32::from_rgb(38, 255, 39);
                                            w.hovered.bg_stroke =
                                                Stroke::new(1.0, Color32::from_rgb(38, 255, 39));
                                            w.active.bg_fill = Color32::from_rgb(0, 200, 1);
                                            w.active.weak_bg_fill = Color32::from_rgb(0, 200, 1);
                                            w.active.bg_stroke =
                                                Stroke::new(1.0, Color32::from_rgb(0, 200, 1));
                                        }
                                        let dl_btn = egui::Button::new(
                                            RichText::new("Download")
                                                .size(13.5)
                                                .strong()
                                                .color(if can_download {
                                                    Color32::BLACK
                                                } else {
                                                    TEXT_MUTED
                                                }),
                                        )
                                        .fill(if can_download {
                                            ACCENT_GREEN
                                        } else {
                                            BG_ELEVATED
                                        })
                                        .stroke(Stroke::new(
                                            1.0,
                                            if can_download {
                                                ACCENT_GREEN
                                            } else {
                                                BORDER_SUBTLE
                                            },
                                        ))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(dl_w, 34.0));

                                        ui.add_enabled(can_download, dl_btn)
                                            .on_disabled_hover_text(if !folder_selected {
                                                "Choose an output folder below before downloading"
                                            } else {
                                                "Paste a Kick VOD or clip URL to download"
                                            })
                                            .clicked()
                                    })
                                    .inner;

                                if dl_clicked {
                                    self.start_download();
                                }
                            });
                        });

                    if let Some((notice, _)) = &self.paste_notice {
                        ui.add_space(4.0);
                        ui.label(RichText::new(notice).size(11.5).color(STATUS_CANCELED));
                    }

                    ui.add_space(12.0);

                    // 2. Stream Options Strip (Start Time, End Time, Quality, Format)
                    let mut config_dirty = false;
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

                                ui.label(RichText::new("Start").size(12.5).color(TEXT_MUTED));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.start_time)
                                        .hint_text("00:00:00")
                                        .desired_width(92.0)
                                        .min_size(Vec2::new(92.0, 28.0))
                                        .vertical_align(egui::Align::Center)
                                        .font(FontId::monospace(12.0)),
                                );

                                ui.add_space(8.0);
                                ui.label(RichText::new("End").size(12.5).color(TEXT_MUTED));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.end_time)
                                        .hint_text("HH:MM:SS")
                                        .desired_width(92.0)
                                        .min_size(Vec2::new(92.0, 28.0))
                                        .vertical_align(egui::Align::Center)
                                        .font(FontId::monospace(12.0)),
                                );

                                ui.add_space(8.0);
                                ui.label(RichText::new("Quality").size(12.5).color(TEXT_MUTED));
                                egui::ComboBox::from_id_salt("resolution_combo")
                                    .width(152.0)
                                    .selected_text(
                                        RichText::new(self.resolution.label())
                                            .size(12.0)
                                            .family(FontFamily::Monospace)
                                            .strong()
                                            .color(ACCENT_GREEN),
                                    )
                                    .show_ui(ui, |ui| {
                                        for r in Resolution::ALL {
                                            let is_sel = self.resolution == r;
                                            let item_btn = egui::Button::new(
                                                RichText::new(r.label())
                                                    .size(12.0)
                                                    .family(FontFamily::Monospace)
                                                    .color(if is_sel {
                                                        ACCENT_GREEN
                                                    } else {
                                                        TEXT_PRIMARY
                                                    }),
                                            )
                                            .fill(if is_sel { ACCENT_TINT } else { BG_SURFACE })
                                            .stroke(if is_sel {
                                                Stroke::new(1.0, ACCENT_GREEN)
                                            } else {
                                                Stroke::NONE
                                            })
                                            .rounding(Rounding::same(5.0))
                                            .min_size(Vec2::new(146.0, 26.0));

                                            if ui.add(item_btn).clicked() {
                                                if self.resolution != r {
                                                    self.resolution = r;
                                                    config_dirty = true;
                                                }
                                                ui.close_menu();
                                            }
                                        }
                                    });

                                ui.add_space(8.0);
                                ui.label(RichText::new("Format").size(12.5).color(TEXT_MUTED));
                                for ext in ContainerExt::ALL {
                                    let selected = self.container_ext == ext;
                                    let btn = egui::Button::new(
                                        RichText::new(ext.dot_upper())
                                            .size(11.5)
                                            .family(FontFamily::Monospace)
                                            .strong()
                                            .color(if selected {
                                                ACCENT_GREEN
                                            } else {
                                                TEXT_MUTED
                                            }),
                                    )
                                    .fill(if selected { ACCENT_TINT } else { BG_ELEVATED })
                                    .stroke(Stroke::new(
                                        1.0,
                                        if selected {
                                            ACCENT_GREEN
                                        } else {
                                            BORDER_SUBTLE
                                        },
                                    ))
                                    .rounding(Rounding::same(6.0))
                                    .min_size(Vec2::new(50.0, 28.0));

                                    if ui.add(btn).clicked() && self.container_ext != ext {
                                        self.container_ext = ext;
                                        config_dirty = true;
                                    }
                                }
                            });
                        });
                    if config_dirty {
                        self.persist_config();
                    }

                    ui.add_space(12.0);

                    // 3. Output Destination Row (single-row horizontal layout matching web preview)
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let btn_w = 114.0;
                                let left_w = (ui.available_width() - btn_w - 12.0).max(100.0);

                                ui.allocate_ui_with_layout(
                                    Vec2::new(left_w, 30.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_max_width(left_w);
                                        ui.label(
                                            RichText::new("Output Folder")
                                                .size(12.5)
                                                .color(TEXT_MUTED),
                                        );
                                        ui.add_space(6.0);
                                        let display_path = if folder_selected {
                                            self.output_path.as_str()
                                        } else {
                                            "Required: click Choose Folder to set destination..."
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(display_path)
                                                    .size(12.0)
                                                    .family(FontFamily::Monospace)
                                                    .color(if folder_selected {
                                                        TEXT_PRIMARY
                                                    } else {
                                                        TEXT_MUTED
                                                    }),
                                            )
                                            .truncate(),
                                        );
                                    },
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let folder_btn = styled_secondary_btn(
                                            "Choose Folder",
                                            if folder_selected {
                                                TEXT_PRIMARY
                                            } else {
                                                ACCENT_GREEN
                                            },
                                            if folder_selected {
                                                BORDER_HOVER
                                            } else {
                                                ACCENT_GREEN
                                            },
                                            Vec2::new(btn_w, 30.0),
                                        );

                                        if ui.add(folder_btn).clicked() {
                                            let mut dialog = rfd::FileDialog::new();
                                            if folder_selected
                                                && Path::new(&self.output_path).is_dir()
                                            {
                                                dialog = dialog.set_directory(&self.output_path);
                                            }
                                            if let Some(folder) = dialog.pick_folder() {
                                                let chosen = folder.display().to_string();
                                                self.output_path = chosen.clone();
                                                self.persist_config();
                                                self.push_log(format!(
                                                    "[config] Saved output destination: {}",
                                                    chosen
                                                ));
                                            }
                                        }
                                    },
                                );
                            });
                        });

                    ui.add_space(18.0);

                    // 4. Scrollable Queue & Recent Downloads Region
                    egui::ScrollArea::vertical()
                        .id_salt("downloads_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Active Queue")
                                        .size(15.0)
                                        .strong()
                                        .color(TEXT_PRIMARY),
                                );
                                if !self.active_downloads.is_empty() {
                                    ui.label(
                                        RichText::new(format!(
                                            "· {}",
                                            self.active_downloads.len()
                                        ))
                                        .size(13.0)
                                        .family(FontFamily::Monospace)
                                        .color(ACCENT_GREEN),
                                    );
                                }
                            });
                            ui.add_space(8.0);

                            if self.active_downloads.is_empty() {
                                egui::Frame::none()
                                    .fill(BG_SURFACE)
                                    .rounding(Rounding::same(10.0))
                                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                    .inner_margin(egui::Margin::symmetric(16.0, 15.0))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        let hint_msg = if folder_selected {
                                            "No active downloads. Paste any Kick VOD URL above to start."
                                        } else {
                                            "Choose an output folder above, then paste any Kick VOD URL to start."
                                        };
                                        ui.label(
                                            RichText::new(hint_msg).size(13.0).color(TEXT_MUTED),
                                        );
                                    });
                            } else {
                                let mut cancel_target_id: Option<usize> = None;

                                for item in &self.active_downloads {
                                    egui::Frame::none()
                                        .fill(BG_SURFACE)
                                        .rounding(Rounding::same(10.0))
                                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                        .inner_margin(egui::Margin::same(14.0))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                let cancel_w = 72.0;
                                                let title_w =
                                                    (ui.available_width() - cancel_w - 10.0)
                                                        .max(80.0);
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(title_w, 28.0),
                                                    egui::Layout::left_to_right(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        ui.set_max_width(title_w);
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(&item.title)
                                                                    .size(13.5)
                                                                    .strong()
                                                                    .color(TEXT_PRIMARY),
                                                            )
                                                            .truncate(),
                                                        );
                                                    },
                                                );

                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        let cancel_btn = styled_secondary_btn(
                                                            "Cancel",
                                                            STATUS_FAILED,
                                                            BORDER_HOVER,
                                                            Vec2::new(cancel_w, 28.0),
                                                        );
                                                        if ui.add(cancel_btn).clicked() {
                                                            cancel_target_id = Some(item.id);
                                                        }
                                                    },
                                                );
                                            });
                                            ui.add_space(8.0);
                                            draw_progress_row(ui, item.progress);
                                            ui.add_space(6.0);
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} · {}",
                                                        item.resolution, item.format
                                                    ))
                                                    .size(11.5)
                                                    .family(FontFamily::Monospace)
                                                    .color(ACCENT_GREEN),
                                                );
                                                ui.label(
                                                    RichText::new("·")
                                                        .size(11.5)
                                                        .color(TEXT_MUTED),
                                                );
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&item.status_text)
                                                            .size(11.5)
                                                            .family(FontFamily::Monospace)
                                                            .color(TEXT_MUTED),
                                                    )
                                                    .truncate(),
                                                );
                                            });
                                        });
                                    ui.add_space(8.0);
                                }

                                if let Some(id_to_cancel) = cancel_target_id {
                                    self.cancel_download(id_to_cancel);
                                }
                            }

                            ui.add_space(18.0);

                            // Recent Downloads Section
                            ui.label(
                                RichText::new("Recent Downloads")
                                    .size(15.0)
                                    .strong()
                                    .color(TEXT_PRIMARY),
                            );
                            ui.add_space(8.0);

                            if self.completed_downloads.is_empty() {
                                egui::Frame::none()
                                    .fill(BG_SURFACE)
                                    .rounding(Rounding::same(10.0))
                                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                    .inner_margin(egui::Margin::symmetric(16.0, 15.0))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.label(
                                            RichText::new("No completed downloads yet.")
                                                .size(13.0)
                                                .color(TEXT_MUTED),
                                        );
                                    });
                            } else {
                                for item in self.completed_downloads.iter().take(5) {
                                    self.render_history_card(ui, item);
                                    ui.add_space(6.0);
                                }
                            }
                        });
                }

                Tab::Terminal => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Terminal Output")
                                .size(17.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new(format!("· {} lines", self.terminal_logs.len()))
                                .size(12.0)
                                .family(FontFamily::Monospace)
                                .color(TEXT_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let clear_btn = styled_secondary_btn(
                                "Clear Logs",
                                TEXT_PRIMARY,
                                BORDER_HOVER,
                                Vec2::new(88.0, 28.0),
                            );
                            if ui.add(clear_btn).clicked() {
                                self.terminal_logs.clear();
                            }
                        });
                    });
                    ui.add_space(10.0);

                    // Virtualized O(1) log rendering: only lays out visible rows
                    let row_height = 19.0;
                    let total_rows = self.terminal_logs.len();
                    egui::Frame::none()
                        .fill(BG_SURFACE)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            egui::ScrollArea::vertical()
                                .id_salt("terminal_scroll")
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                                    for idx in row_range {
                                        if let Some(line) = self.terminal_logs.get(idx) {
                                            let line_color = if line.contains("[error]")
                                                || line.contains("[extractor error]")
                                                || line.contains("[updater error]")
                                            {
                                                STATUS_FAILED
                                            } else if line.contains("[cancel]") {
                                                STATUS_CANCELED
                                            } else if line.starts_with(">>>")
                                                || line.contains("Extracted '")
                                                || line.contains("[updater] Update available:")
                                            {
                                                ACCENT_GREEN
                                            } else {
                                                Color32::from_rgb(198, 204, 212)
                                            };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(line)
                                                        .size(12.0)
                                                        .family(FontFamily::Monospace)
                                                        .color(line_color),
                                                )
                                                .truncate(),
                                            );
                                        }
                                    }
                                });
                        });
                }

                Tab::Completed => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Completed Downloads")
                                .size(17.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        if !self.completed_downloads.is_empty() {
                            ui.label(
                                RichText::new(format!("· {}", self.completed_downloads.len()))
                                    .size(12.5)
                                    .family(FontFamily::Monospace)
                                    .color(TEXT_MUTED),
                            );
                        }
                    });
                    ui.add_space(10.0);

                    if self.completed_downloads.is_empty() {
                        egui::Frame::none()
                            .fill(BG_SURFACE)
                            .rounding(Rounding::same(10.0))
                            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                            .inner_margin(egui::Margin::same(18.0))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(
                                    RichText::new("No completed downloads yet.")
                                        .size(13.0)
                                        .color(TEXT_MUTED),
                                );
                            });
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("completed_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for item in &self.completed_downloads {
                                    self.render_history_card(ui, item);
                                    ui.add_space(8.0);
                                }
                            });
                    }
                }

                Tab::About => {
                    egui::ScrollArea::vertical()
                        .id_salt("about_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                draw_brand_mark(ui, 40.0);
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("5KD - 5wi's Kick Downloader")
                                                .size(18.5)
                                                .strong()
                                                .color(TEXT_PRIMARY),
                                        );
                                        ui.label(
                                            RichText::new(format!("· {}", APP_VERSION))
                                                .size(12.5)
                                                .family(FontFamily::Monospace)
                                                .color(ACCENT_GREEN),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(
                                            "Lightweight Kick.com VOD and clip downloader written in Rust.",
                                        )
                                        .size(13.0)
                                        .color(TEXT_MUTED),
                                    );
                                });
                            });

                            ui.add_space(18.0);

                            // Software Updates Card (in-app GitHub release check + direct silent installer update)
                            let mut trigger_check = false;
                            let mut trigger_update_info: Option<UpdateReleaseInfo> = None;
                            let mut persist_update_prefs = false;

                            egui::Frame::none()
                                .fill(BG_SURFACE)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                .inner_margin(egui::Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("Software Updates")
                                                    .size(14.5)
                                                    .strong()
                                                    .color(TEXT_PRIMARY),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "Installed: {} · {}",
                                                    APP_VERSION, INSTALLER_ASSET_NAME
                                                ))
                                                .size(11.5)
                                                .family(FontFamily::Monospace)
                                                .color(TEXT_MUTED),
                                            );
                                        });

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let busy = matches!(
                                                    self.update_state,
                                                    UpdateState::Checking
                                                        | UpdateState::Downloading { .. }
                                                        | UpdateState::Installing(_)
                                                );
                                                if let UpdateState::Available(info) =
                                                    self.update_state.clone()
                                                {
                                                    let install_clicked = ui
                                                        .scope(|ui| {
                                                            let w = &mut ui.visuals_mut().widgets;
                                                            w.hovered.bg_fill =
                                                                Color32::from_rgb(38, 255, 39);
                                                            w.hovered.weak_bg_fill =
                                                                Color32::from_rgb(38, 255, 39);
                                                            w.hovered.bg_stroke = Stroke::new(
                                                                1.0,
                                                                Color32::from_rgb(38, 255, 39),
                                                            );
                                                            let install_btn = egui::Button::new(
                                                                RichText::new("Update & Restart")
                                                                    .size(12.5)
                                                                    .strong()
                                                                    .color(Color32::BLACK),
                                                            )
                                                            .fill(ACCENT_GREEN)
                                                            .stroke(Stroke::new(1.0, ACCENT_GREEN))
                                                            .rounding(Rounding::same(7.0))
                                                            .min_size(Vec2::new(132.0, 30.0));
                                                            ui.add(install_btn).clicked()
                                                        })
                                                        .inner;
                                                    if install_clicked {
                                                        trigger_update_info = Some(info);
                                                    }
                                                    ui.add_space(6.0);
                                                }

                                                let check_label = if matches!(
                                                    self.update_state,
                                                    UpdateState::Checking
                                                ) {
                                                    "Checking..."
                                                } else {
                                                    "Check for Updates"
                                                };
                                                let check_btn = styled_secondary_btn(
                                                    check_label,
                                                    if busy { TEXT_MUTED } else { TEXT_PRIMARY },
                                                    BORDER_HOVER,
                                                    Vec2::new(128.0, 30.0),
                                                );
                                                if ui.add_enabled(!busy, check_btn).clicked() {
                                                    trigger_check = true;
                                                }
                                            },
                                        );
                                    });

                                    ui.add_space(10.0);

                                    // Auto-check on launch checkbox
                                    if ui
                                        .checkbox(
                                            &mut self.check_updates_on_launch,
                                            RichText::new("Check for updates when app launches")
                                                .size(13.0)
                                                .color(TEXT_PRIMARY),
                                        )
                                        .changed()
                                    {
                                        persist_update_prefs = true;
                                    }

                                    // Show GitHub repo field only when built locally outside a GitHub repo
                                    if GITHUB_REPO.is_empty() {
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new("GitHub Repo")
                                                    .size(12.0)
                                                    .color(TEXT_MUTED),
                                            );
                                            let resp = ui.add(
                                                egui::TextEdit::singleline(&mut self.github_repo)
                                                    .hint_text("owner/repo")
                                                    .desired_width(220.0)
                                                    .font(FontId::monospace(12.0)),
                                            );
                                            if resp.lost_focus() {
                                                persist_update_prefs = true;
                                            }
                                        });
                                    }

                                    // Live updater status & direct download progress
                                    match &self.update_state {
                                        UpdateState::Idle => {}
                                        UpdateState::Checking => {
                                            ui.add_space(10.0);
                                            ui.label(
                                                RichText::new(
                                                    "Checking GitHub Releases for a newer version...",
                                                )
                                                .size(12.0)
                                                .family(FontFamily::Monospace)
                                                .color(TEXT_MUTED),
                                            );
                                        }
                                        UpdateState::UpToDate(ver) => {
                                            ui.add_space(10.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "You are on the latest version ({}).",
                                                    ver
                                                ))
                                                .size(12.0)
                                                .family(FontFamily::Monospace)
                                                .color(ACCENT_GREEN),
                                            );
                                        }
                                        UpdateState::Available(info) => {
                                            ui.add_space(10.0);
                                            let size_suffix = if info.asset_size > 0 {
                                                format!(" ({})", format_bytes(info.asset_size))
                                            } else {
                                                String::new()
                                            };
                                            ui.label(
                                                RichText::new(format!(
                                                    "New release {} ready — {}{}",
                                                    info.tag_name, info.asset_name, size_suffix
                                                ))
                                                .size(12.5)
                                                .family(FontFamily::Monospace)
                                                .color(ACCENT_GREEN),
                                            );
                                            if !info.release_notes.is_empty() {
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(&info.release_notes)
                                                        .size(11.5)
                                                        .color(TEXT_MUTED),
                                                );
                                            }
                                        }
                                        UpdateState::Downloading {
                                            tag_name,
                                            asset_name,
                                            downloaded,
                                            total,
                                        } => {
                                            ui.add_space(10.0);
                                            let progress = if *total > 0 {
                                                (*downloaded as f32 / *total as f32)
                                                    .clamp(0.0, 1.0)
                                            } else {
                                                0.15
                                            };
                                            draw_progress_row(ui, progress);
                                            ui.add_space(4.0);
                                            let dl_status = if *total > 0 {
                                                format!(
                                                    "Downloading {} ({}) — {} / {}",
                                                    asset_name,
                                                    tag_name,
                                                    format_bytes(*downloaded),
                                                    format_bytes(*total)
                                                )
                                            } else {
                                                format!(
                                                    "Downloading {} ({}) — {}",
                                                    asset_name,
                                                    tag_name,
                                                    format_bytes(*downloaded)
                                                )
                                            };
                                            ui.label(
                                                RichText::new(dl_status)
                                                    .size(11.5)
                                                    .family(FontFamily::Monospace)
                                                    .color(TEXT_MUTED),
                                            );
                                        }
                                        UpdateState::Installing(tag_name) => {
                                            ui.add_space(10.0);
                                            draw_progress_row(ui, 1.0);
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "Installing {}, cleaning up temporary files, and restarting...",
                                                    tag_name
                                                ))
                                                .size(12.0)
                                                .family(FontFamily::Monospace)
                                                .color(ACCENT_GREEN),
                                            );
                                        }
                                        UpdateState::Error(err) => {
                                            ui.add_space(10.0);
                                            ui.label(
                                                RichText::new(err)
                                                    .size(12.0)
                                                    .family(FontFamily::Monospace)
                                                    .color(STATUS_FAILED),
                                            );
                                        }
                                    }
                                });

                            if persist_update_prefs {
                                self.persist_config();
                            }
                            if trigger_check {
                                self.check_for_updates();
                            }
                            if let Some(info) = trigger_update_info {
                                self.start_in_app_update(info);
                            }

                            ui.add_space(12.0);

                            egui::Frame::none()
                                .fill(BG_SURFACE)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                .inner_margin(egui::Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(
                                        RichText::new("Architecture & Engine")
                                            .size(14.5)
                                            .strong()
                                            .color(TEXT_PRIMARY),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "Built natively in Rust using eframe/egui, reqwest (rustls), rfd, arboard, yt-dlp, and ffmpeg for low-resource HLS stream downloading and lossless time slicing.",
                                        )
                                        .size(13.0)
                                        .color(TEXT_MUTED),
                                    );
                                });

                            ui.add_space(12.0);

                            egui::Frame::none()
                                .fill(BG_SURFACE)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                                .inner_margin(egui::Margin::same(16.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(
                                        RichText::new("Credits & Acknowledgments")
                                            .size(14.5)
                                            .strong()
                                            .color(TEXT_PRIMARY),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "M3U8 URL Extractor logic powered by kickvod-extractor by mortyobnoxious:",
                                        )
                                        .size(13.0)
                                        .color(TEXT_MUTED),
                                    );
                                    ui.add_space(4.0);
                                    ui.hyperlink(
                                        "https://github.com/mortyobnoxious/kickvod-extractor",
                                    );
                                });
                        });
                }
            }});
    }
}

fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

fn load_window_icon() -> Arc<egui::IconData> {
    const SIZE: usize = 128;
    const SAMPLES: usize = 4;
    let size_f = SIZE as f32;
    let border_w = 0.042_f32.max(1.25 / size_f);
    let half_border = border_w * 0.5;
    let margin = 0.015_f32;
    let half_box = 0.5 - margin - half_border;
    let corner_r = (0.21875 - half_border).max(0.02);
    let half_five = (0.105_f32 * 0.5).max(0.85 / size_f);

    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut sum_r = 0.0_f32;
            let mut sum_g = 0.0_f32;
            let mut sum_b = 0.0_f32;
            let mut sum_a = 0.0_f32;

            for sy in 0..SAMPLES {
                let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size_f;
                for sx in 0..SAMPLES {
                    let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size_f;
                    let d_box = sd_rounded_box(u, v, 0.5, 0.5, half_box, half_box, corner_r);
                    if d_box > half_border {
                        continue;
                    }

                    let mut d_five = f32::MAX;
                    for pair in BRAND_5_PTS.windows(2) {
                        let d = dist_to_segment(u, v, pair[0].0, pair[0].1, pair[1].0, pair[1].1);
                        if d < d_five {
                            d_five = d;
                        }
                    }

                    let (sr, sg, sb) = if d_box >= -half_border || d_five <= half_five {
                        (0.0_f32, 231.0_f32, 1.0_f32) // #00E701
                    } else {
                        (13.0_f32, 14.0_f32, 16.0_f32) // #0D0E10
                    };

                    sum_r += sr;
                    sum_g += sg;
                    sum_b += sb;
                    sum_a += 1.0;
                }
            }

            if sum_a > 0.0 {
                let idx = (y * SIZE + x) * 4;
                rgba[idx] = (sum_r / sum_a).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = (sum_g / sum_a).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = (sum_b / sum_a).round().clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = ((sum_a / (SAMPLES * SAMPLES) as f32) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }

    Arc::new(egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    })
}

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "windows")]
    if maybe_run_windows_update_helper() {
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("5KD - 5wi's Kick Downloader")
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(load_window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "5KD - 5wi's Kick Downloader",
        options,
        Box::new(|cc| Ok(Box::new(FiveKdApp::new(cc)))),
    )
}
