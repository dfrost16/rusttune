use crate::sdl2_platform::AUDIO_FREQ;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

const CHUNK_SIZE: u64 = 512 * 1024; // 512 KB Range chunks

/// Seekable HTTP Range-backed MediaSource for YouTube googlevideo URLs.
pub struct HttpRangeSource {
    agent: ureq::Agent,
    url: String,
    user_agent: String,
    pos: u64,
    total_len: Option<u64>,
    cache: HashMap<u64, Vec<u8>>,
    cancel: Arc<AtomicBool>,
}

impl HttpRangeSource {
    pub fn open(
        url: String,
        user_agent: String,
        known_len: Option<u64>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(8))
            .timeout_read(std::time::Duration::from_secs(15))
            .build();
        let mut src = Self {
            agent,
            url,
            user_agent,
            pos: 0,
            total_len: known_len,
            cache: HashMap::new(),
            cancel,
        };
        // Prime first chunk so we also learn exact Content-Range total length
        src.fetch_chunk(0)?;
        Ok(src)
    }

    fn fetch_chunk(&mut self, chunk_idx: u64) -> Result<(), String> {
        if self.cache.contains_key(&chunk_idx) {
            return Ok(());
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let start = chunk_idx * CHUNK_SIZE;
        let end = if let Some(tot) = self.total_len {
            if start >= tot {
                return Ok(());
            }
            (start + CHUNK_SIZE - 1).min(tot - 1)
        } else {
            start + CHUNK_SIZE - 1
        };

        let mut last_err = String::new();
        for attempt in 0..3 {
            if self.cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            match self
                .agent
                .get(&self.url)
                .set("User-Agent", &self.user_agent)
                .set("Range", &format!("bytes={start}-{end}"))
                .call()
            {
                Ok(resp) => {
                    if self.total_len.is_none() {
                        if let Some(cr) = resp.header("Content-Range") {
                            if let Some(slash) = cr.rfind('/') {
                                if let Ok(tot) = cr[slash + 1..].trim().parse::<u64>() {
                                    self.total_len = Some(tot);
                                }
                            }
                        }
                    }

                    let mut buf = Vec::with_capacity(CHUNK_SIZE as usize);
                    match resp.into_reader().take(CHUNK_SIZE + 1024).read_to_end(&mut buf) {
                        Ok(_) => {
                            if self.cache.len() > 32 {
                                let oldest = self
                                    .cache
                                    .keys()
                                    .copied()
                                    .filter(|&k| k != chunk_idx && k + 1 != chunk_idx)
                                    .next();
                                if let Some(k) = oldest {
                                    self.cache.remove(&k);
                                }
                            }
                            self.cache.insert(chunk_idx, buf);
                            return Ok(());
                        }
                        Err(e) => last_err = format!("HTTP read {start}-{end}: {e}"),
                    }
                }
                Err(e) => last_err = format!("HTTP range {start}-{end}: {e}"),
            }
            if attempt < 2 {
                thread::sleep(std::time::Duration::from_millis(250));
            }
        }
        Err(last_err)
    }
}

impl Read for HttpRangeSource {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
        if let Some(tot) = self.total_len {
            if self.pos >= tot {
                return Ok(0);
            }
        }
        let chunk_idx = self.pos / CHUNK_SIZE;
        let offset = (self.pos % CHUNK_SIZE) as usize;
        self.fetch_chunk(chunk_idx)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let Some(chunk) = self.cache.get(&chunk_idx) else {
            return Ok(0);
        };
        if offset >= chunk.len() {
            return Ok(0);
        }
        let n = (chunk.len() - offset).min(out.len());
        out[..n].copy_from_slice(&chunk[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for HttpRangeSource {
    fn seek(&mut self, style: SeekFrom) -> std::io::Result<u64> {
        let new_pos: i64 = match style {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(delta) => self.pos as i64 + delta,
            SeekFrom::End(delta) => {
                let tot = self.total_len.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::Unsupported, "unknown length")
                })?;
                tot as i64 + delta
            }
        };
        if new_pos < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "negative seek"));
        }
        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}

