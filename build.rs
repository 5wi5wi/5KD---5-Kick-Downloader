use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

// Exact 16x18 pixel grid matching /public/assets/aistudio/Gemini_Generated_Image_z2ceqoz2ceqoz2ce__1__ef8ae8c7.png
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

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let ico_path = out_dir.join("app_icon.ico");
        if let Ok(mut file) = File::create(&ico_path) {
            let _ = file.write_all(&generate_ico_bytes());
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico_path.to_str().unwrap());
            let _ = res.compile();
        }
    }
}
