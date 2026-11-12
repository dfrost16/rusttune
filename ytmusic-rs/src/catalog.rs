use crate::db::TrackMeta;
use crate::innertube::{YtmAlbum, YtmVibe};
use crate::state::{AppState, ArtistEntry, BgMsg, DEFAULT_ARTISTS, VIBES};
use std::collections::HashSet;
use std::thread;
use std::time::Duration;

impl AppState {
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
                    subtitle: if t.album.is_empty() {
                        "Quick Pick".into()
                    } else {
                        t.album.clone()
                    },
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

    pub fn open_now_artist(&mut self) {
        let Some(ref tr) = self.now_track else {
            return;
        };
        let raw_name = tr.artist.trim();
        if raw_name.is_empty() {
            return;
        }
        let primary = raw_name
            .split(&[',', '&'][..])
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(raw_name);
        let entry = self
            .artists_list
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(raw_name) || a.name.eq_ignore_ascii_case(primary))
            .cloned()
            .unwrap_or_else(|| ArtistEntry {
                name: primary.to_string(),
                browse_id: String::new(),
                subtitle: "Artist".into(),
                thumb_url: String::new(),
                rgb: self
                    .cover_colors
                    .get(&tr.video_id)
                    .copied()
                    .unwrap_or((190, 242, 100)),
            });
        self.open_artist_detail(entry, "now");
    }
}
