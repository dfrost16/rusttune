use crate::db::{MusicDb, TrackMeta};
use crate::innertube::{extract_playlist_id, parse_youtube_input, Innertube, PastedLink};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

include!(concat!(env!("OUT_DIR"), "/webapp_assets.rs"));

#[derive(Debug, Clone)]
pub enum RemoteCmd {
    PlayInput(String),
    PlayTrack(TrackMeta),
    PlayPlaylist(Vec<TrackMeta>, String),
    BatchDownload(Vec<TrackMeta>),
    QueueDownload(String),
    RefreshLibrary,
    TogglePlay,
    Next,
    Prev,
    ToggleLoop,
    ToggleTheme,
    ShutdownServer,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RemoteSnapshot {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub loop_track: bool,
    pub elapsed_secs: u64,
    pub duration_secs: u64,
    pub elapsed_str: String,
    pub duration_str: String,
    pub theme: String,
    pub battery: String,
    pub offline_count: usize,
    pub offline_bytes: u64,
    pub orphan_count: usize,
    pub orphan_bytes: u64,
    pub fav_count: usize,
    pub server_url: String,
    pub queue: Vec<String>,
}

struct AuthState {
    pin_enabled: bool,
    pin: String,
    tokens: HashSet<String>,
    fail_count: u32,
    locked_until: Option<Instant>,
}

struct ParsedReq {
    method: String,
    raw_url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Serialize)]
struct PlaylistDto {
    id: i64,
    name: String,
    track_count: usize,
    offline_bytes: u64,
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

#[derive(Deserialize)]
struct PinBody {
    pin: String,
}

#[derive(Deserialize)]
struct ImportPreviewBody {
    url: String,
}

#[derive(Deserialize)]
struct ImportCommitBody {
    name: String,
    tracks: Vec<TrackMeta>,
    #[serde(default)]
    download: bool,
}

#[derive(Deserialize)]
struct BulkDeleteBody {
    video_ids: Vec<String>,
}

#[derive(Deserialize)]
struct PlayTrackBody {
    track: TrackMeta,
}

pub fn get_local_ip() -> String {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("10.255.255.255:1").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                let ip = addr.ip().to_string();
                if !ip.starts_with("127.") {
                    return ip;
                }
            }
        }
    }
    "127.0.0.1".to_string()
}

fn generate_random_6_digit_pin() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(123456);
    let pid = std::process::id() as u128;
    let mixed = nanos.wrapping_mul(6364136223846793005).wrapping_add(pid ^ 0x9e3779b9);
    let code = (mixed >> 16) % 1_000_000;
    format!("{code:06}")
}

fn generate_session_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(987654321);
    let a = nanos.wrapping_mul(6364136223846793005).wrapping_add(0x517cc1b727220a95);
    let b = a.wrapping_mul(2862933555777941757).wrapping_add(std::process::id() as u128);
    format!("{:016x}{:016x}", a as u64, b as u64)
}

struct ConnGuard(Arc<AtomicUsize>);
impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct WebRemote {
    pub url: String,
    pub snapshot: Arc<Mutex<RemoteSnapshot>>,
    pub rx: mpsc::Receiver<RemoteCmd>,
    stop_flag: Arc<AtomicBool>,
    auth: Arc<Mutex<AuthState>>,
}

