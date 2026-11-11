use crate::db::TrackMeta;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct ClientFingerprint {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[allow(dead_code)]
    pub innertube_client_name: String,
    pub innertube_client_version: String,
    pub client_name_id: i64,
    pub user_agent: String,
    pub context: Value,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientsConfig {
    #[serde(default)]
    pub api_key: String,
    pub music_client: ClientFingerprint,
    pub player_clients: Vec<ClientFingerprint>,
}

#[derive(Debug, Clone)]
pub struct YtmVibe {
    pub id: String,
    pub title: String,
    pub browse_id: String,
    pub params: String,
    pub rgb: (u8, u8, u8),
}

#[derive(Debug, Clone)]
pub struct YtmArtist {
    pub name: String,
    pub subtitle: String,
    pub browse_id: String,
    pub thumb_url: String,
    pub rgb: (u8, u8, u8),
}

#[derive(Debug, Clone)]
pub struct YtmAlbum {
    pub title: String,
    pub artist: String,
    pub subtitle: String,
    pub browse_id: String,
    pub thumb_url: String,
    pub rgb: (u8, u8, u8),
}

#[derive(Debug, Clone)]
pub struct YtmArtistPage {
    pub subtitle: String,
    pub thumb_url: String,
    pub songs: Vec<TrackMeta>,
    pub albums: Vec<YtmAlbum>,
    pub singles: Vec<YtmAlbum>,
}

#[derive(Debug, Clone)]
pub struct ResolvedAudio {
    pub url: String,
    pub user_agent: String,
    pub bitrate: u64,
    #[allow(dead_code)]
    pub mime_type: String,
    pub title: String,
    pub artist: String,
    pub duration_secs: u64,
    pub content_length: Option<u64>,
}

pub struct Innertube {
    cfg: ClientsConfig,
    agent: ureq::Agent,
    visitor: Mutex<(String, Option<Instant>)>,
}

impl Innertube {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed reading {}: {e}", path.display()))?;
        let cfg: ClientsConfig =
            serde_json::from_str(&raw).map_err(|e| format!("Invalid clients.json: {e}"))?;
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout_read(Duration::from_secs(15))
            .build();
        Ok(Self {
            cfg,
            agent,
            visitor: Mutex::new((String::new(), None)),
        })
    }

    pub fn ensure_visitor_data(&self, force_refresh: bool) -> String {
        let mut guard = self.visitor.lock().unwrap();
        let expired = guard
            .1
            .map(|t| t.elapsed() > Duration::from_secs(3600))
            .unwrap_or(true);
        if !force_refresh && !guard.0.is_empty() && !expired {
            return guard.0.clone();
        }
        let fp = &self.cfg.music_client;
        let body = json!({ "context": { "client": fp.context } });
        let url = self.endpoint("https://www.youtube.com/youtubei/v1/visitor_id");
        if let Ok(resp) = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("User-Agent", &fp.user_agent)
            .send_json(body)
        {
            if let Ok(v) = resp.into_json::<Value>() {
                if let Some(vd) = v
                    .pointer("/responseContext/visitorData")
                    .and_then(|x| x.as_str())
                {
                    guard.0 = vd.to_string();
                    guard.1 = Some(Instant::now());
                }
            }
        }
        guard.0.clone()
    }

    fn endpoint(&self, base: &str) -> String {
        if self.cfg.api_key.is_empty() {
            format!("{base}?prettyPrint=false")
        } else {
            format!("{base}?key={}&prettyPrint=false", self.cfg.api_key)
        }
    }

    fn post_music(&self, path: &str, mut body: Value) -> Result<Value, String> {
        let vd = self.ensure_visitor_data(false);
        let fp = &self.cfg.music_client;
        if let Some(obj) = body.as_object_mut() {
            obj.insert("context".into(), json!({ "client": fp.context }));
        }
        let url = self.endpoint(&format!("https://music.youtube.com/youtubei/v1/{path}"));
        let mut req = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("User-Agent", &fp.user_agent)
            .set("Origin", "https://music.youtube.com")
            .set("Referer", "https://music.youtube.com/")
            .set("X-YouTube-Client-Name", &fp.client_name_id.to_string())
            .set("X-YouTube-Client-Version", &fp.innertube_client_version);
        if !vd.is_empty() {
            req = req.set("X-Goog-Visitor-Id", &vd);
        }
        let resp = req.send_json(body).map_err(|e| format!("Innertube {path}: {e}"))?;
        resp.into_json::<Value>()
            .map_err(|e| format!("Innertube {path} JSON: {e}"))
    }

    /// Search YouTube Music songs via WEB_REMIX (`params` filters to Songs when possible).
    pub fn search_songs(&self, query: &str) -> Result<Vec<TrackMeta>, String> {
        let body = json!({
            "query": query,
            "params": "EgWKAQIIAWoKEAMQBBAJEAoQBQ%3D%3D"
        });
        let val = self.post_music("search", body)?;
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_music_renderers(&val, &mut tracks, &mut seen);
        if tracks.is_empty() {
            let val2 = self.post_music("search", json!({ "query": query }))?;
            collect_music_renderers(&val2, &mut tracks, &mut seen);
        }
        Ok(tracks)
    }

    /// Search YouTube Music artists via WEB_REMIX.
    pub fn search_artists(&self, query: &str) -> Result<Vec<YtmArtist>, String> {
        let body = json!({
            "query": query,
            "params": "EgWKAQIgAWoKEAMQBBAJEAoQBQ%3D%3D"
        });
        let val = self.post_music("search", body)?;
        let mut artists = Vec::new();
        let mut seen = HashSet::new();
        collect_artist_items(&val, &mut artists, &mut seen);
        Ok(artists)
    }

    /// Search YouTube Music albums via WEB_REMIX.
    pub fn search_albums(&self, query: &str) -> Result<Vec<YtmAlbum>, String> {
        let body = json!({
            "query": query,
            "params": "EgWKAQIYAWoKEAMQBBAJEAoQBQ%3D%3D"
        });
        let val = self.post_music("search", body)?;
        let mut albums = Vec::new();
        let mut seen = HashSet::new();
        collect_album_items(&val, &mut albums, &mut seen);
        Ok(albums)
    }

    /// Fetch tracks for an album browseId (`MPREb_*` / `OLAK*`).
    pub fn fetch_album_tracks(
        &self,
        browse_id: &str,
        album_title: &str,
        album_artist: &str,
    ) -> Vec<TrackMeta> {
        let Ok(val) = self.post_music("browse", json!({ "browseId": browse_id })) else {
            return Vec::new();
        };
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_music_renderers(&val, &mut tracks, &mut seen);
        for t in &mut tracks {
            if t.album.is_empty() && !album_title.is_empty() {
                t.album = album_title.to_string();
            }
            if (t.artist.is_empty() || t.artist == "YouTube Music") && !album_artist.is_empty() {
                t.artist = album_artist.to_string();
            }
        }
        tracks
    }

    /// Fetch real YouTube Music moods & genres (`FEmusic_moods_and_genres`).
    pub fn fetch_moods_and_genres(&self) -> Vec<YtmVibe> {
        let Ok(val) = self.post_music("browse", json!({ "browseId": "FEmusic_moods_and_genres" })) else {
            return Vec::new();
        };
        let mut vibes = Vec::new();
        let mut seen = HashSet::new();
        collect_vibe_buttons(&val, &mut vibes, &mut seen);
        vibes
    }

    /// Fetch tracks for a YouTube Music mood/genre category by browsing its top playlist.
    pub fn fetch_vibe_tracks(&self, browse_id: &str, params: &str) -> Vec<TrackMeta> {
        let body = if params.is_empty() {
            json!({ "browseId": browse_id })
        } else {
            json!({ "browseId": browse_id, "params": params })
        };
        let Ok(val) = self.post_music("browse", body) else {
            return Vec::new();
        };
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_music_renderers(&val, &mut tracks, &mut seen);
        if tracks.len() >= 10 {
            return tracks;
        }
        if let Some(pl_id) = find_first_playlist_browse_id(&val) {
            if let Ok(pl_tracks) = self.fetch_playlist(&pl_id) {
                if pl_tracks.len() > tracks.len() {
                    return pl_tracks;
                }
            }
        }
        tracks
    }

    /// Fetch real Top Artists from YouTube Music Charts (`FEmusic_charts`).
    pub fn fetch_chart_artists(&self) -> Vec<YtmArtist> {
        let Ok(val) = self.post_music("browse", json!({ "browseId": "FEmusic_charts" })) else {
            return Vec::new();
        };
        let mut artists = Vec::new();
        let mut seen = HashSet::new();
        collect_artist_items(&val, &mut artists, &mut seen);
        artists
    }

    /// Fetch full Artist Page (header stats, Top Songs, Albums, Singles & EPs) with search fallback.
    pub fn fetch_artist_page(&self, browse_id: &str, artist_name: &str) -> YtmArtistPage {
        let mut subtitle = String::new();
        let mut thumb_url = String::new();
        let mut songs = Vec::new();
        let mut albums = Vec::new();
        let mut singles = Vec::new();
        let mut top_songs_pl: Option<String> = None;

        if browse_id.starts_with("UC") {
            if let Ok(val) = self.post_music("browse", json!({ "browseId": browse_id })) {
                if let Some(hdr) = val.get("header").and_then(|x| x.as_object()) {
                    for hv in hdr.values() {
                        if subtitle.is_empty() {
                            if let Some(s) = runs_text(hv.pointer("/subscriptionButton/subscribeButtonRenderer/subscriberCountText")) {
                                subtitle = format!("{s} subscribers");
                            }
                        }
                        if thumb_url.is_empty() {
                            if let Some(arr) = hv.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails").and_then(|x| x.as_array()) {
                                if let Some(u) = arr.first().and_then(|t| t.get("url")).and_then(|u| u.as_str()) {
                                    thumb_url = u.to_string();
                                }
                            }
                        }
                    }
                }

                if let Some(sections) = val
                    .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")
                    .and_then(|x| x.as_array())
                {
                    let mut seen_songs = HashSet::new();
                    let mut seen_albums = HashSet::new();
                    let mut seen_singles = HashSet::new();
                    for sec in sections {
                        if let Some(ms) = sec.get("musicShelfRenderer") {
                            let shelf_title = runs_text(ms.get("title")).unwrap_or_default().to_lowercase();
                            if shelf_title.contains("song") || songs.is_empty() {
                                if top_songs_pl.is_none() {
                                    top_songs_pl = ms
                                        .pointer("/bottomEndpoint/browseEndpoint/browseId")
                                        .and_then(|x| x.as_str())
                                        .map(|s| s.to_string());
                                }
                                collect_music_renderers(ms, &mut songs, &mut seen_songs);
                            }
                        }
                        if let Some(cs) = sec.get("musicCarouselShelfRenderer") {
                            let shelf_title = runs_text(cs.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"))
                                .unwrap_or_default()
                                .to_lowercase();
                            if shelf_title.contains("album") {
                                collect_two_row_albums(cs, artist_name, &mut albums, &mut seen_albums);
                            } else if shelf_title.contains("single") || shelf_title.contains("ep") {
                                collect_two_row_albums(cs, artist_name, &mut singles, &mut seen_singles);
                            }
                        }
                    }
                }
            }
        }

        if let Some(pl_id) = top_songs_pl {
            if let Ok(pl_tracks) = self.fetch_playlist(&pl_id) {
                if pl_tracks.len() > songs.len() {
                    songs = pl_tracks;
                }
            }
        }
        if songs.is_empty() {
            songs = self.search_songs(artist_name).unwrap_or_default();
        }
        if albums.is_empty() {
            albums = self.search_albums(artist_name).unwrap_or_default();
        }

        YtmArtistPage {
            subtitle,
            thumb_url,
            songs,
            albums,
            singles,
        }
    }

    /// Fetch Quick Picks / Home feed from YouTube Music, falling back to curated searches.
    pub fn quick_picks(&self) -> Vec<TrackMeta> {
        if let Ok(val) = self.post_music("browse", json!({ "browseId": "FEmusic_home" })) {
            let mut tracks = Vec::new();
            let mut seen = HashSet::new();
            collect_music_renderers(&val, &mut tracks, &mut seen);
            if !tracks.is_empty() {
                tracks.truncate(24);
                return tracks;
            }
        }
        self.search_songs("top synthwave lofi electronic").unwrap_or_default()
    }

    /// Fetch "Up Next" radio queue for a given video_id.
    pub fn up_next_radio(&self, video_id: &str) -> Vec<TrackMeta> {
        let body = json!({
            "videoId": video_id,
            "playlistId": format!("RDAMVM{video_id}"),
            "isAudioOnly": true
        });
        let Ok(val) = self.post_music("next", body) else {
            return Vec::new();
        };
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        seen.insert(video_id.to_string());
        collect_playlist_panel(&val, &mut tracks, &mut seen);
        if tracks.is_empty() {
            collect_music_renderers(&val, &mut tracks, &mut seen);
        }
        tracks.truncate(25);
        tracks
    }

    /// Fetch tracks (plus header title when available) from a YouTube / YT Music playlist ID.
    pub fn fetch_playlist_titled(&self, playlist_id: &str) -> Result<(String, Vec<TrackMeta>), String> {
        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id.to_string()
        } else {
            format!("VL{playlist_id}")
        };
        let val = self.post_music("browse", json!({ "browseId": browse_id }))?;
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_music_renderers(&val, &mut tracks, &mut seen);
        if tracks.is_empty() {
            collect_playlist_panel(&val, &mut tracks, &mut seen);
        }
        let title = find_playlist_header_title(&val).unwrap_or_else(|| "Imported Playlist".into());
        Ok((title, tracks))
    }

    /// Fetch tracks from a YouTube / YouTube Music playlist ID.
    pub fn fetch_playlist(&self, playlist_id: &str) -> Result<Vec<TrackMeta>, String> {
        self.fetch_playlist_titled(playlist_id).map(|(_, tracks)| tracks)
    }

    /// Fetch plain lyrics from YouTube Music's own Lyrics tab (`MPLYt_*`).
    pub fn fetch_ytm_lyrics(&self, video_id: &str) -> Option<String> {
        let val = self
            .post_music(
                "next",
                json!({
                    "videoId": video_id,
                    "isAudioOnly": true
                }),
            )
            .ok()?;
        let browse_id = find_lyrics_browse_id(&val)?;
        let br = self
            .post_music("browse", json!({ "browseId": browse_id }))
            .ok()?;
        find_description_text(&br)
    }

    /// Fetch live search autocomplete suggestions from YouTube Music.
    pub fn search_suggestions(&self, input: &str) -> Vec<String> {
        let q = input.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let Ok(val) = self.post_music("music/get_search_suggestions", json!({ "input": q })) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        collect_suggestions(&val, &mut out);
        out.truncate(5);
        out
    }

    /// Resolve direct unciphered AAC M4A audio stream URL for playback or offline caching.
    pub fn resolve_audio_stream(&self, video_id: &str, force_refresh_visitor: bool) -> Result<ResolvedAudio, String> {
        let vd = self.ensure_visitor_data(force_refresh_visitor);
        let mut last_err = String::from("No player clients configured");
        for fp in &self.cfg.player_clients {
            if !fp.enabled {
                continue;
            }
            match self.try_player_client(fp, video_id, &vd) {
                Ok(res) => return Ok(res),
                Err(e) => last_err = format!("{}: {e}", fp.name),
            }
        }
        Err(last_err)
    }

    fn try_player_client(
        &self,
        fp: &ClientFingerprint,
        video_id: &str,
        visitor_data: &str,
    ) -> Result<ResolvedAudio, String> {
        let url = self.endpoint("https://www.youtube.com/youtubei/v1/player");
        let body = json!({
            "context": { "client": fp.context },
            "videoId": video_id,
            "contentCheckOk": true,
            "racyCheckOk": true
        });
        let mut req = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("User-Agent", &fp.user_agent)
            .set("X-YouTube-Client-Name", &fp.client_name_id.to_string())
            .set("X-YouTube-Client-Version", &fp.innertube_client_version);
        if !visitor_data.is_empty() {
            req = req.set("X-Goog-Visitor-Id", visitor_data);
        }
        let resp = req.send_json(body).map_err(|e| e.to_string())?;
        let v: Value = resp.into_json().map_err(|e| e.to_string())?;

        let status = v
            .pointer("/playabilityStatus/status")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if status != "OK" {
            let reason = v
                .pointer("/playabilityStatus/reason")
                .and_then(|s| s.as_str())
                .unwrap_or(status);
            return Err(format!("Unplayable: {reason}"));
        }

        let title = v
            .pointer("/videoDetails/title")
            .and_then(|s| s.as_str())
            .unwrap_or("Unknown Track")
            .to_string();
        let artist = v
            .pointer("/videoDetails/author")
            .and_then(|s| s.as_str())
            .unwrap_or("Unknown Artist")
            .to_string();
        let duration_secs = v
            .pointer("/videoDetails/lengthSeconds")
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let mut candidates: Vec<(u8, u64, String, String, Option<u64>)> = Vec::new();
        for key in ["adaptiveFormats", "formats"] {
            if let Some(arr) = v
                .get("streamingData")
                .and_then(|sd| sd.get(key))
                .and_then(|x| x.as_array())
            {
                for f in arr {
                    let Some(stream_url) = f.get("url").and_then(|u| u.as_str()) else {
                        continue;
                    };
                    let mime = f
                        .get("mimeType")
                        .and_then(|m| m.as_str())
                        .unwrap_or("");
                    let bitrate = f.get("bitrate").and_then(|b| b.as_u64()).unwrap_or(0);
                    let cl = f
                        .get("contentLength")
                        .and_then(|c| c.as_str())
                        .and_then(|s| s.parse::<u64>().ok());

                    // Prefer pure AAC/MP4 audio (itag 140/139) because symphonia decodes it natively in pure Rust.
                    let rank = if mime.starts_with("audio/mp4") && mime.contains("mp4a") {
                        3
                    } else if mime.starts_with("video/mp4") && mime.contains("mp4a") {
                        // Progressive fallback (MP4 container still demuxable for AAC audio track by symphonia)
                        1
                    } else {
                        0
                    };
                    if rank > 0 {
                        candidates.push((rank, bitrate, stream_url.to_string(), mime.to_string(), cl));
                    }
                }
            }
        }

        // Prefer highest rank (audio-only AAC), then ~128kbps-256kbps sweet spot
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        let best = candidates
            .into_iter()
            .next()
            .ok_or_else(|| "No direct AAC/MP4 stream URL returned".to_string())?;

        Ok(ResolvedAudio {
            url: best.2,
            user_agent: fp.user_agent.clone(),
            bitrate: best.1,
            mime_type: best.3,
            title,
            artist,
            duration_secs,
            content_length: best.4,
        })
    }
}