impl MediaSource for HttpRangeSource {
    fn is_seekable(&self) -> bool {
        self.total_len.is_some()
    }
    fn byte_len(&self) -> Option<u64> {
        self.total_len
    }
}

pub enum AudioEvent {
    Pcm { pos_ms: u64, samples: Vec<i16> },
    Started { duration_secs: u64 },
    Seeked(u64),
    Ended,
    Error(String),
}

pub struct PlaybackSession {
    cancel: Arc<AtomicBool>,
    seek_req_secs: Arc<AtomicU64>,
    seek_pending: Arc<AtomicBool>,
    pub rx: mpsc::Receiver<AudioEvent>,
}

impl PlaybackSession {
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn seek_to(&self, secs: u64) {
        self.seek_req_secs.store(secs, Ordering::Relaxed);
        self.seek_pending.store(true, Ordering::Relaxed);
    }
}

pub fn start_local_playback(path: &Path) -> PlaybackSession {
    let p = path.to_path_buf();
    spawn_decoder(move |cancel, seek_secs, seek_pending, tx| {
        let file = File::open(&p).map_err(|e| format!("Open {}: {e}", p.display()))?;
        let mut hint = Hint::new();
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        decode_source(Box::new(file), hint, cancel, seek_secs, seek_pending, tx)
    })
}

pub fn start_stream_playback(
    url: String,
    user_agent: String,
    known_len: Option<u64>,
) -> PlaybackSession {
    spawn_decoder(move |cancel, seek_secs, seek_pending, tx| {
        let src = HttpRangeSource::open(url, user_agent, known_len, cancel.clone())?;
        let mut hint = Hint::new();
        hint.with_extension("m4a");
        decode_source(Box::new(src), hint, cancel, seek_secs, seek_pending, tx)
    })
}

fn spawn_decoder<F>(f: F) -> PlaybackSession
where
    F: FnOnce(
            Arc<AtomicBool>,
            Arc<AtomicU64>,
            Arc<AtomicBool>,
            mpsc::SyncSender<AudioEvent>,
        ) -> Result<(), String>
        + Send
        + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let seek_req_secs = Arc::new(AtomicU64::new(0));
    let seek_pending = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(10);

    let c2 = cancel.clone();
    let s2 = seek_req_secs.clone();
    let p2 = seek_pending.clone();
    thread::spawn(move || {
        if let Err(e) = f(c2.clone(), s2, p2, tx.clone()) {
            if !c2.load(Ordering::Relaxed) {
                let _ = tx.send(AudioEvent::Error(e));
            }
        }
    });

    PlaybackSession {
        cancel,
        seek_req_secs,
        seek_pending,
        rx,
    }
}

