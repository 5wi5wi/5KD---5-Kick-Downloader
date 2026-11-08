#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::{DateTime, Datelike, NaiveDateTime, Timelike};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Rounding, Stroke, Vec2};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const AWS_ID: &str = "196233775518";
const VIDDASTRAGE_PROXY: &str = "https://cors.viddastrage.workers.dev/corsproxy/?apiurl=";
const PREFIXES_URL: &str =
    "https://raw.githubusercontent.com/mortyobnoxious/kickvod-extractor/refs/heads/main/prefixes.json";

// Pixel art grid matching /public/assets/aistudio/Gemini_Generated_Image_z2ceqoz2ceqoz2ce__1__ef8ae8c7.png
const ICON_5_GRID: [&str; 18] = [
    "################",
    "################",
    "################",
    "####............",
    "####............",
    "####............",
    "############....",
    "############....",
    "##############..",
    "..........#####.",
    "..........######",
    "............####",
    "####......######",
    "####......######",
    "################",
    ".##############.",
    "..############..",
    "..############..",
];

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Downloads,
    Terminal,
    Completed,
    About,
}

#[derive(PartialEq, Clone, Copy)]
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
    fn label(&self) -> &'static str {
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

    fn short_label(&self) -> &'static str {
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

    fn ytdlp_format(&self) -> &'static str {
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

#[derive(PartialEq, Clone, Copy)]
enum ContainerExt {
    Mkv,
    Mp4,
    Ts,
}

impl ContainerExt {
    fn as_str(&self) -> &'static str {
        match self {
            ContainerExt::Mkv => "mkv",
            ContainerExt::Mp4 => "mp4",
            ContainerExt::Ts => "ts",
        }
    }
}

#[derive(Clone)]
struct ExtractedVodMeta {
    m3u8_url: String,
    channel_slug: String,
    session_title: String,
    duration_formatted: String,
    source_type: String,
}

#[derive(Clone)]
struct DownloadItem {
    id: usize,
    title: String,
    url: String,
    output_path: String,
    resolution: String,
    format: String,
    progress: f32,
    status_text: String,
    is_completed: bool,
    is_failed: bool,
    cancel_flag: Arc<AtomicBool>,
    child_pid: Arc<Mutex<Option<u32>>>,
}

enum WorkerEvent {
    Log(String),
    Extracted(usize, ExtractedVodMeta),
    Progress(usize, f32, String),
    Finished(usize, bool, String),
}

fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
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

fn get_kick_json(client: &reqwest::blocking::Client, target_url: &str) -> Result<Value, String> {
    let candidates = [
        format!("{}{}", VIDDASTRAGE_PROXY, encode_uri_component(target_url)),
        target_url.to_string(),
        format!(
            "https://api.allorigins.win/raw?url={}",
            encode_uri_component(target_url)
        ),
    ];

    let mut last_err = String::from("No response from Kick API");
    for candidate in &candidates {
        match client
            .get(candidate)
            .header("Accept", "application/json")
            .send()
        {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    if let Ok(val) = resp.json::<Value>() {
                        if is_valid_kick_payload(&val) {
                            return Ok(val);
                        } else {
                            last_err = format!("API returned error object for {}", target_url);
                        }
                    }
                } else {
                    last_err = format!("HTTP {} from {}", status.as_u16(), target_url);
                }
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }
    Err(last_err)
}

fn test_m3u8_url(client: &reqwest::blocking::Client, m3u8_url: &str) -> bool {
    if let Ok(resp) = client.get(m3u8_url).send() {
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
    false
}

fn parse_kick_utc(s: &str) -> Option<NaiveDateTime> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.naive_utc());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.fZ") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt);
    }
    None
}

fn format_ivs_time_path(dt: &NaiveDateTime) -> String {
    format!(
        "{}/{}/{}/{}/{}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute()
    )
}

