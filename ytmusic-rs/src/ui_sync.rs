use crate::media::{color_from_rgb, fmt_time, local_clock_digits, parse_battery_pct, theme_accent_pair};
use crate::state::{AppState, ConfirmAction, FindHit};
use crate::{GridCard, PlaylistRow, TrackRow, YtMusicWindow};
use slint::{Color, Image, ModelRc, SharedString, VecModel};
use std::rc::Rc;

impl AppState {
    pub fn sync_ui(&mut self, win: &YtMusicWindow) {
        let light_mode = self.cfg.theme == "light";
        let now_id = self.now_track.as_ref().map(|t| t.video_id.clone());
        let dyn_accent = now_id
            .as_ref()
            .and_then(|id| self.cover_colors.get(id).copied())
            .map(|rgb| theme_accent_pair(rgb, light_mode));

        if light_mode {
            win.set_c_bg(Color::from_rgb_u8(243, 244, 246));
            win.set_c_card(Color::from_rgb_u8(255, 255, 255));
            win.set_c_card2(Color::from_rgb_u8(226, 232, 240));
            win.set_c_dock(Color::from_rgb_u8(255, 255, 255));
            win.set_c_border(Color::from_rgb_u8(203, 213, 225));
            win.set_c_text(Color::from_rgb_u8(15, 23, 42));
            win.set_c_muted(Color::from_rgb_u8(51, 65, 85));
            let (acc, fg) = dyn_accent.unwrap_or(((29, 78, 216), (255, 255, 255)));
            win.set_c_accent(color_from_rgb(acc));
            win.set_c_accent_fg(color_from_rgb(fg));
            win.set_c_heart(Color::from_rgb_u8(190, 18, 60));
            win.set_c_ok(Color::from_rgb_u8(21, 128, 61));
            win.set_bg_dots(self.dots_light.clone());
            win.set_theme_label("Studio Light".into());
        } else {
            win.set_c_bg(Color::from_rgb_u8(14, 16, 20));
            win.set_c_card(Color::from_rgb_u8(22, 25, 34));
            win.set_c_card2(Color::from_rgb_u8(31, 36, 48));
            win.set_c_dock(Color::from_rgb_u8(17, 19, 26));
            win.set_c_border(Color::from_rgb_u8(42, 48, 63));
            win.set_c_text(Color::from_rgb_u8(243, 244, 246));
            win.set_c_muted(Color::from_rgb_u8(156, 163, 175));
            let (acc, fg) = dyn_accent.unwrap_or(((190, 242, 100), (15, 23, 42)));
            win.set_c_accent(color_from_rgb(acc));
            win.set_c_accent_fg(color_from_rgb(fg));
            win.set_c_heart(Color::from_rgb_u8(251, 113, 133));
            win.set_c_ok(Color::from_rgb_u8(74, 222, 128));
            win.set_bg_dots(self.dots_dark.clone());
            win.set_theme_label("Studio Dark".into());
        }

        win.set_active_tab(self.active_tab.clone().into());
        let dock_tab = match self.active_tab.as_str() {
            "vibe_detail" => "vibes",
            "artist_detail" | "album_detail" => {
                if self.detail_origin == "find" {
                    "find"
                } else if self.detail_origin == "now" {
                    "now"
                } else {
                    "artists"
                }
            }
            other => other,
        };
        win.set_dock_tab(dock_tab.into());

        self.sync_now(win, now_id.as_deref());
        self.sync_grids(win);
        self.sync_details_and_lib(win);
        self.sync_find(win, now_id.as_deref());

        win.set_light_mode(light_mode);
        win.set_battery_label(self.battery_label.clone().into());
        win.set_battery_pct(parse_battery_pct(&self.battery_label));
        let [h1, h2, m1, m2] = local_clock_digits();
        win.set_clock_h1(h1.into());
        win.set_clock_h2(h2.into());
        win.set_clock_m1(m1.into());
        win.set_clock_m2(m2.into());
        win.set_remote_url(
            self.remote
                .as_ref()
                .map(|r| r.url.clone())
                .unwrap_or_else(|| "Server Off".into())
                .into(),
        );
        win.set_web_server_on(self.remote.is_some());
        win.set_web_pin_label(
            if !self.cfg.web_pin_enabled {
                "OFF".to_string()
            } else if let Some(ref srv) = self.remote {
                srv.pin()
            } else {
                "ON".to_string()
            }
            .into(),
        );
        win.set_pocket_lock(self.pocket_lock);
        win.set_pl_modal_open(self.pl_modal_open);
        win.set_pl_modal_index(self.pl_modal_index as i32);
        win.set_settings_open(self.settings_open);
        win.set_settings_index(self.settings_index as i32);
        win.set_loop_track(self.loop_track);

        if let Some(ref c) = self.confirm_action {
            win.set_confirm_open(true);
            match c {
                ConfirmAction::DownloadBatch { title, tracks } => {
                    let pend = tracks
                        .iter()
                        .filter(|t| self.db.get_local_path(&t.video_id).is_none())
                        .count();
                    win.set_confirm_title("CONFIRM BATCH DOWNLOAD".into());
                    win.set_confirm_body(
                        format!("Download {pend} tracks from \"{title}\"? (~{} MB)", pend * 3).into(),
                    );
                    win.set_confirm_yes_label("Start Download".into());
                }
                ConfirmAction::ClearAllDownloads { count, freed_label } => {
                    win.set_confirm_title("DELETE ALL OFFLINE FILES?".into());
                    win.set_confirm_body(
                        format!("Remove all {count} cached .m4a tracks from your SD card ({freed_label})?")
                            .into(),
                    );
                    win.set_confirm_yes_label("Delete All".into());
                }
            }
        } else {
            win.set_confirm_open(false);
        }

        win.set_radio_autoplay(self.cfg.radio_autoplay);
        win.set_swap_ab(self.cfg.swap_ab);
        win.set_toast_text(
            self.toast.as_ref().map(|(s, _)| s.clone()).unwrap_or_default().into(),
        );
    }

