mod audio;
mod db;
mod innertube;
mod input;
mod lyrics;
mod media;
mod sdl2_platform;
mod state;
mod ui_sync;
mod web_remote;

use audio::{start_stream_playback, AudioEvent};
use db::save_settings;
use media::{fmt_time, mask_circle_rgba, mask_rounded_rgba, read_battery_label};
use sdl2_platform::SdlHost;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use state::{AppState, BgMsg, FindHit, VIBES};
use std::time::{Duration, Instant};
use web_remote::{RemoteCmd, RemoteSnapshot};

slint::include_modules!();

impl AppState {
    fn poll_background(&mut self, sdl: &mut SdlHost) {
        if self.last_battery_check.elapsed() > Duration::from_secs(15) {
            self.battery_label = read_battery_label();
            self.last_battery_check = Instant::now();
        }

        const MAX_SDL_QUEUE_BYTES: u32 = (sdl2_platform::AUDIO_FREQ as u32) * 4 * 4;
        let mut stream_err: Option<String> = None;
        if let Some(ref session) = self.session {
            while sdl.queued_audio_bytes() < MAX_SDL_QUEUE_BYTES {
                match session.rx.try_recv() {
                    Ok(AudioEvent::Pcm { pos_ms, samples }) => {
                        sdl.queue_pcm(&samples);
                        self.last_pcm_pos_ms = pos_ms;
                    }
                    Ok(AudioEvent::Started { duration_secs }) => {
                        if duration_secs > 0 && self.duration_secs == 0 {
                            self.duration_secs = duration_secs;
                        }
                    }
                    Ok(AudioEvent::Seeked(ms)) => {
                        self.last_pcm_pos_ms = ms;
                        self.elapsed_ms = ms;
                    }
                    Ok(AudioEvent::Ended) => {
                        self.stream_ended = true;
                        break;
                    }
                    Ok(AudioEvent::Error(e)) => {
                        stream_err = Some(e);
                        break;
                    }
                    Err(_) => break,
                }
            }
        }

        if self.is_playing && self.last_pcm_pos_ms > 0 {
            let queued_ms = (sdl.queued_audio_bytes() as u64 * 1000)
                / (sdl2_platform::AUDIO_FREQ as u64 * 4);
            let computed = self.last_pcm_pos_ms.saturating_sub(queued_ms);
            if computed > self.elapsed_ms {
                self.elapsed_ms = computed;
            }
        }
        if let Some(e) = stream_err {
            self.now_format = "ERR".into();
            self.is_playing = false;
            self.set_toast(format!("Audio err: {e}"));
        }
        if self.stream_ended && sdl.queued_audio_bytes() < 4096 {
            self.stream_ended = false;
            if self.loop_track {
                if let Some(cur) = self.now_track.clone() {
                    self.start_playing_track(cur, sdl, false);
                } else {
                    self.next_track(sdl);
                }
            } else {
                self.next_track(sdl);
            }
        }

        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMsg::PicksLoaded(tracks) => {
                    self.picks = tracks;
                    self.rebuild_artists_list();
                    if self.queue.is_empty() {
                        self.queue = self.picks.clone();
                    }
                }
                BgMsg::MoodsLoaded(moods) => {
                    if !moods.is_empty() {
                        self.vibes_list = moods;
                        let len = self.filtered_vibes().len();
                        Self::clamp_grid(&mut self.vibe_index, &mut self.vibe_row_scroll, len);
                    }
                }
                BgMsg::ChartArtistsLoaded(artists) => {
                    if !artists.is_empty() {
                        self.chart_artists = artists;
                        self.rebuild_artists_list();
                    }
                }
                BgMsg::ArtistsSearchLoaded { query, artists } => {
                    if self.artist_filter.trim().eq_ignore_ascii_case(&query) {
                        self.searched_artists = artists;
                        self.rebuild_artists_list();
                        self.artist_index = 0;
                        self.artist_row_scroll = 0;
                    }
                }
                BgMsg::VibeLoaded { vibe_id, tracks } => {
                    if self.active_tab == "vibe_detail" {
                        let matches = self
                            .vibes_list
                            .iter()
                            .any(|v| v.id == vibe_id && v.title == self.detail_title)
                            || VIBES
                                .iter()
                                .any(|v| v.id == vibe_id && v.title == self.detail_title);
                        if matches {
                            self.detail_tracks = tracks;
                            self.clamp_list(self.detail_tracks.len());
                        }
                    }
                }
                BgMsg::ArtistPageLoaded { artist, page } => {
                    if self.artist_title == artist {
                        let mut thumbs = Vec::new();
                        if self.artist_thumb_url.is_empty() && !page.thumb_url.is_empty() {
                            self.artist_thumb_url = page.thumb_url.clone();
                            thumbs.push(page.thumb_url);
                        }
                        for al in page.albums.iter().chain(page.singles.iter()).take(20) {
                            if !al.thumb_url.is_empty() {
                                thumbs.push(al.thumb_url.clone());
                            }
                        }
                        self.queue_thumb_urls(thumbs);

                        self.artist_songs = page.songs;
                        self.artist_albums = page.albums;
                        self.artist_singles = page.singles;
                        self.artist_stats = if page.subtitle.is_empty() {
                            format!(
                                "{} top songs · {} albums · {} singles",
                                self.artist_songs.len(),
                                self.artist_albums.len(),
                                self.artist_singles.len()
                            )
                        } else {
                            format!(
                                "{} · {} songs · {} albums",
                                page.subtitle,
                                self.artist_songs.len(),
                                self.artist_albums.len()
                            )
                        };
                        if self.active_tab == "artist_detail" {
                            let len = self.artist_active_len();
                            self.clamp_list(len);
                        }
                    }
                }
                BgMsg::AlbumLoaded { browse_id, tracks } => {
                    if self.active_tab == "album_detail" && self.detail_browse_id == browse_id {
                        self.detail_tracks = tracks;
                        self.clamp_list(self.detail_tracks.len());
                    }
                }
                BgMsg::SearchResults { query, hits } => {
                    self.search_query = query;
                    self.find_hits = hits;
                    self.find_hit_index = 0;
                    self.find_hit_scroll = 0;
                    if !self.find_hits.is_empty() {
                        self.find_zone = 1;
                    }
                }
                BgMsg::SuggestionsLoaded { query, items } => {
                    if self.search_query.trim().eq_ignore_ascii_case(&query) {
                        self.suggest_chips = items;
                        self.chip_index = 0;
                    }
                }
                BgMsg::LyricsLoaded { video_id, lyrics } => {
                    self.lyrics_cache.insert(video_id, lyrics);
                }
                BgMsg::StreamResolved {
                    track,
                    url,
                    user_agent,
                    bitrate,
                    duration_secs,
                    content_length,
                } => {
                    if self.now_track.as_ref().is_some_and(|t| t.video_id == track.video_id) {
                        if duration_secs > 0 {
                            self.duration_secs = duration_secs;
                        }
                        let kbps = (bitrate / 1000).max(64);
                        self.now_format = format!("AAC {kbps}k");
                        self.session = Some(start_stream_playback(url, user_agent, content_length));
                    }
                }
                BgMsg::StreamError(e) => {
                    self.now_format = "ERR".into();
                    self.is_playing = false;
                    self.set_toast(format!("Stream error: {e}"));
                }
                BgMsg::RadioLoaded { seed_id, tracks } => {
                    if self.now_track.as_ref().is_some_and(|t| t.video_id == seed_id) {
                        for r in tracks {
                            if !self.queue.iter().any(|q| q.video_id == r.video_id) {
                                self.queue.push(r);
                            }
                        }
                    }
                }
                BgMsg::CoverLoaded {
                    video_id,
                    width,
                    height,
                    rgba,
                    accent_rgb,
                } => {
                    const MAX_COVERS: usize = 40;
                    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
                    buf.make_mut_bytes().copy_from_slice(&rgba);
                    if !self.cover_cache.contains_key(&video_id) {
                        self.cover_order.push_back(video_id.clone());
                        while self.cover_order.len() > MAX_COVERS {
                            if let Some(old) = self.cover_order.pop_front() {
                                self.cover_cache.remove(&old);
                                self.cover_colors.remove(&old);
                            }
                        }
                    }
                    self.cover_cache.insert(video_id.clone(), Image::from_rgba8(buf));
                    if let Some(rgb) = accent_rgb {
                        self.cover_colors.insert(video_id, rgb);
                    }
                }
                BgMsg::ThumbLoaded {
                    url,
                    width,
                    height,
                    rgba,
                } => {
                    const MAX_THUMBS: usize = 72;
                    self.thumb_fetching.remove(&url);
                    if !self.thumb_cache.contains_key(&url) {
                        self.thumb_order.push_back(url.clone());
                        while self.thumb_order.len() > MAX_THUMBS {
                            if let Some(old) = self.thumb_order.pop_front() {
                                self.thumb_cache.remove(&old);
                                self.circle_thumb_cache.remove(&old);
                            }
                        }
                    }
                    self.thumb_cache
                        .insert(url.clone(), mask_rounded_rgba(&rgba, width, height));
                    self.circle_thumb_cache
                        .insert(url, mask_circle_rgba(&rgba, width, height));
                }
                BgMsg::OfflineCached { track, path, bytes } => {
                    self.db.set_offline(&track, Some(&path), bytes);
                    if let Some(ref mut cur) = self.now_track {
                        if cur.video_id == track.video_id {
                            cur.local_path = Some(path.clone());
                            cur.file_bytes = bytes;
                            self.now_format = "M4A 128k".into();
                        }
                    }
                    for row in self.detail_tracks.iter_mut().chain(self.artist_songs.iter_mut()) {
                        if row.video_id == track.video_id {
                            row.local_path = Some(path.clone());
                        }
                    }
                    for hit in &mut self.find_hits {
                        if let FindHit::Song(row) = hit {
                            if row.video_id == track.video_id {
                                row.local_path = Some(path.clone());
                            }
                        }
                    }
                    self.refresh_lib_tracks();
                    self.set_toast(format!("Saved: {}", track.title));
                }
                BgMsg::Toast(t) => self.set_toast(t),
            }
        }

        self.poll_remote(sdl);

        if let Some((_, until)) = self.toast {
            if Instant::now() >= until {
                self.toast = None;
            }
        }
    }

    fn poll_remote(&mut self, sdl: &SdlHost) {
        let cmds: Vec<RemoteCmd> = self
            .remote
            .as_ref()
            .map(|r| std::iter::from_fn(|| r.rx.try_recv().ok()).collect())
            .unwrap_or_default();

        for cmd in cmds {
            match cmd {
                RemoteCmd::PlayInput(q) => self.trigger_search(q),
                RemoteCmd::PlayTrack(tr) => self.start_playing_track(tr, sdl, false),
                RemoteCmd::PlayPlaylist(tracks, name) => {
                    if let Some(first) = tracks.first().cloned() {
                        self.queue = tracks;
                        self.queue_label = format!("PLAYLIST · {name}").to_uppercase();
                        self.start_playing_track(first, sdl, false);
                    }
                }
                RemoteCmd::BatchDownload(tracks) => self.download_batch(tracks),
                RemoteCmd::QueueDownload(vid) => {
                    if let Some(tr) = self
                        .db
                        .list_recent(100)
                        .into_iter()
                        .chain(self.db.list_favorites())
                        .find(|t| t.video_id == vid)
                    {
                        self.cache_track_offline(tr);
                    }
                }
                RemoteCmd::RefreshLibrary => self.refresh_lib_tracks(),
                RemoteCmd::TogglePlay => self.toggle_play_pause(sdl),
                RemoteCmd::Next => self.next_track(sdl),
                RemoteCmd::Prev => self.prev_track(sdl),
                RemoteCmd::ToggleLoop => {
                    self.loop_track = !self.loop_track;
                    self.set_toast(if self.loop_track { "Loop Song: ON" } else { "Loop Song: OFF" });
                }
                RemoteCmd::ToggleTheme => {
                    self.cfg.theme = if self.cfg.theme == "dark" { "light".into() } else { "dark".into() };
                    save_settings(&self.settings_path, &self.cfg);
                }
                RemoteCmd::ShutdownServer => {
                    if self.remote.is_some() {
                        self.toggle_web_server();
                    }
                }
            }
        }

        if let Some(ref r) = self.remote {
            if let Ok(mut snap) = r.snapshot.try_lock() {
                let offline = self.db.list_offline();
                let offline_bytes: u64 = offline.iter().map(|t| t.file_bytes).sum();
                let orphans = self.db.list_orphaned_offline();
                let orphan_bytes: u64 = orphans.iter().map(|t| t.file_bytes).sum();
                *snap = RemoteSnapshot {
                    video_id: self.now_track.as_ref().map(|t| t.video_id.clone()).unwrap_or_default(),
                    title: self.now_track.as_ref().map(|t| t.title.clone()).unwrap_or_default(),
                    artist: self.now_track.as_ref().map(|t| t.artist.clone()).unwrap_or_default(),
                    album: self.now_track.as_ref().map(|t| t.album.clone()).unwrap_or_default(),
                    playing: self.is_playing,
                    loop_track: self.loop_track,
                    elapsed_secs: self.elapsed_ms / 1000,
                    duration_secs: self.duration_secs,
                    elapsed_str: fmt_time(self.elapsed_ms / 1000),
                    duration_str: fmt_time(self.duration_secs),
                    theme: self.cfg.theme.clone(),
                    battery: self.battery_label.clone(),
                    offline_count: offline.len(),
                    offline_bytes,
                    orphan_count: orphans.len(),
                    orphan_bytes,
                    fav_count: self.db.list_favorites().len(),
                    server_url: r.url.clone(),
                    queue: self
                        .queue
                        .iter()
                        .take(15)
                        .map(|t| format!("{} — {}", t.title, t.artist))
                        .collect(),
                };
            }
        }
    }
}

fn main() {
    let mut sdl = SdlHost::init().expect("SDL2 init failed");
    let mut state = AppState::new().expect("AppState init failed");
    sdl.swap_ab = state.cfg.swap_ab;

    let win = YtMusicWindow::new().expect("Slint window creation failed");
    state.sync_ui(&win);
    sdl.render_if_needed(true);

    while !sdl.should_quit {
        let wait_ms = if state.is_playing { 16 } else { 40 };
        let btns = sdl.poll_actions(wait_ms);
        for b in btns {
            state.handle_button(b, &mut sdl);
        }
        state.poll_background(&mut sdl);
        state.sync_ui(&win);
        sdl.render_if_needed(false);
    }
}