fn extract_thumb_ids(thumb: &str) -> Option<(String, String)> {
    if let Some(idx) = thumb.find("video_thumbnails/") {
        let rest = &thumb[idx + "video_thumbnails/".len()..];
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}

/// Extracts Master M3U8 and metadata from a Kick VOD URL
/// Full port of https://github.com/mortyobnoxious/kickvod-extractor
fn extract_kick_vod(raw_url: &str, tx: &Sender<WorkerEvent>) -> Result<ExtractedVodMeta, String> {
    let trimmed = raw_url.trim();
    if trimmed.contains(".m3u8") {
        return Ok(ExtractedVodMeta {
            m3u8_url: trimmed.to_string(),
            channel_slug: "kick_stream".to_string(),
            session_title: "Direct HLS Stream".to_string(),
            duration_formatted: "Live/VOD".to_string(),
            source_type: "Direct M3U8 URL".to_string(),
        });
    }

    let mut user = String::new();
    let mut uuid = String::new();

    if let Some(idx) = trimmed.find("kick.com/") {
        let rest = &trimmed[idx + "kick.com/".len()..];
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 3 && parts[1] == "videos" {
            user = parts[0].to_string();
            uuid = parts[2]
                .split(&['?', '#'][..])
                .next()
                .unwrap_or("")
                .to_string();
        } else if parts.len() >= 2 && parts[0] == "video" {
            uuid = parts[1]
                .split(&['?', '#'][..])
                .next()
                .unwrap_or("")
                .to_string();
        }
    }

    if uuid.is_empty() {
        return Err("Invalid Kick VOD URL (expected https://kick.com/{channel}/videos/{uuid})".to_string());
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    // 1. Try v1/video/{uuid} first, then fallback to v2/channels/{user}/videos with UUIDv7 timestamp matching
    let v1_url = format!("https://kick.com/api/v1/video/{}", uuid);
    let mut vod_obj: Option<Value> = get_kick_json(&client, &v1_url).ok();
    let mut v2_matched_obj: Option<Value> = None;

    if vod_obj.is_none() && !user.is_empty() {
        let _ = tx.send(WorkerEvent::Log(format!(
            "[extractor] Querying v2 channel videos for '{}' (UUIDv7 lookup)...",
            user
        )));
        let v2_url = format!("https://kick.com/api/v2/channels/{}/videos", user);
        let v2_videos = get_kick_json(&client, &v2_url)?;

        if let Some(arr) = v2_videos.as_array() {
            let hex_ts: String = uuid.chars().filter(|c| *c != '-').take(12).collect();
            let uuid_time_ms = i64::from_str_radix(&hex_ts, 16).unwrap_or(0);

            for v in arr {
                let vod_id_match = v.get("vod_id").and_then(|s| s.as_str()) == Some(&uuid);
                let video_uuid_match = v
                    .get("video")
                    .and_then(|vid| vid.get("uuid"))
                    .and_then(|s| s.as_str())
                    == Some(&uuid);

                let time_match = if uuid_time_ms > 0 {
                    let date_str = v
                        .get("video")
                        .and_then(|vid| vid.get("created_at"))
                        .and_then(|s| s.as_str())
                        .or_else(|| v.get("created_at").and_then(|s| s.as_str()))
                        .or_else(|| v.get("start_time").and_then(|s| s.as_str()))
                        .unwrap_or("");
                    if let Some(dt) = parse_kick_utc(date_str) {
                        let video_time_ms = dt.and_utc().timestamp_millis();
                        (video_time_ms - uuid_time_ms).abs() < 30_000
                    } else {
                        false
                    }
                } else {
                    false
                };

                if vod_id_match || video_uuid_match || time_match {
                    v2_matched_obj = Some(v.clone());
                    if let Some(inner_uuid) = v
                        .get("video")
                        .and_then(|vid| vid.get("uuid"))
                        .and_then(|s| s.as_str())
                    {
                        let inner_v1 = format!("https://kick.com/api/v1/video/{}", inner_uuid);
                        if let Ok(v1_res) = get_kick_json(&client, &inner_v1) {
                            vod_obj = Some(v1_res);
                        }
                    }
                    break;
                }
            }
        }
    }

    let vod = vod_obj
        .as_ref()
        .or(v2_matched_obj.as_ref())
        .ok_or_else(|| "VOD not found in Kick database".to_string())?;

    let ls = vod.get("livestream").or(v2_matched_obj.as_ref());

    let channel_slug = ls
        .and_then(|l| l.get("channel"))
        .and_then(|c| c.get("slug"))
        .and_then(|s| s.as_str())
        .unwrap_or(if user.is_empty() { "kick_vod" } else { &user })
        .to_string();

    let session_title = ls
        .and_then(|l| l.get("session_title"))
        .or_else(|| vod.get("session_title"))
        .and_then(|s| s.as_str())
        .unwrap_or("Kick Broadcast VOD")
        .to_string();

    let duration_ms = ls
        .and_then(|l| l.get("duration"))
        .or_else(|| vod.get("duration"))
        .and_then(|d| d.as_u64())
        .unwrap_or(0);
    let total_secs = duration_ms / 1000;
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

    // 2. Direct source URL from Kick API (v1 or v2)
    let direct_source = vod
        .get("source")
        .and_then(|s| s.as_str())
        .or_else(|| {
            v2_matched_obj
                .as_ref()
                .and_then(|m| m.get("source"))
                .and_then(|s| s.as_str())
        });

    if let Some(src) = direct_source {
        if !src.is_empty() {
            return Ok(ExtractedVodMeta {
                m3u8_url: src.to_string(),
                channel_slug,
                session_title,
                duration_formatted,
                source_type: "Source URL from API".to_string(),
            });
        }
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

    let mut candidate_times: Vec<String> = Vec::new();
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
            let formatted = format_ivs_time_path(&dt);
            if !candidate_times.contains(&formatted) {
                candidate_times.push(formatted);
            }
        }
    }

    if candidate_times.is_empty() {
        return Err("Could not parse VOD creation timestamp".to_string());
    }

    let _ = tx.send(WorkerEvent::Log(
        "[extractor] Testing direct AWS IVS stream URL...".to_string(),
    ));
    for time_path in &candidate_times {
        let ivs_path = format!(
            "ivs/v1/{}/{}/{}/{}/media/hls/master.m3u8",
            AWS_ID, session, time_path, segment
        );
        let direct_url = format!("https://stream.kick.com/{}", ivs_path);
        if test_m3u8_url(&client, &direct_url) {
            return Ok(ExtractedVodMeta {
                m3u8_url: direct_url,
                channel_slug,
                session_title,
                duration_formatted,
                source_type: "Direct URL (no prefix)".to_string(),
            });
        }
    }

    let _ = tx.send(WorkerEvent::Log(
        "[extractor] Fetching CDN prefixes list from kickvod-extractor...".to_string(),
    ));
    let prefixes_val = get_kick_json(&client, PREFIXES_URL)?;
    if let Some(prefixes) = prefixes_val.as_array() {
        for prefix_val in prefixes {
            if let Some(prefix) = prefix_val.as_str() {
                for time_path in &candidate_times {
                    let ivs_path = format!(
                        "ivs/v1/{}/{}/{}/{}/media/hls/master.m3u8",
                        AWS_ID, session, time_path, segment
                    );
                    let candidate_url = if prefix.is_empty() {
                        format!("https://stream.kick.com/{}", ivs_path)
                    } else {
                        format!("https://stream.kick.com/{}/{}", prefix, ivs_path)
                    };
                    if test_m3u8_url(&client, &candidate_url) {
                        return Ok(ExtractedVodMeta {
                            m3u8_url: candidate_url,
                            channel_slug,
                            session_title,
                            duration_formatted,
                            source_type: format!("Found with prefix: {}", prefix),
                        });
                    }
                }
            }
        }
    }

    Err("No working M3U8 URL found with available prefixes".to_string())
}

fn draw_pixel_5_icon(ui: &mut egui::Ui, size: f32, accent_green: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, Color32::BLACK);
    let cell_w = rect.width() / 20.0;
    let cell_h = rect.height() / 22.0;
    for (r, row) in ICON_5_GRID.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            if ch == '#' {
                let px = rect.min.x + (c as f32 + 2.0) * cell_w;
                let py = rect.min.y + (r as f32 + 2.0) * cell_h;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(egui::pos2(px, py), Vec2::new(cell_w, cell_h)),
                    0.0,
                    accent_green,
                );
            }
        }
    }
}

