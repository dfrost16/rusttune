use crate::db::save_settings;
use crate::sdl2_platform::{HandheldBtn, PlaybackHost};
use crate::state::{AppState, BgMsg, ConfirmAction, FindHit, DOCK_TABS, NUM_KEYS, QWERTY_KEYS};
use std::thread;

impl AppState {
    pub fn handle_button<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &mut H) {
        if self.pocket_lock {
            if btn == HandheldBtn::L3 || btn == HandheldBtn::Start {
                self.pocket_lock = false;
            }
            return;
        }

        if btn == HandheldBtn::L3 {
            self.pocket_lock = true;
            return;
        }

        if btn == HandheldBtn::Select {
            self.settings_open = !self.settings_open;
            self.settings_index = 0;
            self.pl_modal_open = false;
            self.confirm_action = None;
            return;
        }

        if self.handle_confirm_modal(btn) {
            return;
        }
        if self.handle_settings_modal(btn, sdl) {
            return;
        }
        if self.handle_playlist_modal(btn) {
            return;
        }

        if btn == HandheldBtn::Start {
            if let Some(tr) = self.current_selected_track() {
                self.open_pl_modal(tr);
            } else {
                self.settings_open = true;
                self.settings_index = 0;
            }
            return;
        }

        if self.handle_grid_kbd(btn) {
            return;
        }

        if btn == HandheldBtn::L1 || btn == HandheldBtn::R1 {
            self.switch_dock_tab(btn == HandheldBtn::R1);
            return;
        }
        if btn == HandheldBtn::L2 {
            self.prev_track(sdl);
            return;
        }
        if btn == HandheldBtn::R2 {
            self.next_track(sdl);
            return;
        }

        match self.active_tab.as_str() {
            "now" => self.handle_now_button(btn, sdl),
            "vibes" => self.handle_vibes_button(btn),
            "artists" => self.handle_artists_button(btn),
            "vibe_detail" | "album_detail" => self.handle_detail_button(btn, sdl),
            "artist_detail" => self.handle_artist_detail_button(btn, sdl),
            "lib" => self.handle_lib_button(btn, sdl),
            "find" => self.handle_find_button(btn, sdl),
            _ => {}
        }
    }

    fn handle_confirm_modal(&mut self, btn: HandheldBtn) -> bool {
        let Some(act) = self.confirm_action.clone() else {
            return false;
        };
        match btn {
            HandheldBtn::A => {
                self.confirm_action = None;
                match act {
                    ConfirmAction::DownloadBatch { tracks, .. } => self.download_batch(tracks),
                    ConfirmAction::ClearAllDownloads { .. } => {
                        let (cnt, freed) = self.db.clear_all_offline();
                        self.detail_top_focused = false;
                        self.refresh_lib_tracks();
                        let mb = (freed as f64) / 1_048_576.0;
                        self.set_toast(format!("Cleared {cnt} files ({mb:.1} MB)"));
                    }
                }
            }
            HandheldBtn::B | HandheldBtn::Start => self.confirm_action = None,
            _ => {}
        }
        true
    }

    fn handle_settings_modal<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &mut H) -> bool {
        if !self.settings_open {
            return false;
        }
        const SETTINGS_COUNT: usize = 7;
        match btn {
            HandheldBtn::Up => self.settings_index = (self.settings_index + SETTINGS_COUNT - 1) % SETTINGS_COUNT,
            HandheldBtn::Down => self.settings_index = (self.settings_index + 1) % SETTINGS_COUNT,
            HandheldBtn::B | HandheldBtn::Select => self.settings_open = false,
            HandheldBtn::X if self.settings_index == 5 => self.rotate_web_pin(),
            HandheldBtn::A => match self.settings_index {
                0 => {
                    self.cfg.theme = if self.cfg.theme == "dark" { "light".into() } else { "dark".into() };
                    save_settings(&self.settings_path, &self.cfg);
                }
                1 => {
                    self.loop_track = !self.loop_track;
                    self.set_toast(if self.loop_track { "Loop Song: ON" } else { "Loop Song: OFF" });
                }
                2 => {
                    self.cfg.radio_autoplay = !self.cfg.radio_autoplay;
                    save_settings(&self.settings_path, &self.cfg);
                }
                3 => {
                    self.cfg.swap_ab = !self.cfg.swap_ab;
                    sdl.set_swap_ab(self.cfg.swap_ab);
                    save_settings(&self.settings_path, &self.cfg);
                }
                4 => self.toggle_web_server(),
                5 => self.toggle_web_pin(),
                6 => sdl.request_quit(),
                _ => {}
            },
            _ => {}
        }
        true
    }

    fn handle_playlist_modal(&mut self, btn: HandheldBtn) -> bool {
        if !self.pl_modal_open {
            return false;
        }
        let total = self.playlists.len() + 1;
        match btn {
            HandheldBtn::Up => self.pl_modal_index = (self.pl_modal_index + total - 1) % total,
            HandheldBtn::Down => self.pl_modal_index = (self.pl_modal_index + 1) % total,
            HandheldBtn::B | HandheldBtn::Start => self.pl_modal_open = false,
            HandheldBtn::A => {
                if let Some(tr) = self.pl_target_track.clone() {
                    if self.pl_modal_index == self.playlists.len() {
                        let name = format!("Mix #{}", self.playlists.len() + 1);
                        if let Some(id) = self.db.create_playlist(&name) {
                            self.db.toggle_track_in_playlist(id, &tr);
                            self.refresh_lib_tracks();
                            self.pl_modal_open = false;
                            self.set_toast(format!("Created {name}"));
                        }
                    } else if let Some(pl) = self.playlists.get(self.pl_modal_index).cloned() {
                        let added = self.db.toggle_track_in_playlist(pl.id, &tr);
                        self.refresh_lib_tracks();
                        self.set_toast(if added {
                            format!("Added to {}", pl.name)
                        } else {
                            format!("Removed from {}", pl.name)
                        });
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn handle_grid_kbd(&mut self, btn: HandheldBtn) -> bool {
        if !self.grid_kbd_open {
            return false;
        }
        let cur_tab = self.active_tab.clone();
        let push_char = |s: &mut Self, ch: &str| match cur_tab.as_str() {
            "vibes" => s.vibe_filter.push_str(ch),
            "lib" => {
                s.lib_filter.push_str(ch);
                s.refresh_lib_tracks();
            }
            _ => s.artist_filter.push_str(ch),
        };
        let pop_char = |s: &mut Self| match cur_tab.as_str() {
            "vibes" => {
                s.vibe_filter.pop();
            }
            "lib" => {
                s.lib_filter.pop();
                s.refresh_lib_tracks();
            }
            _ => {
                s.artist_filter.pop();
            }
        };

        match btn {
            HandheldBtn::Left => self.key_col = (self.key_col + 9) % 10,
            HandheldBtn::Right => self.key_col = (self.key_col + 1) % 10,
            HandheldBtn::Up => self.key_row = (self.key_row + 2) % 3,
            HandheldBtn::Down => self.key_row = (self.key_row + 1) % 3,
            HandheldBtn::R3 => self.kbd_num_mode = !self.kbd_num_mode,
            HandheldBtn::X => push_char(self, " "),
            HandheldBtn::Y => pop_char(self),
            HandheldBtn::B => {
                self.grid_kbd_open = false;
                self.filter_focused = false;
            }
            HandheldBtn::A => {
                let key = if self.kbd_num_mode {
                    NUM_KEYS[self.key_row * 10 + self.key_col]
                } else {
                    QWERTY_KEYS[self.key_row * 10 + self.key_col]
                };
                match key {
                    "123" => self.kbd_num_mode = true,
                    "ABC" => self.kbd_num_mode = false,
                    "SPC" => push_char(self, " "),
                    "DEL" => pop_char(self),
                    "CLR" => match cur_tab.as_str() {
                        "vibes" => {
                            self.vibe_filter.clear();
                            self.vibe_index = 0;
                            self.vibe_row_scroll = 0;
                        }
                        "lib" => {
                            self.lib_filter.clear();
                            self.refresh_lib_tracks();
                        }
                        _ => {
                            self.artist_filter.clear();
                            self.searched_artists.clear();
                            self.rebuild_artists_list();
                            self.artist_index = 0;
                            self.artist_row_scroll = 0;
                        }
                    },
                    "GO" => {
                        self.grid_kbd_open = false;
                        self.filter_focused = false;
                        if cur_tab == "artists" {
                            let q = self.artist_filter.trim().to_string();
                            if q.is_empty() {
                                self.searched_artists.clear();
                                self.rebuild_artists_list();
                            } else {
                                let it = self.innertube.clone();
                                let tx = self.bg_tx.clone();
                                self.set_toast(format!("Searching artist: {q}"));
                                thread::spawn(move || {
                                    if let Ok(artists) = it.search_artists(&q) {
                                        let _ = tx.send(BgMsg::ArtistsSearchLoaded { query: q, artists });
                                    }
                                });
                            }
                        } else if cur_tab == "lib" {
                            self.refresh_lib_tracks();
                        }
                    }
                    ch => push_char(self, &ch.to_lowercase()),
                }
            }
            _ => {}
        }

        if cur_tab == "vibes" {
            let len = self.filtered_vibes().len();
            Self::clamp_grid(&mut self.vibe_index, &mut self.vibe_row_scroll, len);
        } else if cur_tab == "artists" {
            let len = self.filtered_artists().len();
            Self::clamp_grid(&mut self.artist_index, &mut self.artist_row_scroll, len);
            self.ensure_visible_artist_thumbs();
        }
        true
    }

    fn switch_dock_tab(&mut self, forward: bool) {
        self.grid_kbd_open = false;
        self.detail_top_focused = false;
        self.filter_focused = false;
        self.now_artist_focused = false;
        let cur_dock = match self.active_tab.as_str() {
            "vibe_detail" => "vibes",
            "artist_detail" | "album_detail" => {
                if self.detail_origin == "find" { "find" } else { "artists" }
            }
            other => other,
        };
        let idx = DOCK_TABS.iter().position(|&t| t == cur_dock).unwrap_or(0);
        let next = if forward {
            (idx + 1) % DOCK_TABS.len()
        } else {
            (idx + DOCK_TABS.len() - 1) % DOCK_TABS.len()
        };
        self.active_tab = DOCK_TABS[next].into();
        if self.active_tab == "lib" {
            self.refresh_lib_tracks();
        } else if self.active_tab == "artists" {
            self.ensure_visible_artist_thumbs();
        }
    }

    fn seek_relative<H: PlaybackHost>(&mut self, sdl: &H, delta_secs: i64) {
        let Some(ref s) = self.session else {
            return;
        };
        let cur = (self.elapsed_ms / 1000) as i64;
        let target = (cur + delta_secs).max(0) as u64;
        sdl.clear_audio_queue();
        s.seek_to(target);
        self.elapsed_ms = target * 1000;
        self.last_pcm_pos_ms = target * 1000;
        self.stream_ended = false;
        let sign = if delta_secs >= 0 { "+" } else { "" };
        self.set_toast(format!("Seek {sign}{delta_secs}s"));
    }

    fn handle_now_button<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &H) {
        match btn {
            HandheldBtn::RightUp | HandheldBtn::RightRight | HandheldBtn::RightLeft => {
                if self.now_track.is_some() {
                    self.now_artist_focused = true;
                }
            }
            HandheldBtn::RightDown | HandheldBtn::B => {
                self.now_artist_focused = false;
            }
            HandheldBtn::A => {
                if self.now_artist_focused && self.now_track.is_some() {
                    self.now_artist_focused = false;
                    self.open_now_artist();
                } else {
                    self.toggle_play_pause(sdl);
                }
            }
            HandheldBtn::Up => self.prev_track(sdl),
            HandheldBtn::Down => self.next_track(sdl),
            HandheldBtn::Left => self.seek_relative(sdl, -10),
            HandheldBtn::Right => self.seek_relative(sdl, 10),
            HandheldBtn::X => {
                if let Some(ref mut t) = self.now_track {
                    let fav = self.db.toggle_favorite(t);
                    t.is_favorite = fav;
                    self.refresh_lib_tracks();
                    self.set_toast(if fav { "Saved Favorite" } else { "Removed Favorite" });
                }
            }
            HandheldBtn::Y => {
                if let Some(t) = self.now_track.clone() {
                    self.cache_track_offline(t);
                }
            }
            HandheldBtn::R3 => {
                self.loop_track = !self.loop_track;
                self.set_toast(if self.loop_track { "Loop Song: ON" } else { "Loop Song: OFF" });
            }
            _ => {}
        }
    }

    fn nav_grid_index(index: &mut usize, scroll: &mut usize, len: usize, btn: HandheldBtn) -> bool {
        if len == 0 {
            return false;
        }
        match btn {
            HandheldBtn::Right | HandheldBtn::RightRight => *index = (*index + 1) % len,
            HandheldBtn::Left | HandheldBtn::RightLeft => *index = (*index + len - 1) % len,
            HandheldBtn::Down | HandheldBtn::RightDown => {
                *index = if *index + 3 < len { *index + 3 } else { *index % 3 }
            }
            HandheldBtn::Up | HandheldBtn::RightUp => {
                if *index >= 3 {
                    *index -= 3;
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        Self::clamp_grid(index, scroll, len);
        true
    }

    fn open_grid_keyboard(&mut self) {
        self.key_row = 0;
        self.key_col = 0;
        self.kbd_num_mode = false;
        self.grid_kbd_open = true;
    }

    fn handle_vibes_button(&mut self, btn: HandheldBtn) {
        if self.filter_focused {
            match btn {
                HandheldBtn::A | HandheldBtn::R3 => self.open_grid_keyboard(),
                HandheldBtn::Down | HandheldBtn::RightDown | HandheldBtn::B => {
                    self.filter_focused = false;
                }
                HandheldBtn::X => {
                    self.vibe_filter.clear();
                    self.vibe_index = 0;
                    self.vibe_row_scroll = 0;
                }
                HandheldBtn::Y => {
                    self.vibe_fav_only = !self.vibe_fav_only;
                    self.vibe_index = 0;
                    self.vibe_row_scroll = 0;
                }
                _ => {}
            }
            return;
        }
        let list = self.filtered_vibes();
        let len = list.len();
        if Self::nav_grid_index(&mut self.vibe_index, &mut self.vibe_row_scroll, len, btn) {
            return;
        }
        match btn {
            HandheldBtn::Up | HandheldBtn::RightUp => self.filter_focused = true,
            HandheldBtn::X if len > 0 => {
                let id = list[self.vibe_index].id.clone();
                let on = self.db.toggle_fav_vibe(&id);
                if on {
                    self.fav_vibes.insert(id);
                    self.set_toast("Favorited Vibe");
                } else {
                    self.fav_vibes.remove(&id);
                    self.set_toast("Removed Vibe Favorite");
                }
            }
            HandheldBtn::Y => {
                self.vibe_fav_only = !self.vibe_fav_only;
                self.vibe_index = 0;
                self.vibe_row_scroll = 0;
            }
            HandheldBtn::R3 => self.open_grid_keyboard(),
            HandheldBtn::A if len > 0 => self.open_vibe_detail(list[self.vibe_index].clone()),
            HandheldBtn::B => self.active_tab = "now".into(),
            _ => {}
        }
    }

    fn handle_artists_button(&mut self, btn: HandheldBtn) {
        if self.filter_focused {
            match btn {
                HandheldBtn::A | HandheldBtn::R3 => self.open_grid_keyboard(),
                HandheldBtn::Down | HandheldBtn::RightDown | HandheldBtn::B => {
                    self.filter_focused = false;
                }
                HandheldBtn::X => {
                    self.artist_filter.clear();
                    self.artist_index = 0;
                    self.artist_row_scroll = 0;
                    self.ensure_visible_artist_thumbs();
                }
                HandheldBtn::Y => {
                    self.artist_fav_only = !self.artist_fav_only;
                    self.artist_index = 0;
                    self.artist_row_scroll = 0;
                    self.ensure_visible_artist_thumbs();
                }
                _ => {}
            }
            return;
        }
        let list = self.filtered_artists();
        let len = list.len();
        if Self::nav_grid_index(&mut self.artist_index, &mut self.artist_row_scroll, len, btn) {
            self.ensure_visible_artist_thumbs();
            return;
        }
        match btn {
            HandheldBtn::Up | HandheldBtn::RightUp => self.filter_focused = true,
            HandheldBtn::X if len > 0 => {
                let name = list[self.artist_index].name.clone();
                let on = self.db.toggle_fav_artist(&name);
                if on {
                    self.fav_artists.insert(name);
                    self.set_toast("Favorited Artist");
                } else {
                    self.fav_artists.remove(&name);
                    self.set_toast("Removed Artist Favorite");
                }
            }
            HandheldBtn::Y => {
                self.artist_fav_only = !self.artist_fav_only;
                self.artist_index = 0;
                self.artist_row_scroll = 0;
                self.ensure_visible_artist_thumbs();
            }
            HandheldBtn::R3 => self.open_grid_keyboard(),
            HandheldBtn::A if len > 0 => self.open_artist_detail(list[self.artist_index].clone(), "artists"),
            HandheldBtn::B => self.active_tab = "now".into(),
            _ => {}
        }
    }

    fn handle_detail_button<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &H) {
        let len = self.detail_tracks.len();
        match btn {
            HandheldBtn::Up if len > 0 => {
                if self.detail_top_focused {
                    self.detail_top_focused = false;
                    self.list_index = len - 1;
                    self.clamp_list(len);
                } else if self.list_index == 0 {
                    self.detail_top_focused = true;
                } else {
                    self.list_index -= 1;
                    self.clamp_list(len);
                }
            }
            HandheldBtn::Down if len > 0 => {
                if self.detail_top_focused {
                    self.detail_top_focused = false;
                    self.list_index = 0;
                } else {
                    self.list_index = (self.list_index + 1) % len;
                }
                self.clamp_list(len);
            }
            HandheldBtn::RightUp if len > 0 => {
                self.detail_top_focused = false;
                self.list_index = self.list_index.saturating_sub(5);
                self.clamp_list(len);
            }
            HandheldBtn::RightDown if len > 0 => {
                self.detail_top_focused = false;
                self.list_index = (self.list_index + 5).min(len - 1);
                self.clamp_list(len);
            }
            HandheldBtn::A => {
                if self.detail_top_focused && len > 0 {
                    let pending = self
                        .detail_tracks
                        .iter()
                        .filter(|t| self.db.get_local_path(&t.video_id).is_none())
                        .count();
                    if pending == 0 {
                        self.set_toast("All tracks already downloaded");
                    } else {
                        self.confirm_action = Some(ConfirmAction::DownloadBatch {
                            title: self.detail_title.clone(),
                            tracks: self.detail_tracks.clone(),
                        });
                    }
                } else if let Some(tr) = self.detail_tracks.get(self.list_index).cloned() {
                    self.queue = self.detail_tracks.clone();
                    let kind = if self.active_tab == "vibe_detail" { "VIBE" } else { "ALBUM" };
                    self.queue_label = format!("{kind} · {}", self.detail_title).to_uppercase();
                    if self.active_tab == "vibe_detail" {
                        self.now_genre = self.detail_title.to_uppercase();
                    }
                    self.start_playing_track(tr, sdl, false);
                }
            }
            HandheldBtn::X => {
                if let Some(tr) = self.detail_tracks.get_mut(self.list_index) {
                    let fav = self.db.toggle_favorite(tr);
                    tr.is_favorite = fav;
                    self.set_toast(if fav { "Saved Favorite" } else { "Removed Favorite" });
                }
            }
            HandheldBtn::Y => {
                if let Some(tr) = self.detail_tracks.get(self.list_index).cloned() {
                    self.cache_track_offline(tr);
                }
            }
            HandheldBtn::B => {
                self.detail_top_focused = false;
                self.active_tab = self.detail_origin.clone();
                if self.active_tab == "artist_detail" {
                    let len = self.artist_active_len();
                    self.list_index = 0;
                    self.list_scroll = 0;
                    self.clamp_list(len);
                }
            }
            _ => {}
        }
    }

    fn handle_artist_detail_button<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &H) {
        let len = self.artist_active_len();
        match btn {
            HandheldBtn::Left | HandheldBtn::RightLeft => {
                self.artist_section = (self.artist_section + 2) % 3;
                self.detail_top_focused = false;
                self.list_index = 0;
                self.list_scroll = 0;
            }
            HandheldBtn::Right | HandheldBtn::RightRight => {
                self.artist_section = (self.artist_section + 1) % 3;
                self.detail_top_focused = false;
                self.list_index = 0;
                self.list_scroll = 0;
            }
            HandheldBtn::Up if len > 0 => {
                if self.artist_section == 0 && !self.detail_top_focused && self.list_index == 0 {
                    self.detail_top_focused = true;
                } else if self.detail_top_focused {
                    self.detail_top_focused = false;
                    self.list_index = len - 1;
                    self.clamp_list(len);
                } else {
                    self.list_index = (self.list_index + len - 1) % len;
                    self.clamp_list(len);
                }
            }
            HandheldBtn::Down if len > 0 => {
                if self.detail_top_focused {
                    self.detail_top_focused = false;
                    self.list_index = 0;
                } else {
                    self.list_index = (self.list_index + 1) % len;
                }
                self.clamp_list(len);
            }
            HandheldBtn::RightUp if len > 0 => {
                self.detail_top_focused = false;
                self.list_index = self.list_index.saturating_sub(5);
                self.clamp_list(len);
            }
            HandheldBtn::RightDown if len > 0 => {
                self.detail_top_focused = false;
                self.list_index = (self.list_index + 5).min(len - 1);
                self.clamp_list(len);
            }
            HandheldBtn::A => {
                if self.detail_top_focused && self.artist_section == 0 && !self.artist_songs.is_empty() {
                    let pending = self
                        .artist_songs
                        .iter()
                        .filter(|t| self.db.get_local_path(&t.video_id).is_none())
                        .count();
                    if pending == 0 {
                        self.set_toast("All tracks already downloaded");
                    } else {
                        self.confirm_action = Some(ConfirmAction::DownloadBatch {
                            title: self.artist_title.clone(),
                            tracks: self.artist_songs.clone(),
                        });
                    }
                } else {
                    match self.artist_section {
                        0 => {
                            if let Some(tr) = self.artist_songs.get(self.list_index).cloned() {
                                self.queue = self.artist_songs.clone();
                                self.queue_label = format!("ARTIST · {}", self.artist_title).to_uppercase();
                                self.start_playing_track(tr, sdl, false);
                            }
                        }
                        1 => {
                            if let Some(al) = self.artist_albums.get(self.list_index).cloned() {
                                self.open_album_detail(al, "artist_detail");
                            }
                        }
                        _ => {
                            if let Some(sg) = self.artist_singles.get(self.list_index).cloned() {
                                self.open_album_detail(sg, "artist_detail");
                            }
                        }
                    }
                }
            }
            HandheldBtn::X => {
                if self.artist_section == 0 && !self.detail_top_focused {
                    if let Some(tr) = self.artist_songs.get_mut(self.list_index) {
                        let fav = self.db.toggle_favorite(tr);
                        tr.is_favorite = fav;
                        self.set_toast(if fav { "Saved Favorite" } else { "Removed Favorite" });
                    }
                } else {
                    let name = self.artist_title.clone();
                    let on = self.db.toggle_fav_artist(&name);
                    if on {
                        self.fav_artists.insert(name);
                        self.set_toast("Favorited Artist");
                    } else {
                        self.fav_artists.remove(&name);
                        self.set_toast("Removed Artist Favorite");
                    }
                    self.rebuild_artists_list();
                }
            }
            HandheldBtn::Y if self.artist_section == 0 && !self.detail_top_focused => {
                if let Some(tr) = self.artist_songs.get(self.list_index).cloned() {
                    self.cache_track_offline(tr);
                }
            }
            HandheldBtn::B => {
                self.detail_top_focused = false;
                self.active_tab = self.artist_origin.clone();
            }
            _ => {}
        }
    }

    fn handle_lib_button<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &H) {
        const LIB_MODES: [&str; 4] = ["favs", "playlists", "history", "downloads"];
        let switch_lib = |cur: &str, delta: usize| -> String {
            let i = LIB_MODES.iter().position(|&m| m == cur).unwrap_or(0);
            LIB_MODES[(i + delta) % LIB_MODES.len()].into()
        };

        match btn {
            HandheldBtn::RightLeft | HandheldBtn::RightRight => {
                let delta = if btn == HandheldBtn::RightRight { 1 } else { 3 };
                self.lib_mode = switch_lib(&self.lib_mode, delta);
                self.detail_top_focused = false;
                self.filter_focused = false;
                self.lib_focus_playlists = true;
                self.list_index = 0;
                self.list_scroll = 0;
                self.refresh_lib_tracks();
                return;
            }
            HandheldBtn::R3 => {
                self.open_grid_keyboard();
                return;
            }
            HandheldBtn::B => {
                if self.filter_focused {
                    self.filter_focused = false;
                } else if self.detail_top_focused {
                    self.detail_top_focused = false;
                } else if self.lib_mode == "playlists" && !self.lib_focus_playlists {
                    self.lib_focus_playlists = true;
                } else {
                    self.active_tab = "now".into();
                }
                return;
            }
            _ => {}
        }

        if self.filter_focused {
            match btn {
                HandheldBtn::A => self.open_grid_keyboard(),
                HandheldBtn::Down | HandheldBtn::RightDown => self.filter_focused = false,
                HandheldBtn::X => {
                    self.lib_filter.clear();
                    self.list_index = 0;
                    self.list_scroll = 0;
                    self.refresh_lib_tracks();
                }
                _ => {}
            }
            return;
        }

        if self.lib_mode == "playlists" && self.lib_focus_playlists {
            let plen = self.playlists.len();
            match btn {
                HandheldBtn::Left => {
                    self.lib_mode = switch_lib(&self.lib_mode, 3);
                    self.list_index = 0;
                    self.list_scroll = 0;
                    self.refresh_lib_tracks();
                }
                HandheldBtn::Up | HandheldBtn::RightUp => {
                    if plen == 0 || self.playlist_index == 0 {
                        self.filter_focused = true;
                    } else {
                        self.playlist_index -= 1;
                        self.refresh_lib_tracks();
                    }
                }
                HandheldBtn::Down | HandheldBtn::RightDown if plen > 0 => {
                    self.playlist_index = (self.playlist_index + 1) % plen;
                    self.refresh_lib_tracks();
                }
                HandheldBtn::Right | HandheldBtn::A if !self.lib_tracks.is_empty() => {
                    self.lib_focus_playlists = false;
                    self.list_index = 0;
                    self.list_scroll = 0;
                }
                _ => {}
            }
            return;
        }

        let len = self.lib_tracks.len();
        match btn {
            HandheldBtn::Left => {
                if self.lib_mode == "playlists" {
                    self.lib_focus_playlists = true;
                } else {
                    self.lib_mode = switch_lib(&self.lib_mode, 3);
                    self.detail_top_focused = false;
                    self.list_index = 0;
                    self.list_scroll = 0;
                    self.refresh_lib_tracks();
                }
            }
            HandheldBtn::Right if self.lib_mode != "playlists" => {
                self.lib_mode = switch_lib(&self.lib_mode, 1);
                self.detail_top_focused = false;
                self.lib_focus_playlists = true;
                self.list_index = 0;
                self.list_scroll = 0;
                self.refresh_lib_tracks();
            }
            HandheldBtn::Up | HandheldBtn::RightUp => {
                if len == 0 {
                    self.filter_focused = true;
                } else if self.detail_top_focused {
                    self.detail_top_focused = false;
                    self.filter_focused = true;
                } else if self.list_index == 0 {
                    if self.lib_mode == "downloads" {
                        self.detail_top_focused = true;
                    } else {
                        self.filter_focused = true;
                    }
                } else {
                    self.list_index -= 1;
                    self.clamp_list(len);
                }
            }
            HandheldBtn::Down | HandheldBtn::RightDown if len > 0 => {
                if self.detail_top_focused {
                    self.detail_top_focused = false;
                    self.list_index = 0;
                } else {
                    self.list_index = (self.list_index + 1) % len;
                }
                self.clamp_list(len);
            }
            HandheldBtn::A => {
                if self.detail_top_focused && self.lib_mode == "downloads" {
                    let files = self.db.list_offline();
                    if !files.is_empty() {
                        let bytes: u64 = files.iter().map(|t| t.file_bytes).sum();
                        self.confirm_action = Some(ConfirmAction::ClearAllDownloads {
                            count: files.len(),
                            freed_label: format!("{:.1} MB", bytes as f64 / 1_048_576.0),
                        });
                    }
                } else if let Some(tr) = self.lib_tracks.get(self.list_index).cloned() {
                    self.queue = self.lib_tracks.clone();
                    self.queue_label = match self.lib_mode.as_str() {
                        "playlists" => self
                            .playlists
                            .get(self.playlist_index)
                            .map(|p| format!("PLAYLIST · {}", p.name).to_uppercase())
                            .unwrap_or_else(|| "PLAYLIST · QUEUE".into()),
                        "downloads" => "DOWNLOADS · QUEUE".into(),
                        "history" => "HISTORY · QUEUE".into(),
                        _ => "FAVORITES · QUEUE".into(),
                    };
                    self.start_playing_track(tr, sdl, false);
                }
            }
            HandheldBtn::X => {
                if let Some(tr) = self.lib_tracks.get(self.list_index).cloned() {
                    self.db.toggle_favorite(&tr);
                    self.refresh_lib_tracks();
                }
            }
            HandheldBtn::Y => {
                if let Some(tr) = self.lib_tracks.get(self.list_index).cloned() {
                    if self.lib_mode == "downloads" {
                        let freed = self.db.remove_offline(&tr.video_id);
                        self.refresh_lib_tracks();
                        self.set_toast(format!("Deleted {} ({:.1} MB)", tr.title, freed as f64 / 1_048_576.0));
                    } else {
                        self.cache_track_offline(tr);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_find_button<H: PlaybackHost>(&mut self, btn: HandheldBtn, sdl: &H) {
        match self.find_zone {
            0 => match btn {
                HandheldBtn::Left if !self.suggest_chips.is_empty() => {
                    let n = self.suggest_chips.len().min(4);
                    self.chip_index = (self.chip_index + n - 1) % n;
                }
                HandheldBtn::Right if !self.suggest_chips.is_empty() => {
                    let n = self.suggest_chips.len().min(4);
                    self.chip_index = (self.chip_index + 1) % n;
                }
                HandheldBtn::Down => self.find_zone = if self.find_hits.is_empty() { 2 } else { 1 },
                HandheldBtn::A => {
                    if let Some(c) = self.suggest_chips.get(self.chip_index).cloned() {
                        self.trigger_search(c);
                    }
                }
                HandheldBtn::B => self.find_zone = 2,
                _ => {}
            },
            1 => match btn {
                HandheldBtn::Up => {
                    if self.find_hit_index > 0 {
                        self.find_hit_index -= 1;
                        let len = self.find_hits.len();
                        Self::clamp_window(&mut self.find_hit_index, &mut self.find_hit_scroll, len, 6);
                    } else {
                        self.find_zone = 0;
                    }
                }
                HandheldBtn::Down => {
                    let n = self.find_hits.len();
                    if self.find_hit_index + 1 < n {
                        self.find_hit_index += 1;
                        Self::clamp_window(&mut self.find_hit_index, &mut self.find_hit_scroll, n, 6);
                    } else {
                        self.find_zone = 2;
                    }
                }
                HandheldBtn::RightUp if !self.find_hits.is_empty() => {
                    let n = self.find_hits.len();
                    self.find_hit_index = self.find_hit_index.saturating_sub(5);
                    Self::clamp_window(&mut self.find_hit_index, &mut self.find_hit_scroll, n, 6);
                }
                HandheldBtn::RightDown if !self.find_hits.is_empty() => {
                    let n = self.find_hits.len();
                    self.find_hit_index = (self.find_hit_index + 5).min(n - 1);
                    Self::clamp_window(&mut self.find_hit_index, &mut self.find_hit_scroll, n, 6);
                }
                HandheldBtn::A => match self.find_hits.get(self.find_hit_index).cloned() {
                    Some(FindHit::Artist(a)) => self.open_artist_detail(a, "find"),
                    Some(FindHit::Album(al)) => self.open_album_detail(al, "find"),
                    Some(FindHit::Song(tr)) => self.start_playing_track(tr, sdl, true),
                    None => {}
                },
                HandheldBtn::X => match self.find_hits.get_mut(self.find_hit_index) {
                    Some(FindHit::Song(tr)) => {
                        let fav = self.db.toggle_favorite(tr);
                        tr.is_favorite = fav;
                        self.set_toast(if fav { "Saved Favorite" } else { "Removed Favorite" });
                    }
                    Some(FindHit::Artist(a)) => {
                        let name = a.name.clone();
                        let on = self.db.toggle_fav_artist(&name);
                        if on {
                            self.fav_artists.insert(name);
                            self.set_toast("Favorited Artist");
                        } else {
                            self.fav_artists.remove(&name);
                            self.set_toast("Removed Artist Favorite");
                        }
                        self.rebuild_artists_list();
                    }
                    Some(FindHit::Album(_)) | None => {}
                },
                HandheldBtn::Y => {
                    if let Some(FindHit::Song(tr)) = self.find_hits.get(self.find_hit_index).cloned() {
                        self.cache_track_offline(tr);
                    }
                }
                HandheldBtn::B => self.find_zone = 2,
                _ => {}
            },
            _ => match btn {
                HandheldBtn::Left => self.key_col = (self.key_col + 9) % 10,
                HandheldBtn::Right => self.key_col = (self.key_col + 1) % 10,
                HandheldBtn::Up => {
                    if self.key_row > 0 {
                        self.key_row -= 1;
                    } else {
                        self.find_zone = if self.find_hits.is_empty() { 0 } else { 1 };
                    }
                }
                HandheldBtn::Down => self.key_row = (self.key_row + 1) % 3,
                HandheldBtn::R3 => self.kbd_num_mode = !self.kbd_num_mode,
                HandheldBtn::X => self.search_query.push(' '),
                HandheldBtn::Y => {
                    self.search_query.pop();
                    self.request_suggestions();
                }
                HandheldBtn::A => {
                    let key = if self.kbd_num_mode {
                        NUM_KEYS[self.key_row * 10 + self.key_col]
                    } else {
                        QWERTY_KEYS[self.key_row * 10 + self.key_col]
                    };
                    match key {
                        "123" => self.kbd_num_mode = true,
                        "ABC" => self.kbd_num_mode = false,
                        "SPC" => self.search_query.push(' '),
                        "DEL" => {
                            self.search_query.pop();
                            self.request_suggestions();
                        }
                        "CLR" => self.search_query.clear(),
                        "GO" => {
                            let q = self.search_query.clone();
                            self.trigger_search(q);
                        }
                        ch => {
                            self.search_query.push_str(&ch.to_lowercase());
                            self.request_suggestions();
                        }
                    }
                }
                HandheldBtn::B => self.active_tab = "now".into(),
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct MockHost {
        paused: Cell<bool>,
        clears: Cell<u32>,
        swap_ab: bool,
        should_quit: bool,
    }

    impl PlaybackHost for MockHost {
        fn clear_audio_queue(&self) {
            self.clears.set(self.clears.get() + 1);
        }
        fn set_audio_paused(&self, paused: bool) {
            self.paused.set(paused);
        }
        fn set_swap_ab(&mut self, swap: bool) {
            self.swap_ab = swap;
        }
        fn request_quit(&mut self) {
            self.should_quit = true;
        }
    }

    fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rusttune_input_{tag}_{nanos}"));
        let state = AppState::new_for_test(&dir);
        (state, dir)
    }

    #[test]
    fn pocket_lock_blocks_inputs_until_l3_or_start() {
        let (mut state, dir) = temp_state("pocket");
        let mut host = MockHost::default();
        state.active_tab = "now".into();

        state.handle_button(HandheldBtn::L3, &mut host);
        assert!(state.pocket_lock);

        state.handle_button(HandheldBtn::R1, &mut host);
        state.handle_button(HandheldBtn::Select, &mut host);
        assert_eq!(state.active_tab, "now");
        assert!(!state.settings_open);

        state.handle_button(HandheldBtn::Start, &mut host);
        assert!(!state.pocket_lock);

        state.handle_button(HandheldBtn::L3, &mut host);
        assert!(state.pocket_lock);
        state.handle_button(HandheldBtn::L3, &mut host);
        assert!(!state.pocket_lock);

        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dock_tabs_wrap_with_l1_and_r1() {
        let (mut state, dir) = temp_state("dock");
        let mut host = MockHost::default();
        state.active_tab = "now".into();

        let expected = ["vibes", "artists", "lib", "find", "now"];
        for tab in expected {
            state.handle_button(HandheldBtn::R1, &mut host);
            assert_eq!(state.active_tab, tab);
        }

        state.handle_button(HandheldBtn::L1, &mut host);
        assert_eq!(state.active_tab, "find");

        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_modal_toggles_and_quit() {
        let (mut state, dir) = temp_state("settings");
        let mut host = MockHost::default();

        state.handle_button(HandheldBtn::Select, &mut host);
        assert!(state.settings_open);
        assert_eq!(state.settings_index, 0);

        for _ in 0..3 {
            state.handle_button(HandheldBtn::Down, &mut host);
        }
        assert_eq!(state.settings_index, 3);
        state.handle_button(HandheldBtn::A, &mut host);
        assert!(state.cfg.swap_ab);
        assert!(host.swap_ab);

        for _ in 0..3 {
            state.handle_button(HandheldBtn::Down, &mut host);
        }
        assert_eq!(state.settings_index, 6);
        state.handle_button(HandheldBtn::A, &mut host);
        assert!(host.should_quit);

        state.handle_button(HandheldBtn::Select, &mut host);
        assert!(!state.settings_open);

        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn confirm_modal_accepts_and_cancels() {
        let (mut state, dir) = temp_state("confirm");
        let mut host = MockHost::default();
        let file = state.cache_dir.join("vid1.m4a");
        std::fs::write(&file, b"audio-bytes").unwrap();
        let track = crate::db::TrackMeta {
            video_id: "vid1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: String::new(),
            duration_secs: 60,
            thumb_url: String::new(),
            local_path: None,
            file_bytes: 0,
            is_favorite: false,
        };
        state.db.upsert_track(&track, false);
        state
            .db
            .set_offline(&track, Some(&file.to_string_lossy()), 11);

        state.confirm_action = Some(ConfirmAction::ClearAllDownloads {
            count: 1,
            freed_label: "11 B".into(),
        });
        state.handle_button(HandheldBtn::B, &mut host);
        assert!(state.confirm_action.is_none());
        assert_eq!(state.db.list_offline().len(), 1);

        state.confirm_action = Some(ConfirmAction::ClearAllDownloads {
            count: 1,
            freed_label: "11 B".into(),
        });
        state.handle_button(HandheldBtn::A, &mut host);
        assert!(state.confirm_action.is_none());
        assert_eq!(state.db.list_offline().len(), 0);
        assert!(!file.exists());

        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keyboard_r3_toggles_num_and_abc_modes() {
        let (mut state, dir) = temp_state("kbd");
        let mut host = MockHost::default();
        state.active_tab = "find".into();
        state.find_zone = 2;
        assert!(!state.kbd_num_mode);

        state.handle_button(HandheldBtn::R3, &mut host);
        assert!(state.kbd_num_mode);
        state.handle_button(HandheldBtn::R3, &mut host);
        assert!(!state.kbd_num_mode);

        // Selecting key (2, 8) switches "123" -> true, then "ABC" -> false.
        state.key_row = 2;
        state.key_col = 8;
        state.handle_button(HandheldBtn::A, &mut host);
        assert!(state.kbd_num_mode);
        state.handle_button(HandheldBtn::A, &mut host);
        assert!(!state.kbd_num_mode);

        // Inline grid filter keyboard also toggles cleanly on R3.
        state.active_tab = "artists".into();
        state.grid_kbd_open = true;
        state.handle_button(HandheldBtn::R3, &mut host);
        assert!(state.kbd_num_mode);

        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
}
