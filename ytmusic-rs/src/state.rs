use crate::audio::{download_to_file, start_local_playback, PlaybackSession};
use crate::db::{load_settings, save_settings, AppSettings, MusicDb, PlaylistSummary, TrackMeta};
use crate::innertube::{parse_youtube_input, Innertube, PastedLink, YtmAlbum, YtmArtist, YtmArtistPage, YtmVibe};
use crate::lyrics::{fetch_lyrics, TrackLyrics};
use crate::media::{
    build_dot_grid_image, extract_cover_accent, fmt_time, read_battery_label, resolve_base_dir,
};
use crate::sdl2_platform::SdlHost;
use crate::web_remote::WebRemote;
use crate::TrackRow;
use slint::{Image, SharedString};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

pub const DOCK_TABS: [&str; 5] = ["now", "vibes", "artists", "lib", "find"];
pub const QWERTY_KEYS: [&str; 30] = [
    "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P",
    "A", "S", "D", "F", "G", "H", "J", "K", "L", "DEL",
    "Z", "X", "C", "V", "B", "N", "M", "SPC", "123", "GO",
];
pub const NUM_KEYS: [&str; 30] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "0",
    "-", "_", "/", ".", ":", "&", "!", "?", "'", "DEL",
    "@", "#", "(", ")", "+", "=", "SPC", "CLR", "ABC", "GO",
];

pub struct VibeDef {
    pub id: &'static str,
    pub title: &'static str,
    pub query: &'static str,
    pub rgb: (u8, u8, u8),
}

pub const VIBES: [VibeDef; 6] = [
    VibeDef { id: "synth", title: "Synthwave", query: "synthwave outrun electronic", rgb: (244, 63, 94) },
    VibeDef { id: "lofi", title: "Lo-Fi Jazz", query: "lofi jazz hip hop chill beats", rgb: (20, 184, 166) },
    VibeDef { id: "city", title: "City Pop", query: "japanese city pop 80s", rgb: (236, 72, 153) },
    VibeDef { id: "dream", title: "Dream Pop", query: "dream pop indie synthpop", rgb: (139, 92, 246) },
    VibeDef { id: "house", title: "French House", query: "french house daft punk electronic", rgb: (245, 158, 11) },
    VibeDef { id: "ambient", title: "Ambient", query: "ambient electronic focus tycho", rgb: (59, 130, 246) },
];

pub const DEFAULT_ARTISTS: [(&str, &str, (u8, u8, u8)); 6] = [
    ("M83", "Dream Pop", (139, 92, 246)),
    ("Nujabes", "Lo-Fi Jazz", (20, 184, 166)),
    ("Kavinsky", "Synthwave", (244, 63, 94)),
    ("Daft Punk", "French House", (245, 158, 11)),
    ("Tatsuro Yamashita", "City Pop", (236, 72, 153)),
    ("Tycho", "Ambient", (59, 130, 246)),
];

#[derive(Clone)]
pub struct ArtistEntry {
    pub name: String,
    pub subtitle: String,
    pub browse_id: String,
    pub thumb_url: String,
    pub rgb: (u8, u8, u8),
}

#[derive(Clone)]
pub enum FindHit {
    Artist(ArtistEntry),
    Album(YtmAlbum),
    Song(TrackMeta),
}

#[derive(Clone)]
pub enum ConfirmAction {
    DownloadBatch { title: String, tracks: Vec<TrackMeta> },
    ClearAllDownloads { count: usize, freed_label: String },
}

pub enum BgMsg {
    PicksLoaded(Vec<TrackMeta>),
    MoodsLoaded(Vec<YtmVibe>),
    ChartArtistsLoaded(Vec<YtmArtist>),
    ArtistsSearchLoaded { query: String, artists: Vec<YtmArtist> },
    VibeLoaded { vibe_id: String, tracks: Vec<TrackMeta> },
    ArtistPageLoaded { artist: String, page: YtmArtistPage },
    AlbumLoaded { browse_id: String, tracks: Vec<TrackMeta> },
    SearchResults { query: String, hits: Vec<FindHit> },
    SuggestionsLoaded { query: String, items: Vec<String> },
    LyricsLoaded { video_id: String, lyrics: Option<TrackLyrics> },
    StreamResolved {
        track: TrackMeta,
        url: String,
        user_agent: String,
        bitrate: u64,
        duration_secs: u64,
        content_length: Option<u64>,
    },
    StreamError(String),
    RadioLoaded { seed_id: String, tracks: Vec<TrackMeta> },
    CoverLoaded {
        video_id: String,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        accent_rgb: Option<(u8, u8, u8)>,
    },
    ThumbLoaded { url: String, width: u32, height: u32, rgba: Vec<u8> },
    OfflineCached { track: TrackMeta, path: String, bytes: u64 },
    Toast(String),
}

pub struct AppState {
    pub db_path: PathBuf,
    pub cache_dir: PathBuf,
    pub settings_path: PathBuf,
    pub cfg: AppSettings,
    pub db: MusicDb,
    pub innertube: Arc<Innertube>,
    pub bg_tx: mpsc::Sender<BgMsg>,
    pub bg_rx: mpsc::Receiver<BgMsg>,