/// Extract a YouTube video_id or playlist_id from pasted URLs or raw IDs.
pub enum PastedLink {
    Video(String),
    Playlist(String),
}

pub fn extract_playlist_id(input: &str) -> Option<String> {
    let s = input.trim();
    if let Some(idx) = s.find("list=") {
        let id = s[idx + 5..].split('&').next().unwrap_or("").trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    if !s.contains('/') && (s.starts_with("PL") || s.starts_with("OLAK") || s.starts_with("RDCLAK")) {
        return Some(s.to_string());
    }
    None
}

fn find_playlist_header_title(v: &Value) -> Option<String> {
    for ptr in [
        "/header/musicResponsiveHeaderRenderer/title",
        "/header/musicDetailHeaderRenderer/title",
        "/header/playlistHeaderRenderer/title",
        "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer/title",
    ] {
        if let Some(t) = runs_text(v.pointer(ptr)) {
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

pub fn parse_youtube_input(input: &str) -> Option<PastedLink> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if !s.contains('/') && !s.contains('?') {
        if s.len() == 11 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(PastedLink::Video(s.to_string()));
        }
        return None;
    }
    if let Some(idx) = s.find("list=") {
        let rest = &s[idx + 5..];
        let id = rest.split('&').next().unwrap_or("").trim();
        if !id.is_empty() && !s.contains("v=") {
            return Some(PastedLink::Playlist(id.to_string()));
        }
    }
    if let Some(idx) = s.find("v=") {
        let rest = &s[idx + 2..];
        let id = rest.split('&').next().unwrap_or("").trim();
        if id.len() == 11 {
            return Some(PastedLink::Video(id.to_string()));
        }
    }
    if let Some(idx) = s.find("youtu.be/") {
        let rest = &s[idx + 9..];
        let id = rest.split(['?', '&', '/']).next().unwrap_or("").trim();
        if id.len() == 11 {
            return Some(PastedLink::Video(id.to_string()));
        }
    }
    if let Some(idx) = s.find("list=") {
        let rest = &s[idx + 5..];
        let id = rest.split('&').next().unwrap_or("").trim();
        if !id.is_empty() {
            return Some(PastedLink::Playlist(id.to_string()));
        }
    }
    None
}

fn collect_music_renderers(v: &Value, out: &mut Vec<TrackMeta>, seen: &mut HashSet<String>) {
    match v {
        Value::Object(map) => {
            for key in ["musicResponsiveListItemRenderer", "musicTwoRowItemRenderer"] {
                if let Some(item) = map.get(key) {
                    if let Some(t) = parse_music_item(item) {
                        if seen.insert(t.video_id.clone()) {
                            out.push(t);
                        }
                    }
                }
            }
            for val in map.values() {
                collect_music_renderers(val, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_music_renderers(item, out, seen);
            }
        }
        _ => {}
    }
}

fn collect_playlist_panel(v: &Value, out: &mut Vec<TrackMeta>, seen: &mut HashSet<String>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("playlistPanelVideoRenderer") {
                if let Some(vid) = item.get("videoId").and_then(|x| x.as_str()) {
                    if !vid.is_empty() && seen.insert(vid.to_string()) {
                        let title = runs_text(item.get("title")).unwrap_or_else(|| "Unknown".into());
                        let artist = runs_text(item.get("longBylineText"))
                            .or_else(|| runs_text(item.get("shortBylineText")))
                            .map(|s| clean_byline(&s))
                            .unwrap_or_else(|| "Unknown Artist".into());
                        let dur = runs_text(item.get("lengthText"))
                            .map(|d| parse_duration_str(&d))
                            .unwrap_or(0);
                        out.push(TrackMeta {
                            video_id: vid.to_string(),
                            title,
                            artist,
                            album: String::new(),
                            duration_secs: dur,
                            thumb_url: format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"),
                            local_path: None,
                            file_bytes: 0,
                            is_favorite: false,
                        });
                    }
                }
            }
            for val in map.values() {
                collect_playlist_panel(val, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_playlist_panel(item, out, seen);
            }
        }
        _ => {}
    }
}

fn parse_music_item(item: &Value) -> Option<TrackMeta> {
    let vid = find_video_id(item)?;
    let mut title = String::new();
    let mut artist = String::new();
    let mut album = String::new();
    let mut dur = 0u64;

    if let Some(cols) = item.get("flexColumns").and_then(|x| x.as_array()) {
        if let Some(c0) = cols.first() {
            title = runs_text(c0.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
                .unwrap_or_default();
        }
        if let Some(c1) = cols.get(1) {
            if let Some(runs) = c1
                .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")
                .and_then(|r| r.as_array())
            {
                let parts: Vec<&str> = runs
                    .iter()
                    .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != "•" && *s != "·" && *s != "Song" && *s != "Video")
                    .collect();
                if let Some(a) = parts.first() {
                    artist = (*a).to_string();
                }
                for p in parts.iter().skip(1) {
                    if p.contains(':') && p.chars().all(|c| c.is_ascii_digit() || c == ':') {
                        dur = parse_duration_str(p);
                    } else if album.is_empty() && !p.contains("views") && !p.contains("plays") {
                        album = (*p).to_string();
                    }
                }
            }
        }
        // Check extra flexColumns (e.g. album column in playlists/charts)
        if album.is_empty() {
            if let Some(c2) = cols.get(2) {
                if let Some(txt) = runs_text(c2.pointer("/musicResponsiveListItemFlexColumnRenderer/text")) {
                    let t = txt.trim();
                    if !t.is_empty() && !t.contains("views") && !t.contains("plays") {
                        album = t.to_string();
                    }
                }
            }
        }
    }

    if dur == 0 {
        if let Some(fcols) = item.get("fixedColumns").and_then(|x| x.as_array()) {
            for fc in fcols {
                if let Some(txt) = runs_text(fc.pointer("/musicResponsiveListItemFixedColumnRenderer/text")) {
                    let t = txt.trim();
                    if t.contains(':') && t.chars().all(|c| c.is_ascii_digit() || c == ':') {
                        dur = parse_duration_str(t);
                        if dur > 0 {
                            break;
                        }
                    }
                }
            }
        }
    }

    if title.is_empty() {
        title = runs_text(item.get("title"))?;
    }
    if artist.is_empty() {
        artist = runs_text(item.get("subtitle"))
            .map(|s| clean_byline(&s))
            .unwrap_or_else(|| "YouTube Music".into());
    }

    Some(TrackMeta {
        thumb_url: format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"),
        video_id: vid,
        title,
        artist,
        album,
        duration_secs: dur,
        local_path: None,
        file_bytes: 0,
        is_favorite: false,
    })
}

fn find_video_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(vid) = map.get("videoId").and_then(|x| x.as_str()) {
                if !vid.is_empty() {
                    return Some(vid.to_string());
                }
            }
            for val in map.values() {
                if let Some(found) = find_video_id(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_video_id),
        _ => None,
    }
}

fn runs_text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.get("simpleText").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    let runs = v.get("runs")?.as_array()?;
    let mut out = String::new();
    for r in runs {
        if let Some(t) = r.get("text").and_then(|x| x.as_str()) {
            out.push_str(t);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn clean_byline(s: &str) -> String {
    s.split(['•', '·'])
        .map(str::trim)
        .find(|p| !p.is_empty() && *p != "Song" && *p != "Video")
        .unwrap_or(s.trim())
        .to_string()
}

fn color_for_name(name: &str) -> (u8, u8, u8) {
    let palette = [
        (244, 63, 94),
        (20, 184, 166),
        (139, 92, 246),
        (245, 158, 11),
        (236, 72, 153),
        (59, 130, 246),
    ];
    let h = name.bytes().fold(0usize, |acc, b| acc.wrapping_add(b as usize));
    palette[h % palette.len()]
}

fn collect_vibe_buttons(v: &Value, out: &mut Vec<YtmVibe>, seen: &mut HashSet<String>) {
    match v {
        Value::Object(map) => {
            if let Some(btn) = map.get("musicNavigationButtonRenderer") {
                let title = runs_text(btn.get("buttonText")).unwrap_or_default();
                let ep = btn.pointer("/clickCommand/browseEndpoint");
                let browse_id = ep.and_then(|e| e.get("browseId")).and_then(|x| x.as_str()).unwrap_or("");
                let params = ep.and_then(|e| e.get("params")).and_then(|x| x.as_str()).unwrap_or("");
                if !title.is_empty() && !browse_id.is_empty() {
                    let id = format!("{browse_id}:{params}");
                    if seen.insert(id.clone()) {
                        let stripe = btn.pointer("/solid/leftStripeColor").and_then(|x| x.as_u64()).unwrap_or(0);
                        let rgb = if stripe != 0 {
                            let r = ((stripe >> 16) & 0xFF) as u8;
                            let g = ((stripe >> 8) & 0xFF) as u8;
                            let b = (stripe & 0xFF) as u8;
                            // Ensure stripe color is vibrant enough if grayscale
                            if r == g && g == b { color_for_name(&title) } else { (r, g, b) }
                        } else {
                            color_for_name(&title)
                        };
                        out.push(YtmVibe {
                            id,
                            title,
                            browse_id: browse_id.to_string(),
                            params: params.to_string(),
                            rgb,
                        });
                    }
                }
            }
            for val in map.values() {
                collect_vibe_buttons(val, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_vibe_buttons(item, out, seen);
            }
        }
        _ => {}
    }
}

fn find_first_playlist_browse_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(bid) = map.get("browseId").and_then(|x| x.as_str()) {
                if bid.starts_with("VL") {
                    return Some(bid.to_string());
                }
            }
            for val in map.values() {
                if let Some(found) = find_first_playlist_browse_id(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_first_playlist_browse_id),
        _ => None,
    }
}

fn extract_thumb_url(item: &Value) -> String {
    let raw = item
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(|x| x.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .unwrap_or("");
    raw.replace("w120-h120", "w300-h300")
}

fn collect_artist_items(v: &Value, out: &mut Vec<YtmArtist>, seen: &mut HashSet<String>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer") {
                if let Some(bid) = item
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(|x| x.as_str())
                {
                    if bid.starts_with("UC") {
                        let cols = item.get("flexColumns").and_then(|x| x.as_array());
                        let name = cols
                            .and_then(|c| c.first())
                            .and_then(|c0| runs_text(c0.pointer("/musicResponsiveListItemFlexColumnRenderer/text")))
                            .unwrap_or_default();
                        let sub = cols
                            .and_then(|c| c.get(1))
                            .and_then(|c1| runs_text(c1.pointer("/musicResponsiveListItemFlexColumnRenderer/text")))
                            .unwrap_or_else(|| "Artist".into());
                        if !name.is_empty() && seen.insert(name.to_lowercase()) {
                            let rgb = color_for_name(&name);
                            let thumb_url = extract_thumb_url(item);
                            out.push(YtmArtist {
                                name,
                                subtitle: sub,
                                browse_id: bid.to_string(),
                                thumb_url,
                                rgb,
                            });
                        }
                    }
                }
            }
            for val in map.values() {
                collect_artist_items(val, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_artist_items(item, out, seen);
            }
        }
        _ => {}
    }
}

fn collect_album_items(v: &Value, out: &mut Vec<YtmAlbum>, seen: &mut HashSet<String>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer") {
                if let Some(bid) = item
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(|x| x.as_str())
                {
                    if bid.starts_with("MPREb_") || bid.starts_with("OLAK") {
                        let cols = item.get("flexColumns").and_then(|x| x.as_array());
                        let title = cols
                            .and_then(|c| c.first())
                            .and_then(|c0| runs_text(c0.pointer("/musicResponsiveListItemFlexColumnRenderer/text")))
                            .unwrap_or_default();
                        let sub_full = cols
                            .and_then(|c| c.get(1))
                            .and_then(|c1| runs_text(c1.pointer("/musicResponsiveListItemFlexColumnRenderer/text")))
                            .unwrap_or_default();
                        let mut artist = String::new();
                        let mut year = String::new();
                        for part in sub_full.split(['•', '·']).map(str::trim) {
                            if part.is_empty() || part == "Album" || part == "EP" || part == "Single" {
                                continue;
                            }
                            if part.len() == 4 && part.chars().all(|c| c.is_ascii_digit()) {
                                year = part.to_string();
                            } else if artist.is_empty() {
                                artist = part.to_string();
                            }
                        }
                        if !title.is_empty() && seen.insert(bid.to_string()) {
                            let rgb = color_for_name(&title);
                            let thumb_url = extract_thumb_url(item);
                            let subtitle = match (artist.is_empty(), year.is_empty()) {
                                (false, false) => format!("{artist} · {year}"),
                                (false, true) => artist.clone(),
                                (true, false) => format!("Album · {year}"),
                                (true, true) => "Album".into(),
                            };
                            out.push(YtmAlbum {
                                title,
                                artist,
                                subtitle,
                                browse_id: bid.to_string(),
                                thumb_url,
                                rgb,
                            });
                        }
                    }
                }
            }
            for val in map.values() {
                collect_album_items(val, out, seen);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_album_items(item, out, seen);
            }
        }
        _ => {}
    }
}

