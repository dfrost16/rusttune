use slint::{Color, Image, Rgba8Pixel, SharedPixelBuffer};
use std::path::PathBuf;

pub fn resolve_base_dir() -> PathBuf {
    for var in ["RUSTTUNE_BASE_DIR", "YTMUSIC_BASE_DIR"] {
        if let Ok(d) = std::env::var(var) {
            let p = PathBuf::from(d);
            if p.exists() {
                return p;
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("config").join("clients.json").is_file() {
                return dir.to_path_buf();
            }
            for anc in dir.ancestors() {
                let candidate = anc.join("port");
                if candidate.join("config").join("clients.json").is_file() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from(".")
}

pub fn build_dot_grid_image(dark: bool) -> Image {
    let w = 640u32;
    let h = 480u32;
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let bg = if dark { [14, 16, 20, 255] } else { [243, 244, 246, 255] };
    let dot = if dark { [32, 36, 44, 255] } else { [208, 213, 221, 255] };
    let bytes = buf.make_mut_bytes();
    for y in 0..h {
        for x in 0..w {
            let is_dot = x % 14 == 7 && y % 14 == 7;
            let px = if is_dot { dot } else { bg };
            let off = ((y * w + x) * 4) as usize;
            bytes[off..off + 4].copy_from_slice(&px);
        }
    }
    Image::from_rgba8(buf)
}

pub fn read_battery_label() -> String {
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let cap_path = entry.path().join("capacity");
            if let Ok(txt) = std::fs::read_to_string(cap_path) {
                if let Ok(pct) = txt.trim().parse::<u32>() {
                    return format!("{}%", pct.min(100));
                }
            }
        }
    }
    "AC".to_string()
}

pub fn parse_battery_pct(label: &str) -> f32 {
    label
        .trim_end_matches('%')
        .trim()
        .parse::<f32>()
        .map(|v| (v / 100.0).clamp(0.0, 1.0))
        .unwrap_or(1.0)
}

pub fn local_clock_digits() -> [String; 4] {
    let s = chrono::Local::now().format("%H%M").to_string();
    let b = s.as_bytes();
    if b.len() == 4 {
        [
            (b[0] as char).to_string(),
            (b[1] as char).to_string(),
            (b[2] as char).to_string(),
            (b[3] as char).to_string(),
        ]
    } else {
        ["0".into(), "0".into(), "0".into(), "0".into()]
    }
}

pub fn fmt_time(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn color_from_rgb(rgb: (u8, u8, u8)) -> Color {
    Color::from_rgb_u8(rgb.0, rgb.1, rgb.2)
}

/// Extract the dominant vibrant hue cluster from decoded RGBA pixels using 12 hue buckets.
pub fn extract_cover_accent(rgba: &[u8], w: u32, h: u32) -> Option<(u8, u8, u8)> {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return None;
    }
    let step_x = (w / 16).max(1);
    let step_y = (h / 16).max(1);
    let mut bins = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 12];

    for y in (0..h).step_by(step_y as usize) {
        for x in (0..w).step_by(step_x as usize) {
            let idx = ((y * w + x) * 4) as usize;
            let r = rgba[idx] as f32;
            let g = rgba[idx + 1] as f32;
            let b = rgba[idx + 2] as f32;
            if rgba[idx + 3] < 128 {
                continue;
            }
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            let chroma = max_c - min_c;
            if max_c < 28.0 || chroma < 24.0 {
                continue;
            }
            let mut hue = if (max_c - r).abs() < 0.1 {
                ((g - b) / chroma).rem_euclid(6.0)
            } else if (max_c - g).abs() < 0.1 {
                (b - r) / chroma + 2.0
            } else {
                (r - g) / chroma + 4.0
            };
            hue = (hue * 2.0).clamp(0.0, 11.999);
            let bin = hue as usize;
            let lum = 0.299 * r + 0.587 * g + 0.114 * b;
            let lum_w = if lum < 30.0 || lum > 240.0 { 0.3 } else { 1.0 };
            let sat = chroma / max_c.max(1.0);
            let wgt = sat * sat * (chroma / 255.0) * lum_w + 0.02;
            bins[bin].0 += r * wgt;
            bins[bin].1 += g * wgt;
            bins[bin].2 += b * wgt;
            bins[bin].3 += wgt;
        }
    }

    let best = bins
        .iter()
        .max_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))?;
    if best.3 < 0.05 {
        return None;
    }
    Some((
        (best.0 / best.3).clamp(0.0, 255.0) as u8,
        (best.1 / best.3).clamp(0.0, 255.0) as u8,
        (best.2 / best.3).clamp(0.0, 255.0) as u8,
    ))
}

