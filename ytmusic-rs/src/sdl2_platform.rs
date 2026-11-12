use libloading::{Library, Symbol};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, WindowAdapter};
use slint::PhysicalSize;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CString};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
pub const AUDIO_FREQ: i32 = 44100;

const SDL_INIT_AUDIO: u32 = 0x00000010;
const SDL_INIT_VIDEO: u32 = 0x00000020;
const SDL_INIT_JOYSTICK: u32 = 0x00000200;
const SDL_INIT_GAMECONTROLLER: u32 = 0x00002000;

const SDL_WINDOW_SHOWN: u32 = 0x00000004;
const SDL_WINDOW_RESIZABLE: u32 = 0x00000020;
const SDL_WINDOW_FULLSCREEN_DESKTOP: u32 = 0x00001001;
const SDL_WINDOWPOS_CENTERED: c_int = 0x2FFF0000;

const SDL_RENDERER_ACCELERATED: u32 = 0x00000002;
const SDL_RENDERER_PRESENTVSYNC: u32 = 0x00000004;
const SDL_RENDERER_SOFTWARE: u32 = 0x00000001;

const SDL_PIXELFORMAT_RGB565: u32 = 0x15151002;
const SDL_TEXTUREACCESS_STREAMING: c_int = 1;

const SDL_QUIT: u32 = 0x100;
const SDL_KEYDOWN: u32 = 0x300;
const SDL_KEYUP: u32 = 0x301;
const SDL_CONTROLLERAXISMOTION: u32 = 0x650;
const SDL_CONTROLLERBUTTONDOWN: u32 = 0x651;
const SDL_CONTROLLERBUTTONUP: u32 = 0x652;
const SDL_CONTROLLERDEVICEADDED: u32 = 0x653;

const AUDIO_S16LSB: u16 = 0x8010;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandheldBtn {
    Up,
    Down,
    Left,
    Right,
    RightUp,
    RightDown,
    RightLeft,
    RightRight,
    A,
    B,
    X,
    Y,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Start,
    Select,
    Guide,
}