fn collect_two_row_albums(
    v: &Value,
    artist_name: &str,
    out: &mut Vec<YtmAlbum>,
    seen: &mut HashSet<String>,
) {
    let Some(contents) = v.get("contents").and_then(|x| x.as_array()) else {
        return;
    };
    for entry in contents {
        let Some(item) = entry.get("musicTwoRowItemRenderer") else {
            continue;
        };
        let Some(bid) = item
            .pointer("/navigationEndpoint/browseEndpoint/browseId")
            .and_then(|x| x.as_str())
        else {
            continue;
        };
        if !(bid.starts_with("MPREb_") || bid.starts_with("OLAK")) {
            continue;
        }
        let title = runs_text(item.get("title")).unwrap_or_default();
        let sub = runs_text(item.get("subtitle")).unwrap_or_else(|| "Album".into());
        if title.is_empty() || !seen.insert(bid.to_string()) {
            continue;
        }
        let thumb_url = item
            .pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails")
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.first())
            .and_then(|t| t.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let rgb = color_for_name(&title);
        out.push(YtmAlbum {
            title,
            artist: artist_name.to_string(),
            subtitle: sub,
            browse_id: bid.to_string(),
            thumb_url,
            rgb,
        });
    }
}

fn find_lyrics_browse_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(id) = map.get("browseId").and_then(|x| x.as_str()) {
                if id.starts_with("MPLYt_") {
                    return Some(id.to_string());
                }
            }
            for val in map.values() {
                if let Some(found) = find_lyrics_browse_id(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_lyrics_browse_id),
        _ => None,
    }
}

