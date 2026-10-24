// The browser side of art3m1s-core: the page (web/engines/art3m1s.js) mounts the game under
// /game and the saves under /saves in the Emscripten file system, makes a WebGL2 context and
// calls these functions once per animation frame. Everything goes through the versioned
// Art3m1sApiV1 table, like any other art3m1s host (doc/HOST_INTEGRATION.md).
use art3m1s_core::ffi_api::{Art3m1sApiV1, art3m1s_get_api_v1};
use art3m1s_core::host_events::HostEvents;
use art3m1s_core::host_files::HostResources;
use art3m1s_core::runtime::CoreRuntime;
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};

struct Session {
    api: &'static Art3m1sApiV1,
    resources: *mut HostResources,
    events: *mut HostEvents,
    runtime: *mut CoreRuntime,
    pixels: Vec<u8>,
    // Last poll_events output and the last resource read, read by the page.
    out: Vec<u8>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

macro_rules! call {
    ($api:expr, $name:ident $(, $arg:expr)*) => {
        unsafe { ($api.$name.expect(concat!("art3m1s: no ", stringify!($name))))($($arg),*) }
    };
}

fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with(|s| s.borrow_mut().as_mut().map(f))
}

fn c(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}

fn api() -> Result<&'static Art3m1sApiV1, String> {
    let mut size = 0usize;
    let api = unsafe { art3m1s_get_api_v1(&mut size) };
    if api.is_null() || size != std::mem::size_of::<Art3m1sApiV1>() {
        return Err("art3m1s: API table mismatch".into());
    }
    Ok(unsafe { &*api })
}

/// Reads the project INI (system.ini) from the mounted resources.
fn project_ini(resources: &HostResources) -> Result<Vec<u8>, String> {
    resources
        .read_file("system.ini")
        .map_err(|error| format!("system.ini: {error}"))
}

fn start(archive: &str, platform: &str) -> Result<(u32, u32), String> {
    let api = api()?;
    let events = call!(api, host_events_create);
    call!(api, host_events_enable, events, 1);
    let resources = call!(api, resources_create);
    let mounted = if archive.is_empty() {
        call!(api, resources_mount_directory, resources, c("/game").as_ptr())
    } else {
        let path = c(&format!("/game/{archive}"));
        call!(api, resources_mount_pfs, resources, path.as_ptr(), c("shift_jis").as_ptr())
    };
    if mounted != 1 {
        return Err(format!("art3m1s: cannot mount the game ({archive})"));
    }
    if call!(api, resources_set_save_dir, resources, c("/saves").as_ptr()) != 1 {
        return Err("art3m1s: cannot use /saves".into());
    }
    let ini = project_ini(unsafe { &*resources })?;
    // 1 = the GLES path, which the web build maps to WebGL2.
    let runtime = call!(api, runtime_create, 1280, 720, 1);
    if runtime.is_null() {
        return Err("art3m1s: runtime_create failed (see the log)".into());
    }
    call!(api, runtime_set_resources, runtime, resources);
    let platform = c(platform);
    if call!(api, runtime_load_project_bytes, runtime, ini.as_ptr(), ini.len(), platform.as_ptr()) != 0 {
        return Err("art3m1s: cannot load system.ini (see the log)".into());
    }
    let width = call!(api, runtime_stage_width, runtime);
    let height = call!(api, runtime_stage_height, runtime);
    let size = call!(api, runtime_pixel_buffer_size, runtime) as usize;
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session {
            api,
            resources,
            events,
            runtime,
            pixels: vec![0; size],
            out: Vec::new(),
        })
    });
    Ok((width, height))
}

fn set_error(message: String) {
    eprintln!("{message}");
    LAST_ERROR.with(|e| *e.borrow_mut() = c(&message));
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn str_arg<'a>(text: *const c_char) -> &'a str {
    if text.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(text) }.to_str().unwrap_or("")
    }
}

