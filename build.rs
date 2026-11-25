use std::env;
use std::process::Command;

#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::path::PathBuf;

// Normalized polyline matching the in-app geometric brand mark in src/main.rs (`draw_brand_mark`)
#[cfg(windows)]
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

#[cfg(windows)]
fn sd_rounded_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

#[cfg(windows)]
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

#[cfg(windows)]
fn sample_brand_icon_pixel(x: u32, y: u32, size: u32) -> [u8; 4] {
    const SAMPLES: u32 = 4;
    let size_f = size as f32;
    let border_w = 0.042_f32.max(1.25 / size_f);
    let half_border = border_w * 0.5;
    let margin = 0.015_f32;
    let half_box = 0.5 - margin - half_border;
    let corner_r = (0.21875 - half_border).max(0.02);
    let half_five = (0.105_f32 * 0.5).max(0.85 / size_f);

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

    if sum_a <= 0.0 {
        return [0, 0, 0, 0];
    }

    let r = (sum_r / sum_a).round().clamp(0.0, 255.0) as u8;
    let g = (sum_g / sum_a).round().clamp(0.0, 255.0) as u8;
    let b = (sum_b / sum_a).round().clamp(0.0, 255.0) as u8;
    let a = ((sum_a / (SAMPLES * SAMPLES) as f32) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    [r, g, b, a]
}

#[cfg(windows)]
fn generate_ico_bytes() -> Vec<u8> {
    const SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];
    let bpp: u16 = 32;

    let mut image_sizes = Vec::with_capacity(SIZES.len());
    let mut total_data_size = 0usize;
    for &size in &SIZES {
        let xor_size = (size * size * 4) as usize;
        let and_row_bytes = ((size + 31) / 32 * 4) as usize;
        let and_size = and_row_bytes * (size as usize);
        let img_size = 40 + xor_size + and_size;
        image_sizes.push((size, xor_size, and_row_bytes, and_size, img_size));
        total_data_size += img_size;
    }

    let header_size = 6 + 16 * SIZES.len();
    let mut buf = vec![0u8; header_size + total_data_size];

    // 1. ICONDIR Header
    buf[2..4].copy_from_slice(&1u16.to_le_bytes()); // Type 1 = ICO
    buf[4..6].copy_from_slice(&(SIZES.len() as u16).to_le_bytes());

    let mut current_offset = header_size;
    for (idx, &(size, xor_size, and_row_bytes, and_size, img_size)) in image_sizes.iter().enumerate()
    {
        // 2. ICONDIRENTRY
        let entry = 6 + idx * 16;
        let dim_byte = if size >= 256 { 0u8 } else { size as u8 };
        buf[entry] = dim_byte;
        buf[entry + 1] = dim_byte;
        buf[entry + 4..entry + 6].copy_from_slice(&1u16.to_le_bytes()); // Color planes
        buf[entry + 6..entry + 8].copy_from_slice(&bpp.to_le_bytes()); // BPP
        buf[entry + 8..entry + 12].copy_from_slice(&(img_size as u32).to_le_bytes());
        buf[entry + 12..entry + 16].copy_from_slice(&(current_offset as u32).to_le_bytes());

        // 3. BITMAPINFOHEADER
        let offset = current_offset;
        buf[offset..offset + 4].copy_from_slice(&40u32.to_le_bytes());
        buf[offset + 4..offset + 8].copy_from_slice(&(size as i32).to_le_bytes());
        buf[offset + 8..offset + 12].copy_from_slice(&((size * 2) as i32).to_le_bytes());
        buf[offset + 12..offset + 14].copy_from_slice(&1u16.to_le_bytes());
        buf[offset + 14..offset + 16].copy_from_slice(&bpp.to_le_bytes());
        buf[offset + 20..offset + 24]
            .copy_from_slice(&((xor_size + and_size) as u32).to_le_bytes());

        // 4. XOR Mask (BGRA bottom-to-top) + 1-bit AND Mask
        let pixel_offset = offset + 40;
        let and_offset = pixel_offset + xor_size;
        for y in 0..size {
            let bmp_y = size - 1 - y;
            for x in 0..size {
                let [r, g, b, a] = sample_brand_icon_pixel(x, y, size);
                let p_idx = pixel_offset + ((bmp_y * size + x) * 4) as usize;
                buf[p_idx] = b;
                buf[p_idx + 1] = g;
                buf[p_idx + 2] = r;
                buf[p_idx + 3] = a;

                if a == 0 {
                    let and_idx = and_offset + (bmp_y as usize) * and_row_bytes + (x / 8) as usize;
                    buf[and_idx] |= 1u8 << (7 - (x % 8));
                }
            }
        }

        current_offset += img_size;
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