fn find_description_text(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(shelf) = map.get("musicDescriptionShelfRenderer") {
                if let Some(txt) = runs_text(shelf.get("description")) {
                    return Some(txt);
                }
            }
            for val in map.values() {
                if let Some(found) = find_description_text(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_description_text),
        _ => None,
    }
}

fn collect_suggestions(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if let Some(sr) = map.get("searchSuggestionRenderer") {
                if let Some(txt) = runs_text(sr.get("suggestion")) {
                    if !out.contains(&txt) {
                        out.push(txt);
                    }
                }
            }
            for val in map.values() {
                collect_suggestions(val, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_suggestions(item, out);
            }
        }
        _ => {}
    }
}

pub fn parse_duration_str(s: &str) -> u64 {
    let mut total = 0u64;
    for part in s.trim().split(':') {
        let Ok(n) = part.trim().parse::<u64>() else {
            return 0;
        };
        total = total * 60 + n;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations_and_urls() {
        assert_eq!(parse_duration_str("4:03"), 243);
        assert_eq!(parse_duration_str("1:02:05"), 3725);
        match parse_youtube_input("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=abc") {
            Some(PastedLink::Video(id)) => assert_eq!(id, "dQw4w9WgXcQ"),
            _ => panic!("expected video link"),
        }
        match parse_youtube_input("https://music.youtube.com/playlist?list=PL1234567890") {
            Some(PastedLink::Playlist(id)) => assert_eq!(id, "PL1234567890"),
            _ => panic!("expected playlist link"),
        }
        let sample = json!({
            "playlistItemData": { "videoId": "dQw4w9WgXcQ" },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song" }] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Artist" }] } } }
            ],
            "fixedColumns": [
                { "musicResponsiveListItemFixedColumnRenderer": { "text": { "runs": [{ "text": "3:42" }] } } }
            ]
        });
        let parsed = parse_music_item(&sample).expect("parse fixedColumns track");
        assert_eq!(parsed.duration_secs, 222);
    }
}
