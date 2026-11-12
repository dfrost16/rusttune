use crate::audio::{download_to_file, start_local_playback};
use crate::db::TrackMeta;
use crate::innertube::{parse_youtube_input, Innertube, PastedLink, ResolvedAudio};
use crate::lyrics::fetch_lyrics;
use crate::media::extract_cover_accent;
use crate::sdl2_platform::PlaybackHost;
use crate::state::{AppState, ArtistEntry, BgMsg, FindHit};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn resolve_audio_with_fallback(it: &Innertube, video_id: &str) -> Result<ResolvedAudio, String> {
    it.resolve_audio_stream(video_id, false)
        .or_else(|_| it.resolve_audio_stream(video_id, true))
}

fn download_track_asset(
    it: &Innertube,
    video_id: &str,
    dest: &Path,
) -> Result<u64, String> {
    let res = resolve_audio_with_fallback(it, video_id)?;
    download_to_file(&res.url, &res.user_agent, res.content_length, dest)
}

impl AppState {
    pub fn start_playing_track<H: PlaybackHost>(
        &mut self,
        mut track: TrackMeta,
        host: &H,
        fetch_radio: bool,
    ) {
        if let Some(s) = self.session.take() {
            s.stop();
        }
        host.clear_audio_queue();
        host.set_audio_paused(false);

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
            thread::spawn(move || match resolve_audio_with_fallback(&it, &t_clone.video_id) {
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
                Err(e) => {
                    eprintln!("[innertube] stream resolve error: {e}");
                    let _ = tx.send(BgMsg::StreamError(e));
                }
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
        thread::spawn(move || match download_track_asset(&it, &track.video_id, &dest) {
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
                if let Ok(bytes) = download_track_asset(&it, &track.video_id, &dest) {
                    done += 1;
                    let _ = tx.send(BgMsg::OfflineCached {
                        track,
                        path: dest.to_string_lossy().into_owned(),
                        bytes,
                    });
                }
            }
            let _ = tx.send(BgMsg::Toast(format!("Finished downloading {done} tracks")));
        });
    }

    pub fn toggle_play_pause<H: PlaybackHost>(&mut self, host: &H) {
        if self.now_track.is_none() {
            if let Some(first) = self.queue.first().cloned() {
                self.start_playing_track(first, host, true);
            }
            return;
        }
        self.is_playing = !self.is_playing;
        host.set_audio_paused(!self.is_playing);
    }

    pub fn next_track<H: PlaybackHost>(&mut self, host: &H) {
        if self.queue.is_empty() {
            return;
        }
        self.queue_pos = (self.queue_pos + 1) % self.queue.len();
        let next = self.queue[self.queue_pos].clone();
        self.start_playing_track(next, host, false);
    }

    pub fn prev_track<H: PlaybackHost>(&mut self, host: &H) {
        if self.queue.is_empty() {
            return;
        }
        self.queue_pos = (self.queue_pos + self.queue.len() - 1) % self.queue.len();
        let prev = self.queue[self.queue_pos].clone();
        self.start_playing_track(prev, host, false);
    }
}