    pub active_tab: String,
    pub vibes_list: Vec<YtmVibe>,
    pub vibe_index: usize,
    pub vibe_row_scroll: usize,
    pub vibe_fav_only: bool,
    pub vibe_filter: String,
    pub fav_vibes: HashSet<String>,

    pub chart_artists: Vec<YtmArtist>,
    pub searched_artists: Vec<YtmArtist>,
    pub artist_index: usize,
    pub artist_row_scroll: usize,
    pub artist_fav_only: bool,
    pub artist_filter: String,
    pub fav_artists: HashSet<String>,
    pub artists_list: Vec<ArtistEntry>,
    pub grid_kbd_open: bool,

    pub artist_section: usize,
    pub artist_title: String,
    pub artist_thumb_url: String,
    pub artist_kicker: String,
    pub artist_stats: String,
    pub artist_origin: String,
    pub artist_albums: Vec<YtmAlbum>,
    pub artist_singles: Vec<YtmAlbum>,
    pub artist_songs: Vec<TrackMeta>,

    pub detail_title: String,
    pub detail_subtitle: String,
    pub detail_browse_id: String,
    pub detail_thumb_url: String,
    pub detail_origin: String,
    pub detail_accent: (u8, u8, u8),
    pub detail_tracks: Vec<TrackMeta>,
    pub detail_top_focused: bool,
    pub list_index: usize,
    pub list_scroll: usize,

    pub lib_mode: String,
    pub lib_filter: String,
    pub playlists: Vec<PlaylistSummary>,
    pub playlist_index: usize,
    pub lib_focus_playlists: bool,
    pub lib_tracks: Vec<TrackMeta>,
    pub batch_cancel: Option<Arc<AtomicBool>>,
    pub confirm_action: Option<ConfirmAction>,
    pub kbd_num_mode: bool,

    pub picks: Vec<TrackMeta>,
    pub queue: Vec<TrackMeta>,
    pub queue_pos: usize,
    pub queue_label: String,

    pub now_track: Option<TrackMeta>,
    pub now_genre: String,
    pub session: Option<PlaybackSession>,
    pub is_playing: bool,
    pub loop_track: bool,
    pub elapsed_ms: u64,
    pub last_pcm_pos_ms: u64,
    pub stream_ended: bool,
    pub duration_secs: u64,
    pub now_format: String,

    pub cover_cache: HashMap<String, Image>,
    pub cover_order: VecDeque<String>,
    pub cover_colors: HashMap<String, (u8, u8, u8)>,
    pub thumb_cache: HashMap<String, Image>,
    pub circle_thumb_cache: HashMap<String, Image>,
    pub thumb_order: VecDeque<String>,
    pub thumb_fetching: HashSet<String>,
    pub active_cover_id: Option<String>,
    pub lyrics_cache: HashMap<String, Option<TrackLyrics>>,

    pub search_query: String,
    pub find_zone: usize,
    pub chip_index: usize,
    pub suggest_chips: Vec<String>,
    pub find_hits: Vec<FindHit>,
    pub find_hit_index: usize,
    pub find_hit_scroll: usize,
    pub key_row: usize,
    pub key_col: usize,

    pub pl_modal_open: bool,
    pub pl_modal_index: usize,
    pub pl_target_track: Option<TrackMeta>,

    pub settings_open: bool,
    pub settings_index: usize,
    pub pocket_lock: bool,
    pub toast: Option<(String, Instant)>,

    pub battery_label: String,
    pub last_battery_check: Instant,
    pub dots_dark: Image,
    pub dots_light: Image,
    pub remote: Option<WebRemote>,
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let base_dir = resolve_base_dir();
        let data_dir = base_dir.join("data");
        let cache_dir = base_dir.join("cache");
        let db_path = data_dir.join("music.db");
        let settings_path = data_dir.join("settings.json");
        let cfg = load_settings(&settings_path);
        let db = MusicDb::open(&db_path, &cache_dir)?;

        let clients_path = base_dir.join("config").join("clients.json");
        let innertube = Arc::new(Innertube::from_file(&clients_path)?);
        let (bg_tx, bg_rx) = mpsc::channel();

        let fav_vibes: HashSet<String> = db.list_fav_vibes().into_iter().collect();
        let fav_artists: HashSet<String> = db.list_fav_artists().into_iter().collect();
        let playlists = db.list_playlists();

        let mut picks = db.list_favorites();
        for r in db.list_recent(20) {
            if !picks.iter().any(|t| t.video_id == r.video_id) {
                picks.push(r);
            }
        }
        if picks.is_empty() {
            picks = db.list_offline();
        }

        let remote = if cfg.web_server {
            WebRemote::start(
                8080,
                db_path.clone(),
                cache_dir.clone(),
                innertube.clone(),
                cfg.web_pin_enabled,
            )
        } else {
            None
        };

        let vibes_list: Vec<YtmVibe> = VIBES
            .iter()
            .map(|v| YtmVibe {
                id: v.id.to_string(),
                title: v.title.to_string(),
                browse_id: String::new(),
                params: String::new(),
                rgb: v.rgb,
            })
            .collect();