/// Mask an RGBA buffer into a clean circular avatar for Slint software renderer.
pub fn mask_circle_rgba(src: &[u8], w: u32, h: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let dst = buf.make_mut_bytes();
    let cx = (w as f32) * 0.5;
    let cy = (h as f32) * 0.5;
    let r = cx.min(cy) - 0.5;
    for y in 0..h {
        for x in 0..w {
            let off = ((y * w + x) * 4) as usize;
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha_mul = ((r - dist + 0.75).clamp(0.0, 1.5)) / 1.5;
            dst[off] = src[off];
            dst[off + 1] = src[off + 1];
            dst[off + 2] = src[off + 2];
            dst[off + 3] = ((src[off + 3] as f32) * alpha_mul) as u8;
        }
    }
    Image::from_rgba8(buf)
}

/// Mask an RGBA buffer with soft rounded corners so full-bleed cards clip crisply.
pub fn mask_rounded_rgba(src: &[u8], w: u32, h: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let dst = buf.make_mut_bytes();
    let rad = ((w.min(h) as f32) * 0.085).max(4.0);
    let fw = w as f32;
    let fh = h as f32;
    for y in 0..h {
        for x in 0..w {
            let off = ((y * w + x) * 4) as usize;
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let cx = px.clamp(rad, fw - rad);
            let cy = py.clamp(rad, fh - rad);
            let dx = px - cx;
            let dy = py - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha_mul = ((rad - dist + 0.75).clamp(0.0, 1.5)) / 1.5;
            dst[off] = src[off];
            dst[off + 1] = src[off + 1];
            dst[off + 2] = src[off + 2];
            dst[off + 3] = ((src[off + 3] as f32) * alpha_mul) as u8;
        }
    }
    Image::from_rgba8(buf)
}

/// Adjust a cover color for strong contrast in Dark or Light theme and pick readable foreground text color.
pub fn theme_accent_pair(rgb: (u8, u8, u8), light_mode: bool) -> ((u8, u8, u8), (u8, u8, u8)) {
    let (mut r, mut g, mut b) = (rgb.0 as f32, rgb.1 as f32, rgb.2 as f32);
    let max_c = r.max(g).max(b).max(1.0);
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;

    if light_mode {
        if lum > 155.0 {
            let scale = 140.0 / lum;
            r *= scale;
            g *= scale;
            b *= scale;
        }
    } else if lum < 115.0 {
        let boost = (155.0 / lum.max(28.0)).min(220.0 / max_c);
        r = (r * boost).min(255.0);
        g = (g * boost).min(255.0);
        b = (b * boost).min(255.0);
        let lum2 = 0.299 * r + 0.587 * g + 0.114 * b;
        if lum2 < 110.0 {
            let mix = ((115.0 - lum2) / 255.0).clamp(0.0, 0.55);
            r += (255.0 - r) * mix;
            g += (255.0 - g) * mix;
            b += (255.0 - b) * mix;
        }
    }

    let out = (r as u8, g as u8, b as u8);
    let out_lum = 0.299 * r + 0.587 * g + 0.114 * b;
    let fg = if out_lum > 148.0 { (15, 23, 42) } else { (255, 255, 255) };
    (out, fg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cover_accent_and_contrasting_fg() {
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for _ in 0..(16 * 16) {
            rgba.extend_from_slice(&[220, 45, 95, 255]);
        }
        let rgb = extract_cover_accent(&rgba, 16, 16).expect("vibrant pink/red accent");
        assert!(rgb.0 > 180 && rgb.2 > 60);

        let (dark_acc, dark_fg) = theme_accent_pair((40, 20, 120), false);
        assert!(0.299 * dark_acc.0 as f32 + 0.587 * dark_acc.1 as f32 + 0.114 * dark_acc.2 as f32 > 90.0);
        assert!(dark_fg == (255, 255, 255) || dark_fg == (15, 23, 42));

        let gray: Vec<u8> = [120u8, 120, 120, 255].repeat(16 * 16);
        assert!(extract_cover_accent(&gray, 16, 16).is_none());
    }

    #[test]
    fn parses_battery_pct_and_formats_clock_digits() {
        assert!((parse_battery_pct("84%") - 0.84).abs() < 0.001);
        assert!((parse_battery_pct("AC") - 1.0).abs() < 0.001);
        assert!((parse_battery_pct("150%") - 1.0).abs() < 0.001);

        let digits = local_clock_digits();
        assert_eq!(digits.len(), 4);
        assert!(digits.iter().all(|d| d.len() == 1 && d.chars().all(|c| c.is_ascii_digit())));
    }
}