impl WebRemote {
    pub fn start(
        port: u16,
        db_path: PathBuf,
        cache_dir: PathBuf,
        innertube: Arc<Innertube>,
        pin_enabled: bool,
    ) -> Option<Self> {
        let addr = format!("0.0.0.0:{port}");
        let listener = TcpListener::bind(&addr).ok()?;
        let _ = listener.set_nonblocking(true);

        let ip = get_local_ip();
        let snapshot = Arc::new(Mutex::new(RemoteSnapshot::default()));
        let (tx, rx) = mpsc::sync_channel(64);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let active_conns = Arc::new(AtomicUsize::new(0));
        let auth = Arc::new(Mutex::new(AuthState {
            pin_enabled,
            pin: generate_random_6_digit_pin(),
            tokens: HashSet::new(),
            fail_count: 0,
            locked_until: None,
        }));

        let db = Arc::new(Mutex::new(MusicDb::open(&db_path, &cache_dir).ok()?));

        let stop_ref = stop_flag.clone();
        let snap_ref = snapshot.clone();
        let auth_ref = auth.clone();
        let tx_ref = tx.clone();

        thread::spawn(move || {
            const MAX_CONNS: usize = 32;
            while !stop_ref.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if active_conns.load(Ordering::Relaxed) >= MAX_CONNS {
                            continue;
                        }
                        active_conns.fetch_add(1, Ordering::Relaxed);
                        let guard_ctr = active_conns.clone();
                        let db_c = db.clone();
                        let cache_c = cache_dir.clone();
                        let it_c = innertube.clone();
                        let snap_c = snap_ref.clone();
                        let auth_c = auth_ref.clone();
                        let tx_c = tx_ref.clone();

                        thread::spawn(move || {
                            let _guard = ConnGuard(guard_ctr);
                            handle_connection(stream, &db_c, &cache_c, &it_c, &snap_c, &auth_c, &tx_c);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(45));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });

        Some(Self {
            url: format!("http://{ip}:{port}"),
            snapshot,
            rx,
            stop_flag,
            auth,
        })
    }

    pub fn pin(&self) -> String {
        self.auth.lock().unwrap().pin.clone()
    }

    pub fn pin_enabled(&self) -> bool {
        self.auth.lock().unwrap().pin_enabled
    }

    pub fn set_pin_enabled(&self, enabled: bool) {
        let mut a = self.auth.lock().unwrap();
        a.pin_enabled = enabled;
        a.tokens.clear();
        if enabled {
            a.pin = generate_random_6_digit_pin();
        }
    }

    pub fn rotate_pin(&self) -> String {
        let mut a = self.auth.lock().unwrap();
        a.pin = generate_random_6_digit_pin();
        a.tokens.clear();
        a.fail_count = 0;
        a.locked_until = None;
        a.pin.clone()
    }

    pub fn shutdown(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

impl Drop for WebRemote {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn handle_connection(
    mut stream: TcpStream,
    db: &Arc<Mutex<MusicDb>>,
    cache_dir: &Path,
    innertube: &Arc<Innertube>,
    snapshot: &Arc<Mutex<RemoteSnapshot>>,
    auth: &Arc<Mutex<AuthState>>,
    tx: &mpsc::SyncSender<RemoteCmd>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(12)));

    // Drop TLS ClientHello (0x16) immediately so browsers upgrading to https:// fall back cleanly to http://
    let mut peek = [0u8; 1];
    if let Ok(1) = stream.peek(&mut peek) {
        if peek[0] == 0x16 {
            return;
        }
    }

    let Some(req) = read_http_request(&mut stream) else {
        return;
    };

    let path = req.raw_url.split('?').next().unwrap_or("/");
    if path.starts_with("/api/") {
        handle_api(stream, &req, path, db, cache_dir, innertube, snapshot, auth, tx);
        return;
    }

    // Serve static Svelte 5 + Vite PWA assets cleanly without requiring auth
    let asset_path = if path == "/" { "/index.html" } else { path };
    if let Some((mime, bytes)) = get_webapp_asset(asset_path).or_else(|| get_webapp_asset("/index.html")) {
        let cache_hdr = if asset_path.starts_with("/assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        send_response(stream, 200, "OK", mime, bytes, &[("Cache-Control", cache_hdr)]);
        return;
    }

    send_text(stream, 404, "text/plain; charset=utf-8", "Web UI assets not built");
}

fn read_http_request(stream: &mut TcpStream) -> Option<ParsedReq> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).ok()? == 0 {
        return None;
    }
    let mut parts = first_line.split_whitespace();
    let method = parts.next()?.to_uppercase();
    let raw_url = parts.next()?.to_string();

    let mut headers = HashMap::new();
    let mut header_count = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        header_count += 1;
        if header_count > 64 {
            return None;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_len = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(4 * 1024 * 1024);

    let mut body = vec![0u8; content_len];
    if content_len > 0 {
        reader.read_exact(&mut body).ok()?;
    }

    Some(ParsedReq {
        method,
        raw_url,
        headers,
        body,
    })
}

fn extract_token(req: &ParsedReq) -> Option<String> {
    if let Some(auth_hdr) = req.headers.get("authorization") {
        if let Some(tok) = auth_hdr.strip_prefix("Bearer ").or_else(|| auth_hdr.strip_prefix("bearer ")) {
            let t = tok.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    if let Some(cookie) = req.headers.get("cookie") {
        for part in cookie.split(';') {
            if let Some((k, v)) = part.trim().split_once('=') {
                if k == "ytm_session" && !v.is_empty() {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

fn is_authorized(req: &ParsedReq, auth: &Arc<Mutex<AuthState>>) -> bool {
    let a = auth.lock().unwrap();
    if !a.pin_enabled {
        return true;
    }
    extract_token(req).is_some_and(|t| a.tokens.contains(&t))
}

#[allow(clippy::too_many_arguments)]
fn handle_api(
    stream: TcpStream,
    req: &ParsedReq,
    path: &str,
    db: &Arc<Mutex<MusicDb>>,
    cache_dir: &Path,
    innertube: &Arc<Innertube>,
    snapshot: &Arc<Mutex<RemoteSnapshot>>,
    auth: &Arc<Mutex<AuthState>>,
    tx: &mpsc::SyncSender<RemoteCmd>,
) {
    let method = req.method.as_str();
    let raw_url = req.raw_url.as_str();

    // ── Public Auth Routes ───────────────────────────────────────────────
    if method == "GET" && path == "/api/auth/status" {
        let a = auth.lock().unwrap();
        let authed = !a.pin_enabled || extract_token(req).is_some_and(|t| a.tokens.contains(&t));
        let json = serde_json::json!({
            "pin_required": a.pin_enabled,
            "authenticated": authed,
        })
        .to_string();
        send_json(stream, 200, &json, &[]);
        return;
    }

    if method == "POST" && path == "/api/auth/verify" {
        let Ok(body) = serde_json::from_slice::<PinBody>(&req.body) else {
            send_err(stream, 400, "Invalid PIN payload");
            return;
        };
        let mut a = auth.lock().unwrap();
        if let Some(until) = a.locked_until {
            if Instant::now() < until {
                send_err(stream, 429, "Too many wrong PIN attempts. Wait 30 seconds.");
                return;
            }
            a.locked_until = None;
            a.fail_count = 0;
        }
        if !a.pin_enabled || body.pin.trim() == a.pin {
            a.fail_count = 0;
            let token = generate_session_token();
            a.tokens.insert(token.clone());
            let cookie = format!("ytm_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000");
            let json = serde_json::json!({ "ok": true, "token": token }).to_string();
            send_json(stream, 200, &json, &[("Set-Cookie", &cookie)]);
        } else {
            a.fail_count += 1;
            if a.fail_count >= 5 {
                a.locked_until = Some(Instant::now() + Duration::from_secs(30));
            }
            send_err(stream, 401, "Incorrect 6-digit PIN");
        }
        return;
    }

    if !is_authorized(req, auth) {
        send_json(stream, 401, r#"{"error":"PIN required","pin_required":true}"#, &[]);
        return;
    }

    // ── Status & Transport ───────────────────────────────────────────────
    if method == "GET" && path == "/api/status" {
        let body = serde_json::to_string(&*snapshot.lock().unwrap()).unwrap_or_else(|_| "{}".into());
        send_json(stream, 200, &body, &[]);
        return;
    }

    if method == "POST" && path == "/api/cmd" {
        if let Some(action) = query_param(raw_url, "action") {
            let cmd = match action.as_str() {
                "toggle" => Some(RemoteCmd::TogglePlay),
                "next" => Some(RemoteCmd::Next),
                "prev" => Some(RemoteCmd::Prev),
                "loop" => Some(RemoteCmd::ToggleLoop),
                "theme" => Some(RemoteCmd::ToggleTheme),
                "server_off" => Some(RemoteCmd::ShutdownServer),
                _ => None,
            };
            if let Some(c) = cmd {
                let _ = tx.try_send(c);
            }
        }
        send_json(stream, 200, r#"{"ok":true}"#, &[]);
        return;
    }

    if method == "POST" && path == "/api/search" {
        if let Some(q) = query_param(raw_url, "q") {
            let _ = tx.try_send(RemoteCmd::PlayInput(q));
        }
        send_json(stream, 200, r#"{"ok":true}"#, &[]);
        return;
    }

    if method == "POST" && path == "/api/play" {
        if let Ok(body) = serde_json::from_slice::<PlayTrackBody>(&req.body) {
            let _ = tx.try_send(RemoteCmd::PlayTrack(body.track));
            send_json(stream, 200, r#"{"ok":true}"#, &[]);
        } else {
            send_err(stream, 400, "Invalid track JSON");
        }
        return;
    }

    // ── Playlists CRUD ───────────────────────────────────────────────────
    if method == "GET" && path == "/api/playlists" {
        let list: Vec<PlaylistDto> = {
            let d = db.lock().unwrap();
            d.list_playlists()
                .into_iter()
                .map(|p| PlaylistDto {
                    offline_bytes: d.playlist_offline_bytes(p.id),
                    id: p.id,
                    name: p.name,
                    track_count: p.track_count,
                })
                .collect()
        };
        send_json(stream, 200, &serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()), &[]);
        return;
    }

    if method == "POST" && path == "/api/playlists" {
        if let Ok(body) = serde_json::from_slice::<NameBody>(&req.body) {
            let clean = body.name.trim();
            if clean.is_empty() {
                send_err(stream, 400, "Name required");
                return;
            }
            let id_opt = db.lock().unwrap().create_playlist(clean);
            if let Some(id) = id_opt {
                let _ = tx.try_send(RemoteCmd::RefreshLibrary);
                send_json(stream, 200, &format!(r#"{{"id":{id}}}"#), &[]);
            } else {
                send_err(stream, 500, "Could not create playlist");
            }
        } else {
            send_err(stream, 400, "Invalid JSON");
        }
        return;
    }

    if let Some(rest) = path.strip_prefix("/api/playlists/") {
        let parts: Vec<&str> = rest.split('/').collect();
        let Ok(pl_id) = parts[0].parse::<i64>() else {
            send_err(stream, 400, "Bad playlist id");
            return;
        };

        if parts.len() == 1 {
            match method {
                "PATCH" => {
                    if let Ok(body) = serde_json::from_slice::<NameBody>(&req.body) {
                        match db.lock().unwrap().rename_playlist(pl_id, &body.name) {
                            Ok(()) => {
                                let _ = tx.try_send(RemoteCmd::RefreshLibrary);
                                send_json(stream, 200, r#"{"ok":true}"#, &[]);
                            }
                            Err(e) => send_err(stream, 400, &e),
                        }
                    } else {
                        send_err(stream, 400, "Invalid JSON");
                    }
                    return;
                }
                "DELETE" => {
                    let del_files = query_param(raw_url, "delete_files").as_deref() == Some("true");
                    let (removed, freed_bytes) = db.lock().unwrap().delete_playlist(pl_id, del_files);
                    let _ = tx.try_send(RemoteCmd::RefreshLibrary);
                    send_json(
                        stream,
                        200,
                        &format!(r#"{{"removed":{removed},"freed_bytes":{freed_bytes}}}"#),
                        &[],
                    );
                    return;
                }
                _ => {}
            }
        } else if parts.len() == 2 && parts[1] == "tracks" && method == "GET" {
            let tracks = db.lock().unwrap().playlist_tracks(pl_id);
            send_json(stream, 200, &serde_json::to_string(&tracks).unwrap_or_else(|_| "[]".into()), &[]);
            return;
        } else if parts.len() == 2 && parts[1] == "play" && method == "POST" {
            let (tracks, name) = {
                let d = db.lock().unwrap();
                let tracks = d.playlist_tracks(pl_id);
                let name = d
                    .list_playlists()
                    .into_iter()
                    .find(|p| p.id == pl_id)
                    .map(|p| p.name)
                    .unwrap_or_else(|| "PLAYLIST".into());
                (tracks, name)
            };
            if !tracks.is_empty() {
                let _ = tx.try_send(RemoteCmd::PlayPlaylist(tracks, name));
            }
            send_json(stream, 200, r#"{"ok":true}"#, &[]);
            return;
        } else if parts.len() == 3 && parts[1] == "tracks" && method == "DELETE" {
            db.lock().unwrap().remove_track_from_playlist(pl_id, &url_decode(parts[2]));
            let _ = tx.try_send(RemoteCmd::RefreshLibrary);
            send_json(stream, 200, r#"{"ok":true}"#, &[]);
            return;
        }
    }

    // ── Library Tabs ─────────────────────────────────────────────────────
    if method == "GET" && path == "/api/library" {
        let tab = query_param(raw_url, "tab").unwrap_or_else(|| "downloads".into());
        let rows = {
            let d = db.lock().unwrap();
            match tab.as_str() {
                "orphans" => d.list_orphaned_offline(),
                "favorites" => d.list_favorites(),
                "history" => d.list_recent(100),
                _ => d.list_offline(),
            }
        };
        send_json(stream, 200, &serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into()), &[]);
        return;
    }

    // ── YouTube Playlist Importer ────────────────────────────────────────
    if method == "POST" && path == "/api/import/preview" {
        let Ok(body) = serde_json::from_slice::<ImportPreviewBody>(&req.body) else {
            send_err(stream, 400, "Invalid JSON");
            return;
        };
        if let Some(pl_id) = extract_playlist_id(&body.url) {
            match innertube.fetch_playlist_titled(&pl_id) {
                Ok((title, tracks)) => {
                    let json = serde_json::json!({ "title": title, "tracks": tracks }).to_string();
                    send_json(stream, 200, &json, &[]);
                }
                Err(e) => send_err(stream, 502, &e),
            }
        } else if let Some(PastedLink::Video(vid)) = parse_youtube_input(&body.url) {
            match innertube.resolve_audio_stream(&vid, false) {
                Ok(res) => {
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
                    let json = serde_json::json!({ "title": t.title, "tracks": vec![t] }).to_string();
                    send_json(stream, 200, &json, &[]);
                }
                Err(e) => send_err(stream, 502, &e),
            }
        } else {
            send_err(stream, 400, "Could not find a YouTube playlist or video ID in that URL");
        }
        return;
    }

    if method == "POST" && path == "/api/import/commit" {
        let Ok(body) = serde_json::from_slice::<ImportCommitBody>(&req.body) else {
            send_err(stream, 400, "Invalid JSON");
            return;
        };
        let clean = body.name.trim();
        let pl_name = if clean.is_empty() { "Imported Playlist" } else { clean };
        let pl_id = {
            let d = db.lock().unwrap();
            let Some(id) = d.create_playlist(pl_name) else {
                send_err(stream, 500, "Failed to create playlist");
                return;
            };
            for tr in &body.tracks {
                d.add_track_to_playlist(id, tr);
            }
            id
        };
        let count = body.tracks.len();
        if body.download && !body.tracks.is_empty() {
            let _ = tx.try_send(RemoteCmd::BatchDownload(body.tracks));
        }
        let _ = tx.try_send(RemoteCmd::RefreshLibrary);
        send_json(
            stream,
            200,
            &format!(r#"{{"playlist_id":{pl_id},"track_count":{count}}}"#),
            &[],
        );
        return;
    }

    // ── Offline File Deletion & Cleanup ──────────────────────────────────
    if method == "POST" && path == "/api/files/bulk-delete" {
        let Ok(body) = serde_json::from_slice::<BulkDeleteBody>(&req.body) else {
            send_err(stream, 400, "Invalid JSON");
            return;
        };
        let freed: u64 = {
            let d = db.lock().unwrap();
            body.video_ids.iter().map(|vid| d.remove_offline(vid)).sum()
        };
        let _ = tx.try_send(RemoteCmd::RefreshLibrary);
        send_json(
            stream,
            200,
            &format!(r#"{{"removed":{},"freed_bytes":{freed}}}"#, body.video_ids.len()),
            &[],
        );
        return;
    }

    if method == "POST" && path == "/api/files/clean-orphans" {
        let (removed, freed) = db.lock().unwrap().clean_orphaned_offline();
        let _ = tx.try_send(RemoteCmd::RefreshLibrary);
        send_json(
            stream,
            200,
            &format!(r#"{{"removed":{removed},"freed_bytes":{freed}}}"#),
            &[],
        );
        return;
    }

    if let Some(rest) = path.strip_prefix("/api/files/") {
        if let Some(vid) = rest.strip_suffix("/download") {
            if method == "POST" {
                let _ = tx.try_send(RemoteCmd::QueueDownload(url_decode(vid)));
                send_json(stream, 200, r#"{"ok":true}"#, &[]);
                return;
            }
        } else if method == "DELETE" {
            let freed = db.lock().unwrap().remove_offline(&url_decode(rest));
            let _ = tx.try_send(RemoteCmd::RefreshLibrary);
            send_json(stream, 200, &format!(r#"{{"freed_bytes":{freed}}}"#), &[]);
            return;
        }
    }

    // ── JSON Library Export ──────────────────────────────────────────────
    if method == "GET" && path == "/api/export" {
        let body = {
            let d = db.lock().unwrap();
            let playlists: Vec<serde_json::Value> = d
                .list_playlists()
                .into_iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "tracks": d.playlist_tracks(p.id),
                    })
                })
                .collect();
            let dump = serde_json::json!({
                "version": 1,
                "favorites": d.list_favorites(),
                "playlists": playlists,
            });
            serde_json::to_string_pretty(&dump).unwrap_or_else(|_| "{}".into())
        };
        send_response(
            stream,
            200,
            "OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
            &[("Content-Disposition", "attachment; filename=\"rusttune-library-backup.json\"")],
        );
        return;
    }

    // ── Cover Art Proxy ──────────────────────────────────────────────────
    if method == "GET" {
        if let Some(vid_raw) = path.strip_prefix("/api/cover/") {
            let vid = url_decode(vid_raw);
            let url = format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg");
            if let Ok(resp) = ureq::get(&url).timeout(Duration::from_secs(4)).call() {
                let mut bytes = Vec::new();
                if resp.into_reader().take(512 * 1024).read_to_end(&mut bytes).is_ok() {
                    send_response(
                        stream,
                        200,
                        "OK",
                        "image/jpeg",
                        &bytes,
                        &[("Cache-Control", "public, max-age=86400")],
                    );
                    return;
                }
            }
            send_err(stream, 404, "Cover not found");
            return;
        }
    }

    // ── Cached .m4a Audio Stream (Range Support) ─────────────────────────
    if method == "GET" {
        if let Some(vid_raw) = path.strip_prefix("/api/stream/") {
            let vid = url_decode(vid_raw);
            let file_path = cache_dir.join(format!("{vid}.m4a"));
            if let Ok(mut file) = std::fs::File::open(&file_path) {
                let total = file.metadata().map(|m| m.len()).unwrap_or(0);
                let range_hdr = req.headers.get("range").map(|s| s.as_str());
                if let Some((start, end)) = range_hdr.and_then(|r| parse_range(r, total)) {
                    let len = (end - start + 1) as usize;
                    let mut buf = vec![0u8; len];
                    if file.seek(SeekFrom::Start(start)).is_ok() && file.read_exact(&mut buf).is_ok() {
                        let cr = format!("bytes {start}-{end}/{total}");
                        send_response(
                            stream,
                            206,
                            "Partial Content",
                            "audio/mp4",
                            &buf,
                            &[("Accept-Ranges", "bytes"), ("Content-Range", &cr)],
                        );
                        return;
                    }
                }
                if let Ok(all) = std::fs::read(&file_path) {
                    send_response(
                        stream,
                        200,
                        "OK",
                        "audio/mp4",
                        &all,
                        &[("Accept-Ranges", "bytes")],
                    );
                    return;
                }
            }
            send_err(stream, 404, "Track not cached offline");
            return;
        }
    }

    send_err(stream, 404, "Not found");
}

fn send_json(stream: TcpStream, code: u16, body: &str, extra_headers: &[(&str, &str)]) {
    let status_text = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        _ => "Error",
    };
    send_response(
        stream,
        code,
        status_text,
        "application/json; charset=utf-8",
        body.as_bytes(),
        extra_headers,
    );
}

fn send_err(stream: TcpStream, code: u16, msg: &str) {
    let body = serde_json::json!({ "error": msg }).to_string();
    send_json(stream, code, &body, &[]);
}

fn send_text(stream: TcpStream, code: u16, mime: &str, msg: &str) {
    send_response(stream, code, "Not Found", mime, msg.as_bytes(), &[]);
}

fn send_response(
    mut stream: TcpStream,
    code: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
    extra_headers: &[(&str, &str)],
) {
    let mut head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start = a.parse::<u64>().ok()?;
    let end = if b.is_empty() {
        total - 1
    } else {
        b.parse::<u64>().ok()?.min(total - 1)
    };
    (start <= end).then_some((start, end))
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let qs = url.split_once('?')?.1;
    for pair in qs.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(url_decode(v));
            }
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                if let (Some(hi), Some(lo)) = (hex(b[i + 1]), hex(b[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_6_digit_pin() {
        let pin = generate_random_6_digit_pin();
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }
}