        let mut state = Self {
            db_path,
            cache_dir,
            settings_path,
            cfg,
            db,
            innertube,
            bg_tx,
            bg_rx,
            active_tab: if picks.is_empty() { "vibes".into() } else { "now".into() },
            vibes_list,
            vibe_index: 0,
            vibe_row_scroll: 0,
            vibe_fav_only: false,
            vibe_filter: String::new(),
            fav_vibes,
            chart_artists: Vec::new(),
            searched_artists: Vec::new(),
            artist_index: 0,
            artist_row_scroll: 0,
            artist_fav_only: false,
            artist_filter: String::new(),
            fav_artists,
            artists_list: Vec::new(),
            grid_kbd_open: false,
            artist_section: 0,
            artist_title: String::new(),
            artist_thumb_url: String::new(),
            artist_kicker: "ARTIST".into(),
            artist_stats: String::new(),
            artist_origin: "artists".into(),
            artist_albums: Vec::new(),
            artist_singles: Vec::new(),
            artist_songs: Vec::new(),
            detail_title: String::new(),
            detail_subtitle: String::new(),
            detail_browse_id: String::new(),
            detail_thumb_url: String::new(),
            detail_origin: "vibes".into(),
            detail_accent: (190, 242, 100),
            detail_tracks: Vec::new(),
            detail_top_focused: false,
            list_index: 0,
            list_scroll: 0,
            lib_mode: "favs".into(),
            lib_filter: String::new(),
            playlists,
            playlist_index: 0,
            lib_focus_playlists: true,
            lib_tracks: Vec::new(),
            batch_cancel: None,
            confirm_action: None,
            kbd_num_mode: false,
            picks: picks.clone(),
            queue: picks,
            queue_pos: 0,
            queue_label: "QUEUE · UPCOMING".into(),
            now_track: None,
            now_genre: "YT MUSIC".into(),
            session: None,
            is_playing: false,
            loop_track: false,
            elapsed_ms: 0,
            last_pcm_pos_ms: 0,
            stream_ended: false,
            duration_secs: 0,
            now_format: "AAC 128k".into(),
            cover_cache: HashMap::new(),
            cover_order: VecDeque::new(),
            cover_colors: HashMap::new(),
            thumb_cache: HashMap::new(),
            circle_thumb_cache: HashMap::new(),
            thumb_order: VecDeque::new(),
            thumb_fetching: HashSet::new(),
            active_cover_id: None,
            lyrics_cache: HashMap::new(),
            search_query: String::new(),
            find_zone: 2,
            chip_index: 0,
            suggest_chips: vec![
                "nujabes".into(),
                "midnight city".into(),
                "synthwave".into(),
                "city pop".into(),
            ],
            find_hits: Vec::new(),
            find_hit_index: 0,
            find_hit_scroll: 0,
            key_row: 0,
            key_col: 0,
            pl_modal_open: false,
            pl_modal_index: 0,
            pl_target_track: None,
            settings_open: false,
            settings_index: 0,
            pocket_lock: false,
            toast: None,
            battery_label: read_battery_label(),
            last_battery_check: Instant::now(),
            dots_dark: build_dot_grid_image(true),
            dots_light: build_dot_grid_image(false),
            remote,
        };

        state.rebuild_artists_list();
        state.refresh_lib_tracks();

        let it = state.innertube.clone();
        let tx = state.bg_tx.clone();
        thread::spawn(move || {
            let tracks = it.quick_picks();
            if !tracks.is_empty() {
                let _ = tx.send(BgMsg::PicksLoaded(tracks));
            }
        });

        let it_moods = state.innertube.clone();
        let tx_moods = state.bg_tx.clone();
        thread::spawn(move || {
            let moods = it_moods.fetch_moods_and_genres();
            if !moods.is_empty() {
                let _ = tx_moods.send(BgMsg::MoodsLoaded(moods));
            }
        });

        let it_charts = state.innertube.clone();
        let tx_charts = state.bg_tx.clone();
        thread::spawn(move || {
            let artists = it_charts.fetch_chart_artists();
            if !artists.is_empty() {
                let _ = tx_charts.send(BgMsg::ChartArtistsLoaded(artists));
            }
        });

