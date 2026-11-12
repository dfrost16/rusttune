use crate::db::{load_settings, save_settings, AppSettings, MusicDb, PlaylistSummary, TrackMeta};
use crate::innertube::{Innertube, YtmAlbum, YtmArtist, YtmArtistPage, YtmVibe};
use crate::lyrics::TrackLyrics;
use crate::media::{build_dot_grid_image, fmt_time, read_battery_label, resolve_base_dir};
use crate::audio::PlaybackSession;
use crate::web_remote::WebRemote;
use crate::TrackRow;
use slint::{Image, SharedString};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
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
    pub filter_focused: bool,
    pub now_artist_focused: bool,
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

fn default_vibes_list() -> Vec<YtmVibe> {
    VIBES
        .iter()
        .map(|v| YtmVibe {
            id: v.id.to_string(),
            title: v.title.to_string(),
            browse_id: String::new(),
            params: String::new(),
            rgb: v.rgb,
        })
        .collect()
}

impl AppState {
    fn from_parts(
        db_path: PathBuf,
        cache_dir: PathBuf,
        settings_path: PathBuf,
        cfg: AppSettings,
        db: MusicDb,
        innertube: Arc<Innertube>,
        remote: Option<WebRemote>,
        dots_dark: Image,
        dots_light: Image,
    ) -> Self {
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
            vibes_list: default_vibes_list(),
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
            filter_focused: false,
            now_artist_focused: false,
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
            dots_dark,
            dots_light,
            remote,
        };
        state.rebuild_artists_list();
        state.refresh_lib_tracks();
        state
    }

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

        let state = Self::from_parts(
            db_path,
            cache_dir,
            settings_path,
            cfg,
            db,
            innertube,
            remote,
            build_dot_grid_image(true),
            build_dot_grid_image(false),
        );

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

    #[cfg(test)]
    pub fn new_for_test(dir: &std::path::Path) -> Self {
        let data_dir = dir.join("data");
        let cache_dir = dir.join("cache");
        let db_path = data_dir.join("music.db");
        let settings_path = data_dir.join("settings.json");
        let _ = std::fs::create_dir_all(&data_dir);
        let mut cfg = AppSettings::default();
        cfg.web_server = false;
        let db = MusicDb::open(&db_path, &cache_dir).expect("open test db");
        Self::from_parts(
            db_path,
            cache_dir,
            settings_path,
            cfg,
            db,
            Arc::new(Innertube::new_empty_for_test()),
            None,
            Image::default(),
            Image::default(),
        )
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