fn decode_source(
    source: Box<dyn MediaSource>,
    hint: Hint,
    cancel: Arc<AtomicBool>,
    seek_secs: Arc<AtomicU64>,
    seek_pending: Arc<AtomicBool>,
    tx: mpsc::SyncSender<AudioEvent>,
) -> Result<(), String> {
    let mss = MediaSourceStream::new(source, Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Audio probe failed: {e}"))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| "No audio track found".to_string())?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let duration_secs = codec_params
        .n_frames
        .zip(codec_params.sample_rate)
        .map(|(n, sr)| n / sr.max(1) as u64)
        .unwrap_or(0);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Audio decoder init failed: {e}"))?;

    let _ = tx.send(AudioEvent::Started { duration_secs });

    let mut sample_buf: Option<SampleBuffer<i16>> = None;
    let mut base_ms: u64 = 0;
    let mut decoded_frames_since_seek: u64 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        if seek_pending.swap(false, Ordering::Relaxed) {
            let sec = seek_secs.load(Ordering::Relaxed);
            if format
                .seek(
                    SeekMode::Coarse,
                    SeekTo::Time {
                        time: Time::from(sec),
                        track_id: Some(track_id),
                    },
                )
                .is_ok()
            {
                decoder.reset();
                base_ms = sec * 1000;
                decoded_frames_since_seek = 0;
                let _ = tx.send(AudioEvent::Seeked(base_ms));
            }
        }

        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                let _ = tx.send(AudioEvent::Ended);
                return Ok(());
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("Packet read error: {e}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let frames = decoded.frames();
                if frames == 0 {
                    continue;
                }
                let sbuf = sample_buf.get_or_insert_with(|| {
                    SampleBuffer::<i16>::new(decoded.capacity() as u64, spec)
                });
                sbuf.copy_interleaved_ref(decoded);
                let pcm = resample_to_44100_stereo(
                    sbuf.samples(),
                    spec.rate as i32,
                    spec.channels.count(),
                );
                decoded_frames_since_seek += (pcm.len() / 2) as u64;
                let pos_ms = base_ms + (decoded_frames_since_seek * 1000) / (AUDIO_FREQ as u64);
                if tx
                    .send(AudioEvent::Pcm {
                        pos_ms,
                        samples: pcm,
                    })
                    .is_err()
                {
                    return Ok(());
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Decode error: {e}")),
        }
    }
}

/// Download a resolved audio URL to `dest_path` cleanly using Range chunks.
pub fn download_to_file(
    url: &str,
    user_agent: &str,
    known_len: Option<u64>,
    dest_path: &Path,
) -> Result<u64, String> {
    use std::io::Write;
    let tmp = dest_path.with_extension("m4a.part");
    if let Some(parent) = tmp.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let mut src = HttpRangeSource::open(url.to_string(), user_agent.to_string(), known_len, cancel)?;
    let mut out = File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = src.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        total += n as u64;
    }
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&tmp, dest_path).map_err(|e| e.to_string())?;
    Ok(total)
}

pub fn resample_to_44100_stereo(input: &[i16], src_rate: i32, channels: usize) -> Vec<i16> {
    if channels == 0 || input.is_empty() {
        return Vec::new();
    }
    let src_frames = input.len() / channels;
    if src_rate == AUDIO_FREQ && channels == 2 {
        return input.to_vec();
    }
    let dst_frames = ((src_frames as u64) * (AUDIO_FREQ as u64) / (src_rate.max(1) as u64)).max(1) as usize;
    let mut out = Vec::with_capacity(dst_frames * 2);
    for i in 0..dst_frames {
        let src_pos = (i as f64) * (src_rate as f64) / (AUDIO_FREQ as f64);
        let idx0 = (src_pos.floor() as usize).min(src_frames.saturating_sub(1));
        let idx1 = (idx0 + 1).min(src_frames.saturating_sub(1));
        let frac = src_pos - (idx0 as f64);

        let (l0, r0) = frame_lr(input, idx0, channels);
        let (l1, r1) = frame_lr(input, idx1, channels);
        let l = (l0 as f64 + (l1 as f64 - l0 as f64) * frac).round() as i16;
        let r = (r0 as f64 + (r1 as f64 - r0 as f64) * frac).round() as i16;
        out.push(l);
        out.push(r);
    }
    out
}

#[inline]
fn frame_lr(buf: &[i16], frame: usize, channels: usize) -> (i16, i16) {
    let base = frame * channels;
    let l = buf[base];
    let r = if channels >= 2 { buf[base + 1] } else { l };
    (l, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_mono_48k_to_stereo_44k() {
        let mono_48k: Vec<i16> = (0..480).map(|i| (i * 10) as i16).collect();
        let stereo = resample_to_44100_stereo(&mono_48k, 48000, 1);
        assert_eq!(stereo.len(), 441 * 2);
        assert_eq!(stereo[0], stereo[1]);
    }
}
