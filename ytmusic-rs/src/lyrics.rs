use crate::innertube::Innertube;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TrackLyrics {
    pub synced: bool,
    pub lines: Vec<LyricLine>,
}

#[derive(Deserialize)]
struct LrcLibResp {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    duration: Option<f64>,
}

/// Fetch synced lyrics from LRCLIB (`/api/get` then fuzzy `/api/search`), falling back to YTM Innertube.
pub fn fetch_lyrics(
    innertube: &Innertube,
    video_id: &str,
    title: &str,
    artist: &str,
    duration_secs: u64,
) -> Option<TrackLyrics> {
    if let Some(lrc) = fetch_from_lrclib(title, artist, duration_secs) {
        return Some(lrc);
    }
    let plain = innertube.fetch_ytm_lyrics(video_id)?;
    let lines: Vec<LyricLine> = plain
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, l)| LyricLine {
            time_ms: (i as u64) * 4000,
            text: l.to_string(),
        })
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(TrackLyrics {
            synced: false,
            lines,
        })
    }
}

fn fetch_from_lrclib(title: &str, artist: &str, duration_secs: u64) -> Option<TrackLyrics> {
    let clean_title = clean_query_part(title);
    let clean_artist = clean_query_part(artist);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .build();

    // 1. Exact get
    let get_url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}{}",
        url_encode(&clean_artist),
        url_encode(&clean_title),
        if duration_secs > 0 {
            format!("&duration={duration_secs}")
        } else {
            String::new()
        }
    );
    if let Ok(resp) = agent.get(&get_url).set("User-Agent", "RustTune/0.1").call() {
        if let Ok(data) = resp.into_json::<LrcLibResp>() {
            if let Some(synced) = data.synced_lyrics {
                let parsed = parse_lrc(&synced);
                if !parsed.is_empty() {
                    return Some(TrackLyrics {
                        synced: true,
                        lines: parsed,
                    });
                }
            }
        }
    }

    // 2. Fuzzy search so minor title/artist formatting differences still find real-time synced .lrc
    let search_q = format!("{clean_artist} {clean_title}");
    let search_url = format!("https://lrclib.net/api/search?q={}", url_encode(&search_q));
    if let Ok(resp) = agent.get(&search_url).set("User-Agent", "RustTune/0.1").call() {
        if let Ok(items) = resp.into_json::<Vec<LrcLibResp>>() {
            let best_synced = items
                .iter()
                .filter_map(|item| {
                    let s = item.synced_lyrics.as_deref()?;
                    let parsed = parse_lrc(s);
                    if parsed.is_empty() {
                        return None;
                    }
                    let diff = if duration_secs > 0 {
                        item.duration
                            .map(|d| (d - duration_secs as f64).abs() as u64)
                            .unwrap_or(10)
                    } else {
                        0
                    };
                    Some((diff, parsed))
                })
                .min_by_key(|(diff, _)| *diff);
            if let Some((_, lines)) = best_synced {
                return Some(TrackLyrics {
                    synced: true,
                    lines,
                });
            }

            if let Some(plain) = items.into_iter().find_map(|i| i.plain_lyrics) {
                let lines: Vec<LyricLine> = plain
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .enumerate()
                    .map(|(i, l)| LyricLine {
                        time_ms: (i as u64) * 4000,
                        text: l.to_string(),
                    })
                    .collect();
                if !lines.is_empty() {
                    return Some(TrackLyrics {
                        synced: false,
                        lines,
                    });
                }
            }
        }
    }

    None
}

pub fn parse_lrc(raw: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if !line.starts_with('[') {
            continue;
        }
        let Some(close) = line.find(']') else {
            continue;
        };
        let tag = &line[1..close];
        let text = line[close + 1..].trim();
        if text.is_empty() {
            continue;
        }
        if let Some(ms) = parse_lrc_timestamp(tag) {
            out.push(LyricLine {
                time_ms: ms,
                text: text.to_string(),
            });
        }
    }
    out.sort_by_key(|l| l.time_ms);
    out
}

fn parse_lrc_timestamp(tag: &str) -> Option<u64> {
    let (mins_s, rest) = tag.split_once(':')?;
    let mins: u64 = mins_s.trim().parse().ok()?;
    let (secs_s, frac_s) = rest.split_once('.').unwrap_or((rest, "0"));
    let secs: u64 = secs_s.trim().parse().ok()?;
    let frac_trimmed = &frac_s.trim()[..frac_s.trim().len().min(3)];
    let frac_val: u64 = frac_trimmed.parse().ok()?;
    let ms = match frac_trimmed.len() {
        1 => frac_val * 100,
        2 => frac_val * 10,
        _ => frac_val,
    };
    Some(mins * 60_000 + secs * 1_000 + ms)
}

fn clean_query_part(s: &str) -> String {
    let mut out = s.replace(" - Topic", "");
    for marker in [
        "(feat.",
        "(ft.",
        "[feat.",
        "(Official",
        "[Official",
        "(Audio",
        "[Audio",
        "(Lyrics",
        "[Lyrics",
        "(Music Video",
    ] {
        if let Some(i) = out.to_lowercase().find(&marker.to_lowercase()) {
            out.truncate(i);
        }
    }
    out.trim().to_string()
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_timestamps_and_lines() {
        let lrc = "[00:12.50] First line\n[01:05.123] Second line\n[ar:Artist]\n[01:10] Third";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].time_ms, 12_500);
        assert_eq!(lines[0].text, "First line");
        assert_eq!(lines[1].time_ms, 65_123);
        assert_eq!(lines[2].time_ms, 70_000);
    }

    #[test]
    fn cleans_topic_and_official_tags() {
        assert_eq!(clean_query_part("M83 - Topic"), "M83");
        assert_eq!(clean_query_part("Midnight City (Official Audio)"), "Midnight City");
    }
}