    fn sync_now(&mut self, win: &YtMusicWindow, now_id: Option<&str>) {
        if let Some(ref t) = self.now_track {
            win.set_now_title(t.title.clone().into());
            win.set_now_artist(t.artist.clone().into());
            win.set_now_album(t.album.clone().into());
            win.set_is_favorite_now(self.db.is_favorite(&t.video_id));
            win.set_is_offline_now(
                t.local_path.is_some() || self.db.get_local_path(&t.video_id).is_some(),
            );
        } else {
            win.set_now_title("Ready to Play".into());
            win.set_now_artist("Pick a Vibe, Artist, or Search".into());
            win.set_now_album("".into());
            win.set_is_favorite_now(false);
            win.set_is_offline_now(false);
        }
        win.set_now_genre(self.now_genre.clone().into());
        win.set_now_artist_focused(self.now_artist_focused);

        if let Some(id) = now_id {
            if let Some(img) = self.cover_cache.get(id) {
                if self.active_cover_id.as_deref() != Some(id) {
                    win.set_cover_art(img.clone());
                    self.active_cover_id = Some(id.to_string());
                }
                win.set_has_cover_art(true);
            } else {
                win.set_has_cover_art(false);
            }

            if let Some(opt_ly) = self.lyrics_cache.get(id) {
                if let Some(ly) = opt_ly {
                    let cur_idx = if ly.synced {
                        ly.lines.iter().rposition(|l| self.elapsed_ms >= l.time_ms).unwrap_or(0)
                    } else {
                        ((self.elapsed_ms / 4500) as usize).min(ly.lines.len().saturating_sub(1))
                    };
                    let prev = cur_idx
                        .checked_sub(1)
                        .and_then(|i| ly.lines.get(i))
                        .map(|l| l.text.as_str())
                        .unwrap_or("");
                    let curr = ly.lines.get(cur_idx).map(|l| l.text.as_str()).unwrap_or("");
                    let next = ly.lines.get(cur_idx + 1).map(|l| l.text.as_str()).unwrap_or("");
                    win.set_lyric_prev(prev.into());
                    win.set_lyric_curr(curr.into());
                    win.set_lyric_next(next.into());
                } else {
                    win.set_lyric_prev("".into());
                    win.set_lyric_curr("No lyrics available".into());
                    win.set_lyric_next("".into());
                }
            } else {
                win.set_lyric_prev("".into());
                win.set_lyric_curr("Loading lyrics...".into());
                win.set_lyric_next("".into());
            }
        } else {
            win.set_has_cover_art(false);
            win.set_lyric_prev("".into());
            win.set_lyric_curr("Select a track to view lyrics".into());
            win.set_lyric_next("".into());
        }

        let elapsed_s = self.elapsed_ms / 1000;
        win.set_now_elapsed(fmt_time(elapsed_s).into());
        win.set_now_duration(fmt_time(self.duration_secs).into());
        win.set_now_format(self.now_format.clone().into());
        let prog = if self.duration_secs > 0 {
            (elapsed_s as f32 / self.duration_secs as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        win.set_now_progress(prog);
        win.set_is_playing(self.is_playing);

        win.set_playlist_label(self.queue_label.clone().into());
        if self.queue.len() > 1 {
            let next_idx = (self.queue_pos + 1) % self.queue.len();
            let nxt = &self.queue[next_idx];
            win.set_next_title(nxt.title.clone().into());
            win.set_next_artist(nxt.artist.clone().into());
            win.set_next_duration(
                if nxt.duration_secs > 0 {
                    fmt_time(nxt.duration_secs)
                } else {
                    String::new()
                }
                .into(),
            );

            let mut upcoming = Vec::new();
            for offset in 2..=6 {
                if offset < self.queue.len() {
                    let q_i = (self.queue_pos + offset) % self.queue.len();
                    upcoming.push(self.queue[q_i].clone());
                }
            }
            win.set_now_playlist_rows(ModelRc::from(Rc::new(VecModel::from(
                self.to_track_rows(&upcoming),
            ))));
        } else {
            win.set_next_title(if self.cfg.radio_autoplay {
                "Loading Radio Up Next...".into()
            } else {
                "End of Queue".into()
            });
            win.set_next_artist("".into());
            win.set_next_duration("".into());
            win.set_now_playlist_rows(ModelRc::from(Rc::new(VecModel::from(Vec::<TrackRow>::new()))));
        }
    }

    fn sync_grids(&self, win: &YtMusicWindow) {
        let vibes = self.filtered_vibes();
        let vibe_cards: Vec<GridCard> = vibes
            .iter()
            .map(|v| GridCard {
                title: SharedString::from(&v.title),
                subtitle: SharedString::from(if v.browse_id.is_empty() {
                    "Featured Mix"
                } else {
                    "YT Music Mood"
                }),
                count_label: SharedString::from("Vibe"),
                initial: SharedString::from(
                    v.title.chars().next().unwrap_or('V').to_uppercase().to_string(),
                ),
                accent: color_from_rgb(v.rgb),
                is_fav: self.fav_vibes.contains(&v.id),
                thumb: Image::default(),
                has_thumb: false,
            })
            .collect();
        win.set_vibe_cards(ModelRc::from(Rc::new(VecModel::from(vibe_cards))));
        win.set_vibe_index(self.vibe_index as i32);
        win.set_vibe_row_scroll(self.vibe_row_scroll as i32);
        win.set_vibe_fav_only(self.vibe_fav_only);
        win.set_vibe_filter(self.vibe_filter.clone().into());

        let artists = self.filtered_artists();
        let total_pages = artists.len().div_ceil(6).max(1);
        let cur_page = (self.artist_row_scroll / 2) + 1;
        win.set_artist_page_label(
            format!("Page {cur_page}/{total_pages} ({} artists · R-Stick)", artists.len()).into(),
        );
        let artist_cards: Vec<GridCard> = artists
            .iter()
            .map(|a| {
                let thumb = self.thumb_cache.get(&a.thumb_url).cloned();
                GridCard {
                    title: SharedString::from(&a.name),
                    subtitle: SharedString::from(&a.subtitle),
                    count_label: SharedString::from(if a.browse_id.is_empty() { "Pick" } else { "YT Music" }),
                    initial: SharedString::from(
                        a.name.chars().next().unwrap_or('A').to_uppercase().to_string(),
                    ),
                    accent: color_from_rgb(a.rgb),
                    is_fav: self.fav_artists.contains(&a.name),
                    has_thumb: thumb.is_some(),
                    thumb: thumb.unwrap_or_default(),
                }
            })
            .collect();
        win.set_artist_cards(ModelRc::from(Rc::new(VecModel::from(artist_cards))));
        win.set_artist_index(self.artist_index as i32);
        win.set_artist_row_scroll(self.artist_row_scroll as i32);
        win.set_artist_fav_only(self.artist_fav_only);
        win.set_artist_filter(self.artist_filter.clone().into());
        win.set_grid_kbd_open(self.grid_kbd_open);
        win.set_kbd_num_mode(self.kbd_num_mode);
        win.set_grid_kbd_title(
            match self.active_tab.as_str() {
                "vibes" => "FILTER VIBES (R3)",
                "lib" => "FILTER LIBRARY (R3)",
                _ => "SEARCH ARTISTS (R3)",
            }
            .into(),
        );
        let preview_cards: Vec<GridCard> = if self.active_tab == "artists" {
            artists
                .iter()
                .take(3)
                .map(|a| {
                    let thumb = self.thumb_cache.get(&a.thumb_url).cloned();
                    GridCard {
                        title: SharedString::from(&a.name),
                        subtitle: SharedString::new(),
                        count_label: SharedString::new(),
                        initial: SharedString::from(
                            a.name.chars().next().unwrap_or('A').to_uppercase().to_string(),
                        ),
                        accent: color_from_rgb(a.rgb),
                        is_fav: self.fav_artists.contains(&a.name),
                        has_thumb: thumb.is_some(),
                        thumb: thumb.unwrap_or_default(),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        win.set_grid_kbd_previews(ModelRc::from(Rc::new(VecModel::from(preview_cards))));
    }

    fn sync_details_and_lib(&self, win: &YtMusicWindow) {
        win.set_detail_top_focused(self.detail_top_focused);
        win.set_filter_focused(self.filter_focused);
        if self.active_tab == "artist_detail" {
            win.set_detail_title(self.artist_title.clone().into());
            win.set_artist_kicker(self.artist_kicker.clone().into());
            win.set_artist_stats(self.artist_stats.clone().into());
            win.set_detail_is_fav(self.fav_artists.contains(&self.artist_title));
            win.set_detail_accent(color_from_rgb(self.detail_accent));
            if let Some(img) = self.circle_thumb_cache.get(&self.artist_thumb_url) {
                win.set_detail_thumb(img.clone());
                win.set_has_detail_thumb(true);
            } else {
                win.set_has_detail_thumb(false);
            }
            win.set_artist_section(self.artist_section as i32);
            win.set_artist_songs_count(self.artist_songs.len() as i32);
            win.set_artist_albums_count(self.artist_albums.len() as i32);
            win.set_artist_singles_count(self.artist_singles.len() as i32);

            let active_rows: Vec<TrackRow> = match self.artist_section {
                0 => self.to_track_rows(&self.artist_songs),
                1 | 2 => {
                    let src = if self.artist_section == 1 {
                        &self.artist_albums
                    } else {
                        &self.artist_singles
                    };
                    src.iter()
                        .map(|al| {
                            let th = self.thumb_cache.get(&al.thumb_url).cloned();
                            TrackRow {
                                title: SharedString::from(&al.title),
                                artist: SharedString::from(&al.subtitle),
                                album: SharedString::new(),
                                duration: SharedString::from("Open ↗"),
                                is_offline: false,
                                is_favorite: false,
                                is_playing: false,
                                is_artist: true,
                                kind: SharedString::from("ALBUM"),
                                has_thumb: th.is_some(),
                                thumb: th.unwrap_or_default(),
                            }
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            let active_len = active_rows.len();
            win.set_detail_count_label(
                if active_len > 0 {
                    format!("{} / {}", self.list_index + 1, active_len)
                } else {
                    String::from("0")
                }
                .into(),
            );
            win.set_list_rows(ModelRc::from(Rc::new(VecModel::from(active_rows))));
        } else {
            win.set_detail_title(self.detail_title.clone().into());
            win.set_detail_subtitle(self.detail_subtitle.clone().into());
            win.set_detail_accent(color_from_rgb(self.detail_accent));
            if let Some(img) = self.thumb_cache.get(&self.detail_thumb_url) {
                win.set_detail_thumb(img.clone());
                win.set_has_detail_thumb(true);
            } else {
                win.set_has_detail_thumb(false);
            }
            let active_rows = self.to_track_rows(self.active_list_tracks());
            let active_len = active_rows.len();
            win.set_detail_count_label(
                if active_len > 0 {
                    format!("{} / {}", self.list_index + 1, active_len)
                } else {
                    String::new()
                }
                .into(),
            );
            win.set_list_rows(ModelRc::from(Rc::new(VecModel::from(active_rows))));
        }
        win.set_list_index(self.list_index as i32);
        win.set_list_scroll(self.list_scroll as i32);

        win.set_lib_mode(self.lib_mode.clone().into());
        win.set_lib_filter(self.lib_filter.clone().into());
        let dl_bytes: u64 = if self.lib_mode == "downloads" {
            self.lib_tracks.iter().map(|t| t.file_bytes).sum()
        } else {
            0
        };
        win.set_lib_downloads_info(
            if self.lib_mode == "downloads" && !self.lib_tracks.is_empty() {
                format!("Clear ({:.1} MB)", dl_bytes as f64 / 1_048_576.0)
            } else {
                String::new()
            }
            .into(),
        );
        let target_pl_ids: Vec<i64> = self
            .pl_target_track
            .as_ref()
            .map(|t| self.db.track_playlist_ids(&t.video_id))
            .unwrap_or_default();
        let pl_rows: Vec<PlaylistRow> = self
            .playlists
            .iter()
            .map(|p| PlaylistRow {
                name: SharedString::from(&p.name),
                count_label: SharedString::from(format!("{}", p.track_count)),
                contains_target: target_pl_ids.contains(&p.id),
            })
            .collect();
        win.set_playlists(ModelRc::from(Rc::new(VecModel::from(pl_rows))));
        win.set_playlist_index(self.playlist_index as i32);
        win.set_lib_focus_playlists(self.lib_focus_playlists);
    }

    fn sync_find(&self, win: &YtMusicWindow, now_id: Option<&str>) {
        win.set_search_query(self.search_query.clone().into());
        win.set_find_zone(self.find_zone as i32);
        win.set_chip_index(self.chip_index as i32);
        let chip_models: Vec<SharedString> = self.suggest_chips.iter().map(SharedString::from).collect();
        win.set_suggest_chips(ModelRc::from(Rc::new(VecModel::from(chip_models))));
        let find_rows: Vec<TrackRow> = self
            .find_hits
            .iter()
            .map(|h| match h {
                FindHit::Artist(a) => TrackRow {
                    title: SharedString::from(&a.name),
                    artist: SharedString::from(&a.subtitle),
                    album: SharedString::new(),
                    duration: SharedString::from("ARTIST"),
                    is_offline: false,
                    is_favorite: self.fav_artists.contains(&a.name),
                    is_playing: false,
                    is_artist: true,
                    kind: SharedString::from("ARTIST"),
                    thumb: Image::default(),
                    has_thumb: false,
                },
                FindHit::Album(al) => TrackRow {
                    title: SharedString::from(&al.title),
                    artist: SharedString::from(&al.subtitle),
                    album: SharedString::new(),
                    duration: SharedString::from("ALBUM"),
                    is_offline: false,
                    is_favorite: false,
                    is_playing: false,
                    is_artist: true,
                    kind: SharedString::from("ALBUM"),
                    thumb: Image::default(),
                    has_thumb: false,
                },
                FindHit::Song(t) => TrackRow {
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
                },
            })
            .collect();
        win.set_find_hits(ModelRc::from(Rc::new(VecModel::from(find_rows))));
        win.set_find_hit_index(self.find_hit_index as i32);
        win.set_find_hit_scroll(self.find_hit_scroll as i32);
        win.set_key_row(self.key_row as i32);
        win.set_key_col(self.key_col as i32);
    }
}