struct FiveKdApp {
    current_tab: Tab,
    zoom_percent: u32, // 100 to 150
    url_input: String,
    start_time: String,
    end_time: String,
    resolution: Resolution,
    container_ext: ContainerExt,
    output_path: String,

    active_downloads: Vec<DownloadItem>,
    completed_downloads: Vec<DownloadItem>,
    terminal_logs: Vec<String>,
    next_id: usize,

    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
}

impl FiveKdApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // #00e701 -> RGB(0, 231, 1)
        let accent_green = Color32::from_rgb(0, 231, 1);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(0, 0, 0); // Pure black #000000
        visuals.window_fill = Color32::from_rgb(0, 0, 0); // Pure black popup menu background
        visuals.window_stroke = Stroke::new(1.5, accent_green); // #00e701 popup menu border
        visuals.extreme_bg_color = Color32::from_rgb(16, 16, 16);
        visuals.selection.bg_fill = Color32::BLACK;
        visuals.selection.stroke = Stroke::new(1.5, accent_green);
        visuals.hyperlink_color = accent_green;

        // Use #000000 background and #00e701 accent around interactive widgets and popup menus
        visuals.widgets.inactive.bg_fill = Color32::BLACK;
        visuals.widgets.inactive.weak_bg_fill = Color32::BLACK;
        visuals.widgets.hovered.bg_fill = Color32::BLACK;
        visuals.widgets.hovered.weak_bg_fill = Color32::BLACK;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, accent_green);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, accent_green);
        visuals.widgets.active.bg_fill = Color32::BLACK;
        visuals.widgets.active.weak_bg_fill = Color32::BLACK;
        visuals.widgets.active.bg_stroke = Stroke::new(1.5, accent_green);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, accent_green);
        visuals.widgets.open.bg_fill = Color32::BLACK;
        visuals.widgets.open.weak_bg_fill = Color32::BLACK;
        visuals.widgets.open.bg_stroke = Stroke::new(1.5, accent_green);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, accent_green);

        cc.egui_ctx.set_visuals(visuals);

        let (tx, rx) = channel();

        Self {
            current_tab: Tab::Downloads,
            zoom_percent: 100,
            url_input: String::new(),
            start_time: String::new(),
            end_time: String::new(),
            resolution: Resolution::R1080p60,
            container_ext: ContainerExt::Mkv,
            output_path: String::new(),
            active_downloads: Vec::new(),
            completed_downloads: Vec::new(),
            terminal_logs: vec![
                "[info] 5KD - 5wi's Kick Downloader (Native Rust Engine) initialized.".to_string(),
                "[info] Integrated Kick VOD Extractor ready.".to_string(),
            ],
            next_id: 1,
            tx,
            rx,
        }
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
                    #[cfg(target_os = "windows")]
                    {
                        let _ = Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let _ = Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                    }
                }
            }

            self.terminal_logs.push(format!(
                "[cancel] Download #{} canceled by user: {}",
                id, item.title
            ));
            item.is_completed = false;
            item.is_failed = true;
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
            title: format!("Extracting VOD: {}...", raw_url),
            url: raw_url.clone(),
            output_path: custom_out.clone(),
            resolution: res.short_label().to_string(),
            format: ext.as_str().to_uppercase(),
            progress: 0.02,
            status_text: "Extracting Master M3U8 URL from Kick...".to_string(),
            is_completed: false,
            is_failed: false,
            cancel_flag: cancel_flag.clone(),
            child_pid: child_pid.clone(),
        };

        self.active_downloads.insert(0, item);
        self.url_input.clear();

        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(WorkerEvent::Log(format!(
                "[extractor] Analyzing URL: {}",
                raw_url
            )));

            let extracted = match extract_kick_vod(&raw_url, &tx) {
                Ok(meta) => {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "[extractor] Extracted Master M3U8 ({}): {}",
                        meta.source_type, meta.m3u8_url
                    )));
                    let _ = tx.send(WorkerEvent::Extracted(id, meta.clone()));
                    meta
                }
                Err(e) => {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return;
                    }
                    let _ = tx.send(WorkerEvent::Log(format!(
                        "[extractor error] {}",
                        e
                    )));
                    let _ = tx.send(WorkerEvent::Finished(id, false, e));
                    return;
                }
            };

            if cancel_flag.load(Ordering::SeqCst) {
                return;
            }

            let default_filename = format!("{}_vod.{}", extracted.channel_slug, ext.as_str());
            let final_output = if custom_out.ends_with('\\') || custom_out.ends_with('/') {
                format!("{}{}", custom_out, default_filename)
            } else if custom_out.ends_with(".mkv")
                || custom_out.ends_with(".mp4")
                || custom_out.ends_with(".ts")
            {
                custom_out
            } else {
                format!("{}\\{}", custom_out, default_filename)
            };

            let mut cmd = Command::new("yt-dlp");
            cmd.arg("-f")
                .arg(res.ytdlp_format())
                .arg("-N")
                .arg("8")
                .arg("--user-agent")
                .arg("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                .arg("--add-header")
                .arg("Origin:https://kick.com")
                .arg("--add-header")
                .arg("Referer:https://kick.com/")
                .arg("--downloader")
                .arg("ffmpeg");

            let mut time_args = Vec::new();
            if !start_t.is_empty() {
                time_args.push(format!("-ss {}", start_t));
            }
            if !end_t.is_empty() {
                time_args.push(format!("-to {}", end_t));
            }
            if !time_args.is_empty() {
                cmd.arg("--downloader-args")
                    .arg(format!("ffmpeg_i:{}", time_args.join(" ")));
            }

            cmd.arg("-o")
                .arg(&final_output)
                .arg(&extracted.m3u8_url)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let _ = tx.send(WorkerEvent::Log(format!(
                ">>> Executing yt-dlp -> {}",
                final_output
            )));

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
                        thread::spawn(move || {
                            let reader = BufReader::new(stderr);
                            for line in reader.lines().flatten() {
                                if cancel_err.load(Ordering::SeqCst) {
                                    break;
                                }
                                let _ = tx_err.send(WorkerEvent::Log(line.clone()));
                                if line.contains("frame=") || line.contains("time=") {
                                    let _ = tx_err.send(WorkerEvent::Progress(id, 0.5, line));
                                }
                            }
                        });
                    }

                    if let Some(stdout) = child.stdout.take() {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines().flatten() {
                            if cancel_flag.load(Ordering::SeqCst) {
                                break;
                            }
                            let _ = tx.send(WorkerEvent::Log(line.clone()));
                            if let Some(pct_idx) = line.find('%') {
                                let before = &line[..pct_idx];
                                if let Some(num_str) = before.split_whitespace().last() {
                                    if let Ok(pct) = num_str.parse::<f32>() {
                                        let _ = tx.send(WorkerEvent::Progress(
                                            id,
                                            (pct / 100.0).clamp(0.0, 1.0),
                                            line.clone(),
                                        ));
                                    }
                                }
                            }
                        }
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
                            ));
                        }
                        Ok(s) => {
                            let _ = tx.send(WorkerEvent::Finished(
                                id,
                                false,
                                format!("yt-dlp exited with code {:?}", s.code()),
                            ));
                        }
                        Err(e) => {
                            let _ = tx.send(WorkerEvent::Finished(id, false, e.to_string()));
                        }
                    }
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
                    ));
                }
            }
        });
    }

    fn poll_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                WorkerEvent::Log(line) => {
                    self.terminal_logs.push(line);
                }
                WorkerEvent::Extracted(id, meta) => {
                    if let Some(item) = self.active_downloads.iter_mut().find(|i| i.id == id) {
                        item.title = format!(
                            "{} - {} ({})",
                            meta.channel_slug, meta.session_title, meta.duration_formatted
                        );
                        item.url = meta.m3u8_url;
                        item.status_text = "Downloading stream via yt-dlp + ffmpeg...".to_string();
                    }
                }
                WorkerEvent::Progress(id, prog, status) => {
                    if let Some(item) = self.active_downloads.iter_mut().find(|i| i.id == id) {
                        if prog > item.progress {
                            item.progress = prog;
                        }
                        item.status_text = status;
                    }
                }
                WorkerEvent::Finished(id, success, msg) => {
                    if let Some(pos) = self.active_downloads.iter().position(|i| i.id == id) {
                        let mut item = self.active_downloads.remove(pos);
                        item.progress = if success { 1.0 } else { item.progress };
                        item.is_completed = success;
                        item.is_failed = !success;
                        item.status_text = msg;
                        self.completed_downloads.insert(0, item);
                    }
                }
            }
        }
    }
}

