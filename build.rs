use std::env;
use std::process::Command;

#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::path::PathBuf;

// Exact 16x18 pixel grid matching /public/assets/aistudio/Gemini_Generated_Image_z2ceqoz2ceqoz2ce__1__ef8ae8c7.png
#[cfg(windows)]
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

#[cfg(windows)]
fn generate_ico_bytes() -> Vec<u8> {
    let icon_size: u32 = 64;
    let bpp: u16 = 32;
    let xor_size = (icon_size * icon_size * 4) as usize;
    let and_row_bytes = ((icon_size + 31) / 32 * 4) as usize;
    let and_size = and_row_bytes * (icon_size as usize);
    let image_size = 40 + xor_size + and_size;
    let total_size = 6 + 16 + image_size;

    let mut buf = vec![0u8; total_size];

    // 1. ICONDIR Header
    buf[2..4].copy_from_slice(&1u16.to_le_bytes()); // Type 1 = ICO
    buf[4..6].copy_from_slice(&1u16.to_le_bytes()); // 1 image

    // 2. ICONDIRENTRY
    buf[6] = icon_size as u8;
    buf[7] = icon_size as u8;
    buf[10..12].copy_from_slice(&1u16.to_le_bytes()); // Color planes
    buf[12..14].copy_from_slice(&bpp.to_le_bytes()); // BPP
    buf[14..18].copy_from_slice(&(image_size as u32).to_le_bytes());
    buf[18..22].copy_from_slice(&22u32.to_le_bytes());

    // 3. BITMAPINFOHEADER
    let offset = 22;
    buf[offset..offset + 4].copy_from_slice(&40u32.to_le_bytes());
    buf[offset + 4..offset + 8].copy_from_slice(&(icon_size as i32).to_le_bytes());
    buf[offset + 8..offset + 12].copy_from_slice(&((icon_size * 2) as i32).to_le_bytes());
    buf[offset + 12..offset + 14].copy_from_slice(&1u16.to_le_bytes());
    buf[offset + 14..offset + 16].copy_from_slice(&bpp.to_le_bytes());
    buf[offset + 20..offset + 24].copy_from_slice(&((xor_size + and_size) as u32).to_le_bytes());

    // 4. XOR Mask (BGRA bottom-to-top)
    let pixel_offset = offset + 40;
    for y in 0..icon_size {
        let bmp_y = icon_size - 1 - y;
        for x in 0..icon_size {
            let p_idx = pixel_offset + ((bmp_y * icon_size + x) * 4) as usize;
            let mut is_green = false;
            if x >= 8 && x < 56 && y >= 5 && y < 59 {
                let gx = ((x - 8) / 3) as usize;
                let gy = ((y - 5) / 3) as usize;
                if gy < 18 && gx < 16 && ICON_5_GRID[gy].as_bytes()[gx] == b'#' {
                    is_green = true;
                }
            }
            if is_green {
                // #00e701 -> R: 0, G: 231, B: 1
                buf[p_idx] = 1;       // B
                buf[p_idx + 1] = 231; // G
                buf[p_idx + 2] = 0;   // R
                buf[p_idx + 3] = 255; // A
            } else {
                buf[p_idx] = 0;
                buf[p_idx + 1] = 0;
                buf[p_idx + 2] = 0;
                buf[p_idx + 3] = 255;
            }
        }
    }

    buf
}

fn parse_github_repo_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        rest
    } else {
        return None;
    };

    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if !owner.is_empty() && !repo.is_empty() {
        Some(format!("{}/{}", owner, repo))
    } else {
        None
    }
}

fn detect_github_repo() -> String {
    for key in ["FIVEKD_GITHUB_REPO", "GITHUB_REPOSITORY"] {
        if let Ok(val) = env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() && trimmed.contains('/') {
                return trimmed.to_string();
            }
        }
    }

    if let Ok(output) = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()
    {
        if output.status.success() {
            let remote = String::from_utf8_lossy(&output.stdout);
            if let Some(repo) = parse_github_repo_from_url(&remote) {
                return repo;
            }
        }
    }

    String::new()
}

fn detect_app_version() -> String {
    if let Ok(tag) = env::var("FIVEKD_RELEASE_TAG") {
        let trimmed = tag.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".to_string())
}

fn detect_installer_asset() -> String {
    if let Ok(asset) = env::var("FIVEKD_INSTALLER_ASSET") {
        let trimmed = asset.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    match (os.as_str(), arch.as_str()) {
        ("windows", "aarch64") => "5KD-windows-arm64-setup.exe".to_string(),
        ("windows", _) => "5KD-windows-x86_64-setup.exe".to_string(),
        ("macos", "aarch64") => "5KD-macos-arm64.tar.gz".to_string(),
        ("macos", _) => "5KD-macos-x86_64.tar.gz".to_string(),
        _ => "5KD-linux-x86_64.tar.gz".to_string(),
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=FIVEKD_GITHUB_REPO");
    println!("cargo:rerun-if-env-changed=GITHUB_REPOSITORY");
    println!("cargo:rerun-if-env-changed=FIVEKD_RELEASE_TAG");
    println!("cargo:rerun-if-env-changed=FIVEKD_INSTALLER_ASSET");

    println!("cargo:rustc-env=FIVEKD_GITHUB_REPO={}", detect_github_repo());
    println!("cargo:rustc-env=FIVEKD_APP_VERSION={}", detect_app_version());
    println!(
        "cargo:rustc-env=FIVEKD_INSTALLER_ASSET={}",
        detect_installer_asset()
    );

    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let ico_bytes = generate_ico_bytes();
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let ico_path = out_dir.join("app_icon.ico");
        if let Ok(mut file) = File::create(&ico_path) {
            let _ = file.write_all(&ico_bytes);
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico_path.to_str().unwrap());
            let _ = res.compile();
        }
        if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
            let target_dir = PathBuf::from(manifest_dir).join("target");
            let _ = std::fs::create_dir_all(&target_dir);
            if let Ok(mut target_ico) = File::create(target_dir.join("app_icon.ico")) {
                let _ = target_ico.write_all(&ico_bytes);
            }
        }
    }
}