/// Abstract playback & host controls so `input` and `workers` stay unit-testable without SDL2.
pub trait PlaybackHost {
    fn clear_audio_queue(&self);
    fn set_audio_paused(&self, paused: bool);
    fn set_swap_ab(&mut self, swap: bool);
    fn request_quit(&mut self);
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlAudioSpec {
    freq: c_int,
    format: u16,
    channels: u8,
    silence: u8,
    samples: u16,
    padding: u16,
    size: u32,
    callback: *mut c_void,
    userdata: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlRect {
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlKeysym {
    scancode: i32,
    sym: i32,
    kmod: u16,
    unused: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlKeyboardEvent {
    etype: u32,
    timestamp: u32,
    window_id: u32,
    state: u8,
    repeat: u8,
    pad2: u8,
    pad3: u8,
    keysym: SdlKeysym,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlControllerAxisEvent {
    etype: u32,
    timestamp: u32,
    which: i32,
    axis: u8,
    pad1: u8,
    pad2: u8,
    pad3: u8,
    value: i16,
    pad4: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlControllerButtonEvent {
    etype: u32,
    timestamp: u32,
    which: i32,
    button: u8,
    state: u8,
    pad1: u8,
    pad2: u8,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SdlControllerDeviceEvent {
    etype: u32,
    timestamp: u32,
    which: i32,
}

#[repr(C)]
union SdlEvent {
    etype: u32,
    key: SdlKeyboardEvent,
    caxis: SdlControllerAxisEvent,
    cbutton: SdlControllerButtonEvent,
    cdevice: SdlControllerDeviceEvent,
    padding: [u8; 64],
}

pub struct SdlPlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for SdlPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

fn load_lib(names: &[&str]) -> Option<Library> {
    let mut dirs = Vec::new();
    if let Ok(val) = std::env::var("LD_LIBRARY_PATH") {
        for p in val.split(if cfg!(windows) { ';' } else { ':' }) {
            if !p.is_empty() {
                dirs.push(p.to_string());
            }
        }
    }
    dirs.extend([
        "/usr/lib".into(),
        "/usr/lib/aarch64-linux-gnu".into(),
        "/usr/local/lib".into(),
        "/lib".into(),
    ]);
    for d in &dirs {
        for n in names {
            let p = Path::new(d).join(n);
            if p.is_file() {
                if let Ok(lib) = unsafe { Library::new(p) } {
                    return Some(lib);
                }
            }
        }
    }
    for n in names {
        if let Ok(lib) = unsafe { Library::new(*n) } {
            return Some(lib);
        }
    }
    None
}

pub struct SdlHost {
    sdl: Library,
    sdl_window: *mut c_void,
    renderer: *mut c_void,
    texture: *mut c_void,
    audio_dev: u32,
    pub slint_window: Rc<MinimalSoftwareWindow>,
    fb: Vec<Rgb565Pixel>,
    controllers: HashMap<i32, *mut c_void>,
    held: HashMap<HandheldBtn, Instant>,
    repeat_at: HashMap<HandheldBtn, Instant>,
    axis_state: HashMap<u8, Option<HandheldBtn>>,
    pub swap_ab: bool,
    pub should_quit: bool,
}

impl SdlHost {
    pub fn init() -> Result<Self, String> {
        let sdl = load_lib(&["libSDL2-2.0.so.0", "libSDL2.so", "SDL2.dll"])
            .ok_or_else(|| "Could not load libSDL2".to_string())?;

        let flags = SDL_INIT_VIDEO | SDL_INIT_AUDIO | SDL_INIT_JOYSTICK | SDL_INIT_GAMECONTROLLER;

        unsafe {
            let sdl_init: Symbol<unsafe extern "C" fn(u32) -> c_int> =
                sdl.get(b"SDL_Init\0").map_err(|e| e.to_string())?;
            if sdl_init(flags) != 0 {
                return Err("SDL_Init failed".into());
            }

            // Open queued PCM audio device (44.1kHz stereo signed 16-bit)
            let mut audio_dev = 0u32;
            if let Ok(open_aud) = sdl.get::<unsafe extern "C" fn(
                *const c_char,
                c_int,
                *const SdlAudioSpec,
                *mut SdlAudioSpec,
                c_int,
            ) -> u32>(b"SDL_OpenAudioDevice\0")
            {
                let desired = SdlAudioSpec {
                    freq: AUDIO_FREQ,
                    format: AUDIO_S16LSB,
                    channels: 2,
                    silence: 0,
                    samples: 2048,
                    padding: 0,
                    size: 0,
                    callback: std::ptr::null_mut(),
                    userdata: std::ptr::null_mut(),
                };
                let mut obtained = desired;
                audio_dev = open_aud(std::ptr::null(), 0, &desired, &mut obtained, 0);
                if audio_dev != 0 {
                    if let Ok(pause_aud) =
                        sdl.get::<unsafe extern "C" fn(u32, c_int)>(b"SDL_PauseAudioDevice\0")
                    {
                        pause_aud(audio_dev, 0);
                    }
                }
            }

            if let Ok(add_map) =
                sdl.get::<unsafe extern "C" fn(*const c_char) -> c_int>(b"SDL_GameControllerAddMapping\0")
            {
                if let Ok(path) = std::env::var("SDL_GAMECONTROLLERCONFIG_FILE") {
                    if let Ok(txt) = std::fs::read_to_string(path) {
                        for line in txt.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
                            if let Ok(cs) = CString::new(line) {
                                add_map(cs.as_ptr());
                            }
                        }
                    }
                }
                if let Ok(inline) = std::env::var("SDL_GAMECONTROLLERCONFIG") {
                    for line in inline.lines().map(str::trim).filter(|l| !l.is_empty()) {
                        if let Ok(cs) = CString::new(line) {
                            add_map(cs.as_ptr());
                        }
                    }
                }
            }

            let title = CString::new("RustTune").unwrap();
            let win_flags = SDL_WINDOW_SHOWN
                | if cfg!(target_os = "linux") {
                    SDL_WINDOW_FULLSCREEN_DESKTOP
                } else {
                    SDL_WINDOW_RESIZABLE
                };
            let create_win: Symbol<
                unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void,
            > = sdl.get(b"SDL_CreateWindow\0").map_err(|e| e.to_string())?;
            let sdl_window = create_win(
                title.as_ptr(),
                SDL_WINDOWPOS_CENTERED,
                SDL_WINDOWPOS_CENTERED,
                WIDTH as c_int,
                HEIGHT as c_int,
                win_flags,
            );
            if sdl_window.is_null() {
                return Err("SDL_CreateWindow failed".into());
            }

            let create_ren: Symbol<unsafe extern "C" fn(*mut c_void, c_int, u32) -> *mut c_void> =
                sdl.get(b"SDL_CreateRenderer\0").map_err(|e| e.to_string())?;
            let mut renderer =
                create_ren(sdl_window, -1, SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC);
            if renderer.is_null() {
                renderer = create_ren(sdl_window, -1, SDL_RENDERER_SOFTWARE);
            }
            if renderer.is_null() {
                return Err("SDL_CreateRenderer failed".into());
            }

            let set_logical: Symbol<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int> =
                sdl.get(b"SDL_RenderSetLogicalSize\0").map_err(|e| e.to_string())?;
            set_logical(renderer, WIDTH as c_int, HEIGHT as c_int);

            let create_tex: Symbol<
                unsafe extern "C" fn(*mut c_void, u32, c_int, c_int, c_int) -> *mut c_void,
            > = sdl.get(b"SDL_CreateTexture\0").map_err(|e| e.to_string())?;
            let texture = create_tex(
                renderer,
                SDL_PIXELFORMAT_RGB565,
                SDL_TEXTUREACCESS_STREAMING,
                WIDTH as c_int,
                HEIGHT as c_int,
            );
            if texture.is_null() {
                return Err("SDL_CreateTexture failed".into());
            }

            let slint_window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
            slint_window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
            let _ = slint::platform::set_platform(Box::new(SdlPlatform {
                window: slint_window.clone(),
                start: Instant::now(),
            }));

            let mut host = Self {
                sdl,
                sdl_window,
                renderer,
                texture,
                audio_dev,
                slint_window,
                fb: vec![Rgb565Pixel(0); (WIDTH * HEIGHT) as usize],
                controllers: HashMap::new(),
                held: HashMap::new(),
                repeat_at: HashMap::new(),
                axis_state: HashMap::new(),
                swap_ab: false,
                should_quit: false,
            };
            host.open_controllers();
            Ok(host)
        }
    }

    pub fn queue_pcm(&self, samples: &[i16]) {
        if self.audio_dev == 0 || samples.is_empty() {
            return;
        }
        unsafe {
            if let Ok(q) = self.sdl.get::<unsafe extern "C" fn(u32, *const c_void, u32) -> c_int>(
                b"SDL_QueueAudio\0",
            ) {
                q(
                    self.audio_dev,
                    samples.as_ptr() as *const c_void,
                    (samples.len() * std::mem::size_of::<i16>()) as u32,
                );
            }
        }
    }

    pub fn clear_audio_queue(&self) {
        if self.audio_dev == 0 {
            return;
        }
        unsafe {
            if let Ok(clr) = self.sdl.get::<unsafe extern "C" fn(u32)>(b"SDL_ClearQueuedAudio\0") {
                clr(self.audio_dev);
            }
        }
    }

    pub fn set_audio_paused(&self, paused: bool) {
        if self.audio_dev == 0 {
            return;
        }
        unsafe {
            if let Ok(p) = self.sdl.get::<unsafe extern "C" fn(u32, c_int)>(b"SDL_PauseAudioDevice\0") {
                p(self.audio_dev, if paused { 1 } else { 0 });
            }
        }
    }
}

impl PlaybackHost for SdlHost {
    fn clear_audio_queue(&self) {
        SdlHost::clear_audio_queue(self);
    }

    fn set_audio_paused(&self, paused: bool) {
        SdlHost::set_audio_paused(self, paused);
    }

    fn set_swap_ab(&mut self, swap: bool) {
        self.swap_ab = swap;
    }

    fn request_quit(&mut self) {
        self.should_quit = true;
    }
}

impl SdlHost {

    pub fn queued_audio_bytes(&self) -> u32 {
        if self.audio_dev == 0 {
            return 0;
        }
        unsafe {
            if let Ok(f) = self.sdl.get::<unsafe extern "C" fn(u32) -> u32>(b"SDL_GetQueuedAudioSize\0") {
                return f(self.audio_dev);
            }
        }
        0
    }

    fn open_controllers(&mut self) {
        unsafe {
            let Ok(num_joy) = self.sdl.get::<unsafe extern "C" fn() -> c_int>(b"SDL_NumJoysticks\0") else {
                return;
            };
            let Ok(is_gc) = self.sdl.get::<unsafe extern "C" fn(c_int) -> c_int>(b"SDL_IsGameController\0") else {
                return;
            };
            let Ok(open_gc) = self.sdl.get::<unsafe extern "C" fn(c_int) -> *mut c_void>(b"SDL_GameControllerOpen\0") else {
                return;
            };
            let n = num_joy();
            for i in 0..n {
                if !self.controllers.contains_key(&i) && is_gc(i) != 0 {
                    let c = open_gc(i);
                    if !c.is_null() {
                        self.controllers.insert(i, c);
                    }
                }
            }
        }
    }

    fn map_raw_btn(&self, btn: HandheldBtn) -> HandheldBtn {
        if !self.swap_ab {
            return btn;
        }
        match btn {
            HandheldBtn::A => HandheldBtn::B,
            HandheldBtn::B => HandheldBtn::A,
            other => other,
        }
    }

    pub fn poll_actions(&mut self, wait_ms: c_int) -> Vec<HandheldBtn> {
        let mut out = Vec::new();
        let mut ev = SdlEvent { padding: [0; 64] };
        let now = Instant::now();
        let mut need_reopen = false;

        unsafe {
            let wait_ev: Option<Symbol<unsafe extern "C" fn(*mut SdlEvent, c_int) -> c_int>> =
                self.sdl.get(b"SDL_WaitEventTimeout\0").ok();
            let poll_ev: Symbol<unsafe extern "C" fn(*mut SdlEvent) -> c_int> =
                self.sdl.get(b"SDL_PollEvent\0").expect("SDL_PollEvent");

            let mut first = if let Some(ref wf) = wait_ev {
                if wait_ms > 0 && self.held.is_empty() {
                    wf(&mut ev as *mut SdlEvent, wait_ms) != 0
                } else {
                    poll_ev(&mut ev as *mut SdlEvent) != 0
                }
            } else {
                poll_ev(&mut ev as *mut SdlEvent) != 0
            };

            while first {
                match ev.etype {
                    SDL_QUIT => self.should_quit = true,
                    SDL_CONTROLLERDEVICEADDED => need_reopen = true,
                    SDL_CONTROLLERBUTTONDOWN => {
                        if let Some(raw) = sdl_btn_to_handheld(ev.cbutton.button) {
                            let b = self.map_raw_btn(raw);
                            self.held.insert(b, now);
                            self.repeat_at.insert(b, now + Duration::from_millis(260));
                            out.push(b);
                        }
                    }
                    SDL_CONTROLLERBUTTONUP => {
                        if let Some(raw) = sdl_btn_to_handheld(ev.cbutton.button) {
                            let b = self.map_raw_btn(raw);
                            self.held.remove(&b);
                            self.repeat_at.remove(&b);
                        }
                    }
                    SDL_CONTROLLERAXISMOTION => {
                        let axis = ev.caxis.axis;
                        let val = ev.caxis.value;
                        let cur = match axis {
                            0 if val < -14000 => Some(HandheldBtn::Left),
                            0 if val > 14000 => Some(HandheldBtn::Right),
                            1 if val < -14000 => Some(HandheldBtn::Up),
                            1 if val > 14000 => Some(HandheldBtn::Down),
                            2 if val < -14000 => Some(HandheldBtn::RightLeft),
                            2 if val > 14000 => Some(HandheldBtn::RightRight),
                            3 if val < -14000 => Some(HandheldBtn::RightUp),
                            3 if val > 14000 => Some(HandheldBtn::RightDown),
                            4 if val > 10000 => Some(HandheldBtn::L2),
                            5 if val > 10000 => Some(HandheldBtn::R2),
                            _ => None,
                        };
                        let prev = self.axis_state.get(&axis).copied().flatten();
                        if cur != prev {
                            if let Some(p) = prev {
                                self.held.remove(&p);
                                self.repeat_at.remove(&p);
                            }
                            self.axis_state.insert(axis, cur);
                            if let Some(c) = cur {
                                self.held.insert(c, now);
                                self.repeat_at.insert(c, now + Duration::from_millis(260));
                                out.push(c);
                            }
                        }
                    }
                    SDL_KEYDOWN if ev.key.repeat == 0 => {
                        if let Some(raw) = sdl_key_to_handheld(ev.key.keysym.sym) {
                            let b = self.map_raw_btn(raw);
                            self.held.insert(b, now);
                            self.repeat_at.insert(b, now + Duration::from_millis(260));
                            out.push(b);
                        }
                    }
                    SDL_KEYUP => {
                        if let Some(raw) = sdl_key_to_handheld(ev.key.keysym.sym) {
                            let b = self.map_raw_btn(raw);
                            self.held.remove(&b);
                            self.repeat_at.remove(&b);
                        }
                    }
                    _ => {}
                }
                first = poll_ev(&mut ev as *mut SdlEvent) != 0;
            }
        }
        if need_reopen {
            self.open_controllers();
        }

        if self.held.contains_key(&HandheldBtn::Start)
            && (self.held.contains_key(&HandheldBtn::Select)
                || self.held.contains_key(&HandheldBtn::Guide))
        {
            self.should_quit = true;
        }

        for b in [
            HandheldBtn::Up,
            HandheldBtn::Down,
            HandheldBtn::Left,
            HandheldBtn::Right,
            HandheldBtn::RightUp,
            HandheldBtn::RightDown,
            HandheldBtn::RightLeft,
            HandheldBtn::RightRight,
            HandheldBtn::L1,
            HandheldBtn::R1,
        ] {
            if let Some(&t) = self.repeat_at.get(&b) {
                if now >= t {
                    self.repeat_at.insert(b, now + Duration::from_millis(75));
                    out.push(b);
                }
            }
        }
        out
    }

    pub fn render_if_needed(&mut self, force_present: bool) {
        slint::platform::update_timers_and_animations();
        if self.slint_window.has_active_animations() {
            self.slint_window.request_redraw();
        }
        let mut drew = false;
        let fb = &mut self.fb;
        self.slint_window.draw_if_needed(|renderer| {
            renderer.render(fb, WIDTH as usize);
            drew = true;
        });

        if drew || force_present {
            unsafe {
                let Ok(upd) = self.sdl.get::<unsafe extern "C" fn(*mut c_void, *const SdlRect, *const c_void, c_int) -> c_int>(b"SDL_UpdateTexture\0") else {
                    return;
                };
                let Ok(clr) = self.sdl.get::<unsafe extern "C" fn(*mut c_void) -> c_int>(b"SDL_RenderClear\0") else {
                    return;
                };
                let Ok(cpy) = self.sdl.get::<unsafe extern "C" fn(*mut c_void, *mut c_void, *const SdlRect, *const SdlRect) -> c_int>(b"SDL_RenderCopy\0") else {
                    return;
                };
                let Ok(pres) = self.sdl.get::<unsafe extern "C" fn(*mut c_void)>(b"SDL_RenderPresent\0") else {
                    return;
                };
                upd(
                    self.texture,
                    std::ptr::null(),
                    self.fb.as_ptr() as *const c_void,
                    (WIDTH * 2) as c_int,
                );
                clr(self.renderer);
                cpy(self.renderer, self.texture, std::ptr::null(), std::ptr::null());
                pres(self.renderer);
            }
        }
    }
}

impl Drop for SdlHost {
    fn drop(&mut self) {
        unsafe {
            if self.audio_dev != 0 {
                if let Ok(ca) = self.sdl.get::<unsafe extern "C" fn(u32)>(b"SDL_CloseAudioDevice\0") {
                    ca(self.audio_dev);
                }
            }
            if let Ok(f) = self.sdl.get::<unsafe extern "C" fn(*mut c_void)>(b"SDL_GameControllerClose\0") {
                for &c in self.controllers.values() {
                    f(c);
                }
            }
            if let Ok(f) = self.sdl.get::<unsafe extern "C" fn(*mut c_void)>(b"SDL_DestroyTexture\0") {
                f(self.texture);
            }
            if let Ok(f) = self.sdl.get::<unsafe extern "C" fn(*mut c_void)>(b"SDL_DestroyRenderer\0") {
                f(self.renderer);
            }
            if let Ok(f) = self.sdl.get::<unsafe extern "C" fn(*mut c_void)>(b"SDL_DestroyWindow\0") {
                f(self.sdl_window);
            }
            if let Ok(q) = self.sdl.get::<unsafe extern "C" fn()>(b"SDL_Quit\0") {
                q();
            }
        }
    }
}

fn sdl_btn_to_handheld(b: u8) -> Option<HandheldBtn> {
    match b {
        0 => Some(HandheldBtn::A),
        1 => Some(HandheldBtn::B),
        2 => Some(HandheldBtn::X),
        3 => Some(HandheldBtn::Y),
        4 => Some(HandheldBtn::Select),
        5 => Some(HandheldBtn::Guide),
        6 => Some(HandheldBtn::Start),
        7 => Some(HandheldBtn::L3),
        8 => Some(HandheldBtn::R3),
        9 => Some(HandheldBtn::L1),
        10 => Some(HandheldBtn::R1),
        11 => Some(HandheldBtn::Up),
        12 => Some(HandheldBtn::Down),
        13 => Some(HandheldBtn::Left),
        14 => Some(HandheldBtn::Right),
        _ => None,
    }
}

fn sdl_key_to_handheld(sym: i32) -> Option<HandheldBtn> {
    match sym {
        0x40000052 => Some(HandheldBtn::Up),
        0x40000051 => Some(HandheldBtn::Down),
        0x40000050 => Some(HandheldBtn::Left),
        0x4000004F => Some(HandheldBtn::Right),
        0x4000004B => Some(HandheldBtn::RightUp),   // PageUp
        0x4000004E => Some(HandheldBtn::RightDown), // PageDown
        32 | 122 => Some(HandheldBtn::A),      // Space / z
        120 | 27 => Some(HandheldBtn::B),      // x / Esc
        97 => Some(HandheldBtn::X),            // a
        115 => Some(HandheldBtn::Y),           // s
        113 => Some(HandheldBtn::L1),          // q
        101 => Some(HandheldBtn::R1),          // e
        91 => Some(HandheldBtn::L2),           // [
        93 => Some(HandheldBtn::R2),           // ]
        92 => Some(HandheldBtn::L3),           // backslash
        47 => Some(HandheldBtn::R3),           // slash
        13 => Some(HandheldBtn::Start),        // Enter
        8 => Some(HandheldBtn::Select),        // Backspace
        _ => None,
    }
}