impl eframe::App for FiveKdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events();
        ctx.set_zoom_factor(self.zoom_percent as f32 / 100.0);
        ctx.request_repaint_after(std::time::Duration::from_millis(120));

        // #00e701 -> RGB(0, 231, 1)
        let accent_green = Color32::from_rgb(0, 231, 1);
        let sidebar_bg = Color32::BLACK; // Pure black #000000
        let card_bg = Color32::from_rgb(26, 26, 26);     // #1a1a1a
        let input_bg = Color32::from_rgb(16, 16, 16);    // #101010
        let border_col = Color32::from_rgb(42, 42, 42);  // #2a2a2a
        let muted_text = Color32::from_rgb(155, 160, 165);

        // Top Bar with Zoom In/Out Buttons (100% to 150%)
        egui::TopBottomPanel::top("top_bar")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(12, 12, 12))
                    .inner_margin(egui::Margin::symmetric(14.0, 7.0))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(31, 31, 31))),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    draw_pixel_5_icon(ui, 18.0, accent_green);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("5KD - 5wi's Kick Downloader")
                            .size(12.5)
                            .strong()
                            .color(Color32::WHITE),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Zoom In (+) up to 150%
                        let zoom_in_btn = egui::Button::new(
                            RichText::new("+")
                                .size(13.0)
                                .family(FontFamily::Monospace)
                                .strong()
                                .color(if self.zoom_percent < 150 {
                                    accent_green
                                } else {
                                    muted_text
                                }),
                        )
                        .fill(Color32::BLACK)
                        .stroke(Stroke::new(1.0, border_col))
                        .rounding(Rounding::same(5.0))
                        .min_size(Vec2::new(26.0, 22.0));

                        if ui.add_enabled(self.zoom_percent < 150, zoom_in_btn).clicked() {
                            self.zoom_percent = (self.zoom_percent + 10).min(150);
                        }

                        // Current Zoom Percentage badge (100% - 150%)
                        ui.label(
                            RichText::new(format!("{}%", self.zoom_percent))
                                .size(11.5)
                                .family(FontFamily::Monospace)
                                .color(accent_green),
                        );

                        // Zoom Out (-) down to 100%
                        let zoom_out_btn = egui::Button::new(
                            RichText::new("-")
                                .size(13.0)
                                .family(FontFamily::Monospace)
                                .strong()
                                .color(if self.zoom_percent > 100 {
                                    accent_green
                                } else {
                                    muted_text
                                }),
                        )
                        .fill(Color32::BLACK)
                        .stroke(Stroke::new(1.0, border_col))
                        .rounding(Rounding::same(5.0))
                        .min_size(Vec2::new(26.0, 22.0));

                        if ui.add_enabled(self.zoom_percent > 100, zoom_out_btn).clicked() {
                            self.zoom_percent = (self.zoom_percent.saturating_sub(10)).max(100);
                        }

                        ui.label(
                            RichText::new("Zoom:")
                                .size(11.5)
                                .color(muted_text),
                        );
                    });
                });
            });

        // Left Sidebar (#141414)
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(205.0)
            .frame(
                egui::Frame::none()
                    .fill(sidebar_bg)
                    .inner_margin(egui::Margin::same(16.0))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(31, 31, 31))),
            )
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        draw_pixel_5_icon(ui, 26.0, accent_green);
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("5KD")
                                .size(20.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                    });
                    ui.label(
                        RichText::new("5wi's Kick Downloader")
                            .size(11.5)
                            .color(muted_text),
                    );
                    ui.add_space(20.0);

                    let nav_items = [
                        (Tab::Downloads, "Downloads", self.active_downloads.len(), "📥"),
                        (Tab::Terminal, "Terminal", self.terminal_logs.len(), ">_"),
                        (Tab::Completed, "Completed", self.completed_downloads.len(), "✓"),
                        (Tab::About, "About", 0, "ℹ"),
                    ];

                    for (tab, label, count, icon) in nav_items {
                        let is_selected = self.current_tab == tab;
                        let fg = if is_selected {
                            accent_green
                        } else {
                            muted_text
                        };

                        let btn_res = egui::Frame::none()
                            .fill(if is_selected {
                                Color32::from_rgba_premultiplied(0, 231, 1, 12)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .rounding(Rounding { nw: 0.0, sw: 0.0, ne: 6.0, se: 6.0 })
                            .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(icon)
                                            .size(13.0)
                                            .color(fg)
                                            .family(FontFamily::Monospace),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(label).size(13.5).color(fg).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if count > 0 {
                                                ui.label(
                                                    RichText::new(format!("{}", count))
                                                        .size(11.0)
                                                        .color(accent_green)
                                                        .family(FontFamily::Monospace),
                                                );
                                            }
                                        },
                                    );
                                });
                            });

                        let interact = btn_res.response.interact(egui::Sense::click());
                        // Use a line instead for the active/hovered tab indicator
                        if is_selected {
                            ui.painter().vline(
                                btn_res.response.rect.left(),
                                btn_res.response.rect.y_range(),
                                Stroke::new(3.0, accent_green),
                            );
                        } else if interact.hovered() {
                            ui.painter().vline(
                                btn_res.response.rect.left(),
                                btn_res.response.rect.y_range(),
                                Stroke::new(2.0, accent_green),
                            );
                        }

                        if interact.clicked() {
                            self.current_tab = tab;
                        }
                        ui.add_space(4.0);
                    }
                });
            });

        // Main Central Panel (#000000)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::BLACK)
                    .inner_margin(egui::Margin::same(22.0)),
            )
            .show(ctx, |ui| match self.current_tab {
                Tab::Downloads => {
                    let folder_selected = self.has_chosen_folder();
                    let can_download = folder_selected && !self.url_input.trim().is_empty();

                    // 1. Top URL Bar with #00e701 hover accent
                    let url_frame = egui::Frame::none()
                        .fill(input_bg)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, border_col))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let text_edit = egui::TextEdit::singleline(&mut self.url_input)
                                    .hint_text("Paste Kick VOD/Clip URL here...")
                                    .frame(false)
                                    .font(FontId::proportional(15.0))
                                    .desired_width(ui.available_width() - 115.0);
                                let resp = ui.add(text_edit);
                                if resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    && can_download
                                {
                                    self.start_download();
                                }

                                let dl_btn = egui::Button::new(
                                    RichText::new("Download")
                                        .size(13.5)
                                        .strong()
                                        .color(if can_download {
                                            accent_green
                                        } else {
                                            muted_text
                                        }),
                                )
                                .fill(Color32::BLACK)
                                .stroke(Stroke::new(
                                    1.0,
                                    if can_download {
                                        accent_green
                                    } else {
                                        border_col
                                    },
                                ))
                                .rounding(Rounding::same(8.0))
                                .min_size(Vec2::new(100.0, 34.0));

                                let dl_resp = ui
                                    .add_enabled(can_download, dl_btn)
                                    .on_disabled_hover_text(
                                        "Choose an output folder below before downloading",
                                    );
                                if dl_resp.clicked() {
                                    self.start_download();
                                }
                            });
                        });
                    if url_frame.response.hovered() {
                        ui.painter().rect_stroke(
                            url_frame.response.rect,
                            Rounding::same(10.0),
                            Stroke::new(1.5, accent_green),
                        );
                    }

                    ui.add_space(10.0);

                    // 2. Controls Row: Start Time, End Time, Resolution, Container Extension
                    let ctrl_frame = egui::Frame::none()
                        .fill(card_bg)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, border_col))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new("Start Time (-ss):")
                                        .size(12.5)
                                        .color(muted_text),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.start_time)
                                        .hint_text("00:00:00 (Optional)")
                                        .desired_width(120.0)
                                        .font(FontId::monospace(12.5)),
                                );

                                ui.add_space(10.0);

                                ui.label(
                                    RichText::new("End Time (-to):")
                                        .size(12.5)
                                        .color(muted_text),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.end_time)
                                        .hint_text("Optional (HH:MM:SS)")
                                        .desired_width(120.0)
                                        .font(FontId::monospace(12.5)),
                                );

                                ui.add_space(10.0);

                                // Resolution next to End Time
                                ui.label(
                                    RichText::new("Resolution:").size(12.5).color(muted_text),
                                );
                                egui::ComboBox::from_id_salt("resolution_combo")
                                    .selected_text(
                                        RichText::new(self.resolution.label())
                                            .size(12.0)
                                            .family(FontFamily::Monospace)
                                            .strong()
                                            .color(accent_green),
                                    )
                                    .show_ui(ui, |ui| {
                                        for r in [
                                            Resolution::R1080p60,
                                            Resolution::R1080p,
                                            Resolution::R720p60,
                                            Resolution::R720p,
                                            Resolution::R480p,
                                            Resolution::R360p,
                                            Resolution::AudioOnly,
                                        ] {
                                            let is_sel = self.resolution == r;
                                            let item_btn = egui::Button::new(
                                                RichText::new(r.label())
                                                    .size(12.0)
                                                    .family(FontFamily::Monospace)
                                                    .strong()
                                                    .color(if is_sel {
                                                        accent_green
                                                    } else {
                                                        muted_text
                                                    }),
                                            )
                                            .fill(Color32::BLACK)
                                            .stroke(if is_sel {
                                                Stroke::new(1.0, accent_green)
                                            } else {
                                                Stroke::NONE
                                            })
                                            .rounding(Rounding::same(5.0))
                                            .min_size(Vec2::new(145.0, 24.0));

                                            if ui.add(item_btn).clicked() {
                                                self.resolution = r;
                                                ui.close_menu();
                                            }
                                        }
                                    });

                                ui.add_space(10.0);

                                // Container Extension to the right of Resolution
                                // Uses green (#00e701) for text and black (#000000) for background
                                ui.label(
                                    RichText::new("Container Extension:")
                                        .size(12.5)
                                        .color(muted_text),
                                );
                                for ext in [ContainerExt::Mkv, ContainerExt::Mp4, ContainerExt::Ts]
                                {
                                    let selected = self.container_ext == ext;
                                    let btn = egui::Button::new(
                                        RichText::new(format!(".{}", ext.as_str().to_uppercase()))
                                            .size(11.5)
                                            .family(FontFamily::Monospace)
                                            .strong()
                                            .color(if selected {
                                                accent_green
                                            } else {
                                                muted_text
                                            }),
                                    )
                                    .fill(Color32::BLACK)
                                    .stroke(if selected {
                                        Stroke::new(1.5, accent_green)
                                    } else {
                                        Stroke::new(1.0, Color32::from_rgb(51, 51, 51))
                                    })
                                    .rounding(Rounding::same(5.0));

                                    if ui.add(btn).clicked() {
                                        self.container_ext = ext;
                                    }
                                }
                            });
                        });
                    if ctrl_frame.response.hovered() {
                        ui.painter().rect_stroke(
                            ctrl_frame.response.rect,
                            Rounding::same(10.0),
                            Stroke::new(1.0, accent_green),
                        );
                    }

                    ui.add_space(10.0);

                    // 3. Output Path + File Explorer Folder Picker (Required before downloading)
                    let out_frame = egui::Frame::none()
                        .fill(sidebar_bg)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, border_col))
                        .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Output Folder (-o):")
                                        .size(12.5)
                                        .color(muted_text),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.output_path)
                                        .interactive(false)
                                        .hint_text("Required: Click 'Choose Folder' to enable downloading...")
                                        .desired_width(ui.available_width() - 135.0)
                                        .font(FontId::monospace(12.0)),
                                );

                                let folder_btn = egui::Button::new(
                                    RichText::new("Choose Folder")
                                        .size(12.0)
                                        .strong()
                                        .color(accent_green),
                                )
                                .fill(Color32::BLACK)
                                .stroke(Stroke::new(
                                    1.0,
                                    if folder_selected {
                                        Color32::from_rgb(51, 51, 51)
                                    } else {
                                        accent_green
                                    },
                                ))
                                .rounding(Rounding::same(6.0));

                                if ui.add(folder_btn).clicked() {
                                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                        self.output_path = folder.display().to_string();
                                    }
                                }
                            });
                        });
                    if out_frame.response.hovered() {
                        ui.painter().rect_stroke(
                            out_frame.response.rect,
                            Rounding::same(10.0),
                            Stroke::new(1.0, accent_green),
                        );
                    }

                    ui.add_space(18.0);

                    // 4. Active Downloads Queue with Cancel Button
                    ui.label(
                        RichText::new("Download Queue / Active")
                            .size(15.5)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(6.0);

                    if self.active_downloads.is_empty() {
                        let empty_active = egui::Frame::none()
                            .fill(card_bg)
                            .rounding(Rounding::same(10.0))
                            .stroke(Stroke::new(1.0, border_col))
                            .inner_margin(egui::Margin::same(18.0))
                            .show(ui, |ui| {
                                let hint_msg = if folder_selected {
                                    "No active downloads. Paste any Kick VOD URL above to automatically extract & download."
                                } else {
                                    "No active downloads. Choose an output folder first, then paste any Kick VOD URL to download."
                                };
                                ui.label(
                                    RichText::new(hint_msg)
                                        .size(13.0)
                                        .color(muted_text),
                                );
                            });
                        if empty_active.response.hovered() {
                            ui.painter().rect_stroke(
                                empty_active.response.rect,
                                Rounding::same(10.0),
                                Stroke::new(1.0, accent_green),
                            );
                        }
                    } else {
                        let mut cancel_target_id: Option<usize> = None;

                        for item in &self.active_downloads {
                            let card_res = egui::Frame::none()
                                .fill(card_bg)
                                .rounding(Rounding::same(10.0))
                                .stroke(Stroke::new(1.0, border_col))
                                .inner_margin(egui::Margin::same(14.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&item.title)
                                                .size(14.0)
                                                .strong()
                                                .color(Color32::WHITE),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let cancel_btn = egui::Button::new(
                                                    RichText::new("Cancel")
                                                        .size(12.0)
                                                        .strong()
                                                        .color(accent_green),
                                                )
                                                .fill(Color32::BLACK)
                                                .stroke(Stroke::new(1.0, accent_green))
                                                .rounding(Rounding::same(6.0))
                                                .min_size(Vec2::new(72.0, 26.0));

                                                if ui.add(cancel_btn).clicked() {
                                                    cancel_target_id = Some(item.id);
                                                }
                                            },
                                        );
                                    });
                                    ui.add_space(6.0);
                                    ui.add(
                                        egui::ProgressBar::new(item.progress)
                                            .fill(accent_green)
                                            .show_percentage(),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(&item.status_text)
                                            .size(11.5)
                                            .color(muted_text)
                                            .family(FontFamily::Monospace),
                                    );
                                });
                            if card_res.response.hovered() {
                                ui.painter().rect_stroke(
                                    card_res.response.rect,
                                    Rounding::same(10.0),
                                    Stroke::new(1.0, accent_green),
                                );
                            }
                            ui.add_space(8.0);
                        }

                        if let Some(id_to_cancel) = cancel_target_id {
                            self.cancel_download(id_to_cancel);
                        }
                    }

                    ui.add_space(14.0);

                    // 5. Recent Downloads
                    ui.label(
                        RichText::new("Recent Downloads")
                            .size(15.5)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(6.0);

                    if self.completed_downloads.is_empty() {
                        let empty_rec = egui::Frame::none()
                            .fill(card_bg)
                            .rounding(Rounding::same(10.0))
                            .stroke(Stroke::new(1.0, border_col))
                            .inner_margin(egui::Margin::same(16.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("No completed downloads yet.")
                                        .size(13.0)
                                        .color(muted_text),
                                );
                            });
                        if empty_rec.response.hovered() {
                            ui.painter().rect_stroke(
                                empty_rec.response.rect,
                                Rounding::same(10.0),
                                Stroke::new(1.0, accent_green),
                            );
                        }
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for item in &self.completed_downloads {
                                let rec_res = egui::Frame::none()
                                    .fill(card_bg)
                                    .rounding(Rounding::same(10.0))
                                    .stroke(Stroke::new(1.0, border_col))
                                    .inner_margin(egui::Margin::same(12.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&item.title)
                                                        .size(13.5)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                                ui.label(
                                                    RichText::new(&item.status_text)
                                                        .size(11.5)
                                                        .color(if item.is_failed {
                                                            Color32::LIGHT_RED
                                                        } else {
                                                            accent_green
                                                        })
                                                        .family(FontFamily::Monospace),
                                                );
                                            });
                                        });
                                    });
                                if rec_res.response.hovered() {
                                    ui.painter().rect_stroke(
                                        rec_res.response.rect,
                                        Rounding::same(10.0),
                                        Stroke::new(1.0, accent_green),
                                    );
                                }
                                ui.add_space(6.0);
                            }
                        });
                    }
                }

                Tab::Terminal => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Live Terminal Output")
                                .size(17.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Clear Logs").clicked() {
                                self.terminal_logs.clear();
                            }
                        });
                    });
                    ui.add_space(8.0);
                    egui::Frame::none()
                        .fill(input_bg)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, border_col))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for line in &self.terminal_logs {
                                        ui.label(
                                            RichText::new(line)
                                                .size(12.0)
                                                .family(FontFamily::Monospace)
                                                .color(Color32::from_rgb(200, 205, 210)),
                                        );
                                    }
                                });
                        });
                }

                Tab::Completed => {
                    ui.label(
                        RichText::new("Completed Downloads")
                            .size(17.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(8.0);
                    if self.completed_downloads.is_empty() {
                        ui.label(
                            RichText::new("No completed downloads yet.")
                                .size(13.0)
                                .color(muted_text),
                        );
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for item in &self.completed_downloads {
                                let comp_res = egui::Frame::none()
                                    .fill(card_bg)
                                    .rounding(Rounding::same(10.0))
                                    .stroke(Stroke::new(1.0, border_col))
                                    .inner_margin(egui::Margin::same(12.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(&item.title)
                                                .size(14.0)
                                                .strong()
                                                .color(Color32::WHITE),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "[{} · {}] {}",
                                                item.resolution, item.format, item.status_text
                                            ))
                                            .size(12.0)
                                            .color(accent_green)
                                            .family(FontFamily::Monospace),
                                        );
                                    });
                                if comp_res.response.hovered() {
                                    ui.painter().rect_stroke(
                                        comp_res.response.rect,
                                        Rounding::same(10.0),
                                        Stroke::new(1.0, accent_green),
                                    );
                                }
                                ui.add_space(6.0);
                            }
                        });
                    }
                }

                Tab::About => {
                    ui.horizontal(|ui| {
                        draw_pixel_5_icon(ui, 36.0, accent_green);
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("5KD - 5wi's Kick Downloader")
                                    .size(19.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(
                                    "High-performance Kick.com VOD & clip downloader written in Rust.",
                                )
                                .size(13.0)
                                .color(muted_text),
                            );
                        });
                    });

                    ui.add_space(16.0);

                    let about_card = egui::Frame::none()
                        .fill(card_bg)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, border_col))
                        .inner_margin(egui::Margin::same(16.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Architecture & Engine")
                                    .size(14.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "Built natively in Rust using eframe/egui, reqwest, rfd, yt-dlp, and ffmpeg for multi-threaded HLS stream downloading and lossless time slicing.",
                                )
                                .size(12.5)
                                .color(muted_text),
                            );
                        });
                    if about_card.response.hovered() {
                        ui.painter().rect_stroke(
                            about_card.response.rect,
                            Rounding::same(10.0),
                            Stroke::new(1.0, accent_green),
                        );
                    }

                    ui.add_space(12.0);

                    // Credit Card for mortyobnoxious/kickvod-extractor
                    let credit_card = egui::Frame::none()
                        .fill(card_bg)
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::new(1.0, border_col))
                        .inner_margin(egui::Margin::same(16.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Credits & Acknowledgments")
                                    .size(14.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "M3U8 URL Extractor logic powered by kickvod-extractor by mortyobnoxious:",
                                )
                                .size(12.5)
                                .color(muted_text),
                            );
                            ui.add_space(4.0);
                            ui.hyperlink("https://github.com/mortyobnoxious/kickvod-extractor");
                        });
                    if credit_card.response.hovered() {
                        ui.painter().rect_stroke(
                            credit_card.response.rect,
                            Rounding::same(10.0),
                            Stroke::new(1.0, accent_green),
                        );
                    }
                }
            });
    }
}

fn load_window_icon() -> Arc<egui::IconData> {
    let size = 64usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let mut is_green = false;
            if x >= 8 && x < 56 && y >= 5 && y < 59 {
                let gx = (x - 8) / 3;
                let gy = (y - 5) / 3;
                if gy < 18 && gx < 16 && ICON_5_GRID[gy].as_bytes()[gx] == b'#' {
                    is_green = true;
                }
            }
            if is_green {
                // #00e701 -> R: 0, G: 231, B: 1
                rgba[idx] = 0;
                rgba[idx + 1] = 231;
                rgba[idx + 2] = 1;
                rgba[idx + 3] = 255;
            } else {
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 255;
            }
        }
    }
    Arc::new(egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    })
}

fn main() -> eframe::Result<()> {
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