        Ok(state)
    }

    pub fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now() + Duration::from_secs(3)));
    }

    pub fn toggle_web_server(&mut self) {
        if let Some(srv) = self.remote.take() {
            srv.shutdown();
            self.cfg.web_server = false;
            save_settings(&self.settings_path, &self.cfg);
            self.set_toast("Local Web Server: OFF");
        } else {
            self.cfg.web_server = true;
            save_settings(&self.settings_path, &self.cfg);
            self.remote = WebRemote::start(
                8080,
                self.db_path.clone(),
                self.cache_dir.clone(),
                self.innertube.clone(),
                self.cfg.web_pin_enabled,
            );
            if let Some(ref srv) = self.remote {
                if srv.pin_enabled() {
                    self.set_toast(format!("Web ON · PIN {}", srv.pin()));
                } else {
                    self.set_toast("Local Web Server: ON");
                }
            } else {
                self.set_toast("Web Server failed to bind :8080");
            }
        }
    }

    pub fn toggle_web_pin(&mut self) {
        self.cfg.web_pin_enabled = !self.cfg.web_pin_enabled;
        save_settings(&self.settings_path, &self.cfg);
        if let Some(ref srv) = self.remote {
            srv.set_pin_enabled(self.cfg.web_pin_enabled);
            if self.cfg.web_pin_enabled {
                self.set_toast(format!("Web PIN ON: {}", srv.pin()));
            } else {
                self.set_toast("Web Security PIN: OFF");
            }
        } else {
            self.set_toast(if self.cfg.web_pin_enabled {
                "Web Security PIN: ON"
            } else {
                "Web Security PIN: OFF"
            });
        }
    }

    pub fn rotate_web_pin(&mut self) {
        if let Some(ref srv) = self.remote {
            if !self.cfg.web_pin_enabled {
                self.cfg.web_pin_enabled = true;
                save_settings(&self.settings_path, &self.cfg);
                srv.set_pin_enabled(true);
            }
            let pin = srv.rotate_pin();
            self.set_toast(format!("New Web PIN: {pin}"));
        } else {
            self.set_toast("Turn ON Web Server first");
        }
    }

    pub fn queue_thumb_urls(&mut self, urls: Vec<String>) {
        let mut todo = Vec::new();
        for url in urls {
            if !url.is_empty()
                && !self.thumb_cache.contains_key(&url)
                && self.thumb_fetching.insert(url.clone())
            {
                todo.push(url);
            }
        }
        if todo.is_empty() {
            return;
        }
        let tx = self.bg_tx.clone();
        thread::spawn(move || {
            use std::io::Read;
            for url in todo {
                if let Ok(resp) = ureq::get(&url).timeout(Duration::from_secs(5)).call() {
                    let mut bytes = Vec::new();
                    if resp.into_reader().take(512 * 1024).read_to_end(&mut bytes).is_ok() {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let rgba = img.to_rgba8();
                            let _ = tx.send(BgMsg::ThumbLoaded {
                                url,
                                width: rgba.width(),
                                height: rgba.height(),
                                rgba: rgba.into_raw(),
                            });
                        }
                    }
                }
            }
        });
    }

    pub fn rebuild_artists_list(&mut self) {
        let mut entries: Vec<ArtistEntry> = Vec::new();
        let mut seen = HashSet::new();
        let palette = [
            (139, 92, 246),
            (20, 184, 166),
            (244, 63, 94),
            (245, 158, 11),
            (236, 72, 153),
            (59, 130, 246),
        ];

        for a in &self.searched_artists {
            if seen.insert(a.name.to_lowercase()) {
                entries.push(ArtistEntry {
                    name: a.name.clone(),
                    subtitle: a.subtitle.clone(),
                    browse_id: a.browse_id.clone(),
                    thumb_url: a.thumb_url.clone(),
                    rgb: a.rgb,
                });
            }
        }
        for fav in &self.fav_artists {
            if seen.insert(fav.to_lowercase()) {
                let idx = entries.len() % palette.len();
                let found = self
                    .chart_artists
                    .iter()
                    .chain(self.searched_artists.iter())
                    .find(|a| a.name.eq_ignore_ascii_case(fav));
                entries.push(ArtistEntry {
                    name: fav.clone(),
                    subtitle: "Favorited Artist".into(),
                    browse_id: found.map(|a| a.browse_id.clone()).unwrap_or_default(),
                    thumb_url: found.map(|a| a.thumb_url.clone()).unwrap_or_default(),
                    rgb: palette[idx],
                });
            }
        }
        for a in &self.chart_artists {
            if seen.insert(a.name.to_lowercase()) {
                entries.push(ArtistEntry {
                    name: a.name.clone(),
                    subtitle: a.subtitle.clone(),
                    browse_id: a.browse_id.clone(),
                    thumb_url: a.thumb_url.clone(),
                    rgb: a.rgb,
                });
            }
        }
        for t in &self.picks {
            let name = t.artist.trim();
            if name.is_empty() || name == "YouTube Music" {
                continue;
            }
            if seen.insert(name.to_lowercase()) {
                let idx = entries.len() % palette.len();
                entries.push(ArtistEntry {
                    name: name.to_string(),
                    subtitle: if t.album.is_empty() { "Quick Pick".into() } else { t.album.clone() },
                    browse_id: String::new(),
                    thumb_url: t.thumb_url.clone(),
                    rgb: palette[idx],
                });
            }
        }
        for (name, genre, rgb) in DEFAULT_ARTISTS {
            if seen.insert(name.to_lowercase()) {
                entries.push(ArtistEntry {
                    name: name.into(),
                    subtitle: genre.into(),
                    browse_id: String::new(),
                    thumb_url: String::new(),
                    rgb,
                });
            }
        }
        self.artists_list = entries;
        let len = self.filtered_artists().len();
        Self::clamp_grid(&mut self.artist_index, &mut self.artist_row_scroll, len);
        self.ensure_visible_artist_thumbs();
    }

    pub fn ensure_visible_artist_thumbs(&mut self) {
        let start = self.artist_row_scroll * 3;
        let urls: Vec<String> = self
            .filtered_artists()
            .into_iter()
            .skip(start)
            .take(18)
            .filter(|a| !a.thumb_url.is_empty())
            .map(|a| a.thumb_url)
            .collect();
        self.queue_thumb_urls(urls);
    }

    pub fn clamp_grid(index: &mut usize, scroll: &mut usize, len: usize) {
        if len == 0 {
            *index = 0;
            *scroll = 0;
            return;
        }
        if *index >= len {
            *index = len - 1;
        }
        let row = *index / 3;
        if row < *scroll {
            *scroll = row;
        } else if row >= *scroll + 2 {
            *scroll = row - 1;
        }
    }

    pub fn filtered_vibes(&self) -> Vec<YtmVibe> {
        let q = self.vibe_filter.trim().to_lowercase();
        self.vibes_list
            .iter()
            .filter(|v| {
                (!self.vibe_fav_only || self.fav_vibes.contains(&v.id))
                    && (q.is_empty() || v.title.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    pub fn filtered_artists(&self) -> Vec<ArtistEntry> {
        let q = self.artist_filter.trim().to_lowercase();
        self.artists_list
            .iter()
            .filter(|a| {
                (!self.artist_fav_only || self.fav_artists.contains(&a.name))
                    && (q.is_empty() || a.name.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    pub fn refresh_lib_tracks(&mut self) {
        self.playlists = self.db.list_playlists();
        if !self.playlists.is_empty() && self.playlist_index >= self.playlists.len() {
            self.playlist_index = self.playlists.len() - 1;
        }
        let mut raw = match self.lib_mode.as_str() {
            "favs" => self.db.list_favorites(),
            "downloads" => self.db.list_offline(),
            "history" => self.db.list_recent(100),
            "playlists" => {
                if let Some(pl) = self.playlists.get(self.playlist_index) {
                    self.db.playlist_tracks(pl.id)
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        };
        let q = self.lib_filter.trim().to_lowercase();
        if !q.is_empty() {
            raw.retain(|t| {
                t.title.to_lowercase().contains(&q)
                    || t.artist.to_lowercase().contains(&q)
                    || t.album.to_lowercase().contains(&q)
            });
        }
        self.lib_tracks = raw;
        if self.lib_tracks.is_empty() {
            self.detail_top_focused = false;
        }
        self.clamp_list(self.lib_tracks.len());
    }

    pub fn active_list_tracks(&self) -> &[TrackMeta] {
        match self.active_tab.as_str() {
            "vibe_detail" | "album_detail" => &self.detail_tracks,
            "artist_detail" if self.artist_section == 0 => &self.artist_songs,
            "lib" => &self.lib_tracks,
            _ => &[],
        }
    }

    pub fn clamp_window(index: &mut usize, scroll: &mut usize, len: usize, visible: usize) {
        if len == 0 {
            *index = 0;
            *scroll = 0;
            return;
        }
        if *index >= len {
            *index = len - 1;
        }
        if *index < *scroll {
            *scroll = *index;
        } else if *index + 1 >= *scroll + visible && *index + 1 < len {
            *scroll = (*index + 2).saturating_sub(visible);
        } else if *index >= *scroll + visible {
            *scroll = (*index + 1).saturating_sub(visible);
        }
        let max_scroll = len.saturating_sub(visible);
        if *scroll > max_scroll {
            *scroll = max_scroll;
        }
    }

    pub fn clamp_list(&mut self, len: usize) {
        let vis = if self.active_tab == "artist_detail" { 5 } else { 6 };
        Self::clamp_window(&mut self.list_index, &mut self.list_scroll, len, vis);
    }

    pub fn artist_active_len(&self) -> usize {
        match self.artist_section {
            1 => self.artist_albums.len(),
            2 => self.artist_singles.len(),
            _ => self.artist_songs.len(),
        }
    }

    pub fn open_vibe_detail(&mut self, vibe: YtmVibe) {
        self.detail_title = vibe.title.clone();
        self.detail_subtitle = if vibe.browse_id.is_empty() {
            "Mix".into()
        } else {
            "YT Music Mood & Genre".into()
        };
        self.detail_browse_id = vibe.browse_id.clone();
        self.detail_thumb_url.clear();
        self.detail_origin = "vibes".into();
        self.detail_accent = vibe.rgb;
        self.detail_tracks.clear();
        self.detail_top_focused = false;
        self.list_index = 0;
        self.list_scroll = 0;
        self.active_tab = "vibe_detail".into();
        self.now_genre = vibe.title.to_uppercase();

        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        let vid = vibe.id;
        let browse_id = vibe.browse_id;
        let params = vibe.params;
        let title = vibe.title;
        thread::spawn(move || {
            let mut tracks = if !browse_id.is_empty() {
                it.fetch_vibe_tracks(&browse_id, &params)
            } else {
                Vec::new()
            };
            if tracks.is_empty() {
                let fallback_q = VIBES
                    .iter()
                    .find(|v| v.id == vid || v.title.eq_ignore_ascii_case(&title))
                    .map(|v| v.query)
                    .unwrap_or(&title);
                tracks = it.search_songs(fallback_q).unwrap_or_default();
            }
            let _ = tx.send(BgMsg::VibeLoaded { vibe_id: vid, tracks });
        });
    }

    pub fn open_artist_detail(&mut self, artist: ArtistEntry, origin: &str) {
        self.artist_title = artist.name.clone();
        self.artist_thumb_url = artist.thumb_url.clone();
        self.artist_kicker = if artist.subtitle.is_empty() {
            "ARTIST".into()
        } else {
            format!("ARTIST · {}", artist.subtitle).to_uppercase()
        };
        self.artist_stats = "Loading Top Songs & Releases...".into();
        self.artist_origin = origin.into();
        self.artist_section = 0;
        self.artist_songs.clear();
        self.artist_albums.clear();
        self.artist_singles.clear();

        self.detail_title = artist.name.clone();
        self.detail_subtitle = artist.subtitle;
        self.detail_browse_id = artist.browse_id.clone();
        self.detail_thumb_url = artist.thumb_url.clone();
        self.detail_origin = origin.into();
        self.detail_accent = artist.rgb;
        self.detail_tracks.clear();
        self.detail_top_focused = false;
        self.list_index = 0;
        self.list_scroll = 0;
        self.active_tab = "artist_detail".into();

        if !self.artist_thumb_url.is_empty() {
            let u = self.artist_thumb_url.clone();
            self.queue_thumb_urls(vec![u]);
        }

        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        let name = artist.name;
        let browse_id = artist.browse_id;
        thread::spawn(move || {
            let page = it.fetch_artist_page(&browse_id, &name);
            let _ = tx.send(BgMsg::ArtistPageLoaded { artist: name, page });
        });
    }

    pub fn open_album_detail(&mut self, album: YtmAlbum, origin: &str) {
        self.detail_title = album.title.clone();
        self.detail_subtitle = album.subtitle.clone();
        self.detail_browse_id = album.browse_id.clone();
        self.detail_thumb_url = album.thumb_url.clone();
        self.detail_origin = origin.into();
        self.detail_accent = album.rgb;
        self.detail_tracks.clear();
        self.detail_top_focused = false;
        self.list_index = 0;
        self.list_scroll = 0;
        self.active_tab = "album_detail".into();

        if !self.detail_thumb_url.is_empty() {
            let u = self.detail_thumb_url.clone();
            self.queue_thumb_urls(vec![u]);
        }

        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        let bid = album.browse_id;
        let title = album.title;
        let artist = album.artist;
        thread::spawn(move || {
            let tracks = it.fetch_album_tracks(&bid, &title, &artist);
            let _ = tx.send(BgMsg::AlbumLoaded {
                browse_id: bid,
                tracks,
            });
        });
    }

    pub fn start_playing_track(&mut self, mut track: TrackMeta, sdl: &SdlHost, fetch_radio: bool) {
        if let Some(s) = self.session.take() {
            s.stop();
        }
        sdl.clear_audio_queue();
        sdl.set_audio_paused(false);

        track.is_favorite = self.db.is_favorite(&track.video_id);
        let local_path = self.db.get_local_path(&track.video_id);
        track.local_path = local_path.as_ref().map(|p| p.to_string_lossy().into_owned());
        self.db.upsert_track(&track, true);

        self.elapsed_ms = 0;
        self.last_pcm_pos_ms = 0;
        self.stream_ended = false;
        self.duration_secs = track.duration_secs;
        self.now_track = Some(track.clone());
        self.is_playing = true;

        if fetch_radio {
            self.queue = vec![track.clone()];
            self.queue_pos = 0;
            self.queue_label = "RADIO · UP NEXT".into();
        } else if !self.queue.iter().any(|t| t.video_id == track.video_id) {
            self.queue.insert(0, track.clone());
            self.queue_pos = 0;
        } else if let Some(pos) = self.queue.iter().position(|t| t.video_id == track.video_id) {
            self.queue_pos = pos;
        }

        if !self.cover_cache.contains_key(&track.video_id) && !track.thumb_url.is_empty() {
            let vid = track.video_id.clone();
            let url = track.thumb_url.clone();
            let tx = self.bg_tx.clone();
            thread::spawn(move || {
                if let Ok(resp) = ureq::get(&url).timeout(Duration::from_secs(6)).call() {
                    let mut bytes = Vec::new();
                    use std::io::Read;
                    if resp.into_reader().take(2 * 1024 * 1024).read_to_end(&mut bytes).is_ok() {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let rgba = img.to_rgba8();
                            let w = rgba.width();
                            let h = rgba.height();
                            let raw = rgba.into_raw();
                            let accent_rgb = extract_cover_accent(&raw, w, h);
                            let _ = tx.send(BgMsg::CoverLoaded {
                                video_id: vid,
                                width: w,
                                height: h,
                                rgba: raw,
                                accent_rgb,
                            });
                        }
                    }
                }
            });
        }

        if !self.lyrics_cache.contains_key(&track.video_id) {
            let it = self.innertube.clone();
            let tx = self.bg_tx.clone();
            let vid = track.video_id.clone();
            let title = track.title.clone();
            let artist = track.artist.clone();
            let dur = track.duration_secs;
            thread::spawn(move || {
                let lyrics = fetch_lyrics(&it, &vid, &title, &artist, dur);
                let _ = tx.send(BgMsg::LyricsLoaded { video_id: vid, lyrics });
            });
        }

        if let Some(lp) = local_path {
            self.now_format = "M4A 128k".into();
            eprintln!("[audio] playing local {}", lp.display());
            self.session = Some(start_local_playback(&lp));
            self.set_toast(format!("Playing: {}", track.title));
        } else {
            self.now_format = "AAC ...".into();
            let it = self.innertube.clone();
            let tx = self.bg_tx.clone();
            let t_clone = track.clone();
            thread::spawn(move || match it.resolve_audio_stream(&t_clone.video_id, false) {
                Ok(res) => {
                    let _ = tx.send(BgMsg::StreamResolved {
                        track: t_clone,
                        url: res.url,
                        user_agent: res.user_agent,
                        bitrate: res.bitrate,
                        duration_secs: res.duration_secs,
                        content_length: res.content_length,
                    });
                }
                Err(_) => match it.resolve_audio_stream(&t_clone.video_id, true) {
                    Ok(res) => {
                        let _ = tx.send(BgMsg::StreamResolved {
                            track: t_clone,
                            url: res.url,
                            user_agent: res.user_agent,
                            bitrate: res.bitrate,
                            duration_secs: res.duration_secs,
                            content_length: res.content_length,
                        });
                    }
                    Err(e2) => {
                        eprintln!("[innertube] stream resolve error: {e2}");
                        let _ = tx.send(BgMsg::StreamError(e2));
                    }
                },
            });
        }

        if fetch_radio && self.cfg.radio_autoplay {
            let it = self.innertube.clone();
            let tx = self.bg_tx.clone();
            let vid = track.video_id;
            thread::spawn(move || {
                let radio = it.up_next_radio(&vid);
                if !radio.is_empty() {
                    let _ = tx.send(BgMsg::RadioLoaded { seed_id: vid, tracks: radio });
                }
            });
        }
    }

    pub fn trigger_search(&mut self, query: String) {
        let q = query.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.search_query = q.clone();
        self.active_tab = "find".into();
        self.find_zone = 1;
        self.find_hit_index = 0;
        self.find_hit_scroll = 0;
        self.set_toast(format!("Searching: {q}"));

        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        thread::spawn(move || {
            if let Some(link) = parse_youtube_input(&q) {
                match link {
                    PastedLink::Playlist(pl) => match it.fetch_playlist(&pl) {
                        Ok(tracks) => {
                            let hits = tracks.into_iter().map(FindHit::Song).collect();
                            let _ = tx.send(BgMsg::SearchResults { query: q, hits });
                            return;
                        }
                        Err(e) => {
                            let _ = tx.send(BgMsg::Toast(format!("Playlist err: {e}")));
                        }
                    },
                    PastedLink::Video(vid) => {
                        if let Ok(res) = it.resolve_audio_stream(&vid, false) {
                            let t = TrackMeta {
                                video_id: vid.clone(),
                                title: res.title,
                                artist: res.artist,
                                album: String::new(),
                                duration_secs: res.duration_secs,
                                thumb_url: format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"),
                                local_path: None,
                                file_bytes: 0,
                                is_favorite: false,
                            };
                            let _ = tx.send(BgMsg::SearchResults {
                                query: q,
                                hits: vec![FindHit::Song(t)],
                            });
                            return;
                        }
                    }
                }
            }

            let artists = it.search_artists(&q).unwrap_or_default();
            let albums = it.search_albums(&q).unwrap_or_default();
            let songs_res = it.search_songs(&q);
            let mut hits: Vec<FindHit> = Vec::new();
            for a in artists.into_iter().take(2) {
                hits.push(FindHit::Artist(ArtistEntry {
                    name: a.name,
                    subtitle: a.subtitle,
                    browse_id: a.browse_id,
                    thumb_url: a.thumb_url,
                    rgb: a.rgb,
                }));
            }
            for al in albums.into_iter().take(4) {
                hits.push(FindHit::Album(al));
            }
            match songs_res {
                Ok(tracks) => {
                    hits.extend(tracks.into_iter().take(18).map(FindHit::Song));
                    let _ = tx.send(BgMsg::SearchResults { query: q, hits });
                }
                Err(e) => {
                    if !hits.is_empty() {
                        let _ = tx.send(BgMsg::SearchResults { query: q, hits });
                    } else {
                        let _ = tx.send(BgMsg::Toast(format!("Search failed: {e}")));
                    }
                }
            }
        });
    }

    pub fn request_suggestions(&self) {
        let q = self.search_query.trim().to_string();
        if q.len() < 2 {
            return;
        }
        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        thread::spawn(move || {
            let items = it.search_suggestions(&q);
            if !items.is_empty() {
                let _ = tx.send(BgMsg::SuggestionsLoaded { query: q, items });
            }
        });
    }

    pub fn cache_track_offline(&mut self, track: TrackMeta) {
        if self.db.get_local_path(&track.video_id).is_some() {
            self.set_toast("Already downloaded");
            return;
        }
        let dest = self.db.cache_file_path(&track.video_id);
        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        self.set_toast(format!("Downloading: {}", track.title));
        thread::spawn(move || match it.resolve_audio_stream(&track.video_id, false) {
            Ok(res) => match download_to_file(&res.url, &res.user_agent, res.content_length, &dest) {
                Ok(bytes) => {
                    let _ = tx.send(BgMsg::OfflineCached {
                        track,
                        path: dest.to_string_lossy().into_owned(),
                        bytes,
                    });
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::Toast(format!("Download failed: {e}")));
                }
            },
            Err(e) => {
                let _ = tx.send(BgMsg::Toast(format!("Resolve failed: {e}")));
            }
        });
    }

    pub fn download_batch(&mut self, tracks: Vec<TrackMeta>) {
        let pending: Vec<(TrackMeta, PathBuf)> = tracks
            .into_iter()
            .filter(|t| self.db.get_local_path(&t.video_id).is_none())
            .map(|t| {
                let p = self.db.cache_file_path(&t.video_id);
                (t, p)
            })
            .collect();
        if pending.is_empty() {
            self.set_toast("All tracks already downloaded");
            return;
        }
        if let Some(ref prev_cancel) = self.batch_cancel {
            prev_cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.batch_cancel = Some(cancel.clone());
        let count = pending.len();
        self.set_toast(format!("Downloading {count} tracks..."));
        let it = self.innertube.clone();
        let tx = self.bg_tx.clone();
        thread::spawn(move || {
            let mut done = 0usize;
            for (track, dest) in pending {
                if cancel.load(Ordering::Relaxed) {
                    let _ = tx.send(BgMsg::Toast(format!("Stopped batch ({done}/{count} saved)")));
                    return;
                }
                if let Ok(res) = it.resolve_audio_stream(&track.video_id, false) {
                    if let Ok(bytes) = download_to_file(&res.url, &res.user_agent, res.content_length, &dest) {
                        done += 1;
                        let _ = tx.send(BgMsg::OfflineCached {
                            track,
                            path: dest.to_string_lossy().into_owned(),
                            bytes,
                        });
                    }
                }
            }
            let _ = tx.send(BgMsg::Toast(format!("Finished downloading {done} tracks")));
        });
    }

    pub fn toggle_play_pause(&mut self, sdl: &SdlHost) {
        if self.now_track.is_none() {
            if let Some(first) = self.queue.first().cloned() {
                self.start_playing_track(first, sdl, true);
            }
            return;
        }
        self.is_playing = !self.is_playing;
        sdl.set_audio_paused(!self.is_playing);
    }

    pub fn next_track(&mut self, sdl: &SdlHost) {
        if self.queue.is_empty() {
            return;
        }
        self.queue_pos = (self.queue_pos + 1) % self.queue.len();
        let next = self.queue[self.queue_pos].clone();
        self.start_playing_track(next, sdl, false);
    }

    pub fn prev_track(&mut self, sdl: &SdlHost) {
        if self.queue.is_empty() {
            return;
        }
        self.queue_pos = (self.queue_pos + self.queue.len() - 1) % self.queue.len();
        let prev = self.queue[self.queue_pos].clone();
        self.start_playing_track(prev, sdl, false);
    }

    pub fn open_pl_modal(&mut self, track: TrackMeta) {
        self.playlists = self.db.list_playlists();
        self.pl_target_track = Some(track);
        self.pl_modal_index = 0;
        self.pl_modal_open = true;
    }

    pub fn current_selected_track(&self) -> Option<TrackMeta> {
        match self.active_tab.as_str() {
            "now" => self.now_track.clone(),
            "vibe_detail" | "album_detail" => self.detail_tracks.get(self.list_index).cloned(),
            "artist_detail" if self.artist_section == 0 => {
                self.artist_songs.get(self.list_index).cloned()
            }
            "lib" => self.lib_tracks.get(self.list_index).cloned(),
            "find" => match self.find_hits.get(self.find_hit_index) {
                Some(FindHit::Song(t)) => Some(t.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn to_track_rows(&self, tracks: &[TrackMeta]) -> Vec<TrackRow> {
        let now_id = self.now_track.as_ref().map(|t| t.video_id.as_str());
        tracks
            .iter()
            .map(|t| TrackRow {
                title: SharedString::from(&t.title),
                artist: SharedString::from(&t.artist),
                album: SharedString::from(&t.album),
                duration: SharedString::from(if t.duration_secs > 0 {
                    fmt_time(t.duration_secs)
                } else {
                    String::new()
                }),
                is_offline: t.local_path.is_some() || self.db.get_local_path(&t.video_id).is_some(),
                is_favorite: self.db.is_favorite(&t.video_id),
                is_playing: now_id == Some(t.video_id.as_str()),
                is_artist: false,
                kind: SharedString::new(),
                thumb: Image::default(),
                has_thumb: false,
            })
            .collect()
    }
}