/// Starts the game: `archive` is the PFS under /game ("" to use /game as a folder) and
/// `platform` the system.ini section. Returns stage width << 16 | height, or 0 (see vn_error).
#[unsafe(no_mangle)]
pub extern "C" fn vn_start(archive: *const c_char, platform: *const c_char) -> u32 {
    match start(str_arg(archive), str_arg(platform)) {
        Ok((width, height)) => (width << 16) | height,
        Err(message) => {
            set_error(message);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// Advances `delta_ms` and renders. Returns the RGBA bytes written to vn_pixels (0: no new frame).
#[unsafe(no_mangle)]
pub extern "C" fn vn_frame(delta_ms: u32) -> u32 {
    with(|s| {
        let capacity = s.pixels.len() as u32;
        call!(s.api, runtime_advance_and_render, s.runtime, delta_ms, s.pixels.as_mut_ptr(), capacity)
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_pixels() -> *const u8 {
    with(|s| s.pixels.as_ptr()).unwrap_or(std::ptr::null())
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_exit_requested() -> i32 {
    with(|s| call!(s.api, runtime_is_exit_requested, s.runtime)).unwrap_or(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_mouse(x: i32, y: i32) {
    with(|s| call!(s.api, runtime_feed_mouse, s.runtime, x, y));
}

/// `button` is a Windows virtual key: 1 left, 2 right, 4 middle.
#[unsafe(no_mangle)]
pub extern "C" fn vn_button(button: u32, pressed: i32) {
    with(|s| call!(s.api, runtime_feed_mouse_button, s.runtime, button, pressed));
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_key(vk: u32, pressed: i32) {
    with(|s| call!(s.api, runtime_feed_key, s.runtime, vk, pressed));
}

/// Takes the queued host events (24-byte little-endian headers, then payloads) into vn_out.
/// Returns their byte length.
#[unsafe(no_mangle)]
pub extern "C" fn vn_poll_events() -> u32 {
    with(|s| {
        s.out.clear();
        loop {
            let next = call!(s.api, host_events_next, s.events);
            if next == 0 {
                break;
            }
            let start = s.out.len();
            s.out.resize(start + next.max(4096), 0);
            let mut count = 0u32;
            let written = call!(
                s.api,
                poll_events,
                s.events,
                s.out[start..].as_mut_ptr(),
                s.out.len() - start,
                &mut count
            );
            s.out.truncate(start + written);
            if written == 0 {
                break;
            }
        }
        s.out.len() as u32
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_out() -> *const u8 {
    with(|s| s.out.as_ptr()).unwrap_or(std::ptr::null())
}

/// Reads a game resource (sound files of media events) into vn_out. Returns its length, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn vn_read(path: *const c_char) -> i32 {
    let path = str_arg(path).to_owned();
    with(|s| match unsafe { &*s.resources }.read_file(&path) {
        Ok(bytes) => {
            s.out = bytes;
            s.out.len() as i32
        }
        Err(error) => {
            eprintln!("art3m1s: read {path}: {error}");
            -1
        }
    })
    .unwrap_or(-1)
}

/// A sound finished: `id` "" is the BGM.
#[unsafe(no_mangle)]
pub extern "C" fn vn_sound_finished(id: *const c_char) {
    let id = str_arg(id);
    let id = (!id.is_empty()).then(|| c(id));
    with(|s| {
        call!(
            s.api,
            runtime_notify_sound_finished,
            s.runtime,
            id.as_ref().map_or(std::ptr::null(), |id| id.as_ptr())
        )
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_video_finished(id: *const c_char) {
    let id = str_arg(id);
    let id = (!id.is_empty()).then(|| c(id));
    with(|s| {
        call!(
            s.api,
            runtime_notify_video_finished,
            s.runtime,
            id.as_ref().map_or(std::ptr::null(), |id| id.as_ptr())
        )
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn vn_dialog(accepted: i32, text: *const c_char) -> i32 {
    with(|s| call!(s.api, runtime_submit_dialog, s.runtime, accepted, text)).unwrap_or(0)
}

/// 0 before leaving, 1 hidden, 2 shown again.
#[unsafe(no_mangle)]
pub extern "C" fn vn_lifecycle(state: i32) {
    with(|s| call!(s.api, runtime_notify_lifecycle, s.runtime, state));
}

fn main() {
    // The page drives the engine; keep the runtime alive after main returns.
}
