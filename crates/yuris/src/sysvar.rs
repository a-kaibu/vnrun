//! System variables (`@_LC`, `@_MOUSE_X`, ...): numbers 0 to 512, typed in ysv.ybn. The
//! compiled scripts only have the numbers; these were matched against the names in the SDK's
//! ERIS sources.

use crate::expr::Value;
use crate::vm::State;

/// @_TIME(n, m): now (n=0) and the start (n=1): year, month, day, weekday, h, m, s, ms
pub const TIME: u16 = 0;
/// @_OS_TIME(n): milliseconds since the OS started (0) and since the engine started (1)
pub const OS_TIME: u16 = 1;
/// @_OS_SOUND: sound works
pub const OS_SOUND: u16 = 4;
pub const MEDIA_READONLY: u16 = 9;
/// @_PAI: pi (a FLT)
pub const PAI: u16 = 10;
pub const DEBUGMODE: u16 = 11;
/// @_LC: counter of the innermost LOOP (1..)
pub const LC: u16 = 48;
/// @_PINT(n) @_PFLT(n) $_PSTR(n) @_PFLG(n): what GOSUB passed
pub const PINT: u16 = 50;
pub const PFLT: u16 = 51;
pub const PSTR: u16 = 52;
pub const PFLG: u16 = 53;
/// @_RINT(n) @_RFLT(n) $_RSTR(n) @_RFLG(n): what RETURN passed back
pub const RINT: u16 = 54;
pub const RFLT: u16 = 55;
pub const RSTR: u16 = 56;
pub const RFLG: u16 = 57;
pub const PROJECT_PATH: u16 = 72;
pub const SAVE_PATH: u16 = 73;
/// @_DXMODE: Direct3D drawing; 0, so ERIS composes its layers itself (no DXBUF / MATH3D)
pub const DXMODE: u16 = 78;
/// @_SCREEN_COLOR: bits per pixel of the screen
pub const SCREEN_COLOR: u16 = 79;
/// @_WINDOW_EXEC_CLOSE: the window's close button was pressed
pub const WINDOW_EXEC_CLOSE: u16 = 84;
/// $_TEXT / $_TEXT_FLAG: text the scripts gave (WORD) and per character '1' (half width) or
/// '2' (full width), with 'E' where a line ended; the ERIS text task shows it
pub const TEXT: u16 = 87;
pub const TEXT_FLAG: u16 = 88;
/// $_CHR(n): the one-byte string of byte n
pub const CHR: u16 = 92;
/// @_MENUCODE: CODE of the menu item chosen last
pub const MENUCODE: u16 = 97;
pub const MOUSE_X: u16 = 98;
pub const MOUSE_Y: u16 = 103;
/// @_INP(code): the state of the button with that code, as in @_KEY_x
pub const INP: u16 = 109;
/// @_KEYLAST / @_MOUSELAST: a key / mouse button was pressed (scripts clear them)
pub const KEYLAST: u16 = 110;
pub const MOUSELAST: u16 = 111;
/// @_MOUSE_L .. @_KEY_x: frames the button has been held (1, 2, ..) or released (-1, -2, ..);
/// at STATE + code - 1
const STATE: u16 = 112;
/// @_MOUSENUM_x, @_KEYNUM_x: times pressed
const COUNT: u16 = 246;
/// @_CODE_x: the code of each button, at CODE + code - 1
const CODE: u16 = 380;

/// Button codes (the values of @_CODE_x): mouse buttons, then keys A-Z, 0-9, F1-F24, ...
pub mod code {
    pub const MOUSE_L: usize = 1;
    pub const MOUSE_R: usize = 5;
    pub const MOUSE_M: usize = 9;
    pub const MOUSE_U: usize = 13;
    pub const MOUSE_D: usize = 17;
    pub const A: usize = 21;
    pub const N0: usize = 47;
    pub const F1: usize = 57;
    pub const NUM0: usize = 81;
    pub const BS: usize = 96;
    pub const TAB: usize = 97;
    pub const ENTER: usize = 98;
    pub const SPACE: usize = 99;
    pub const SHIFT: usize = 100;
    pub const CTRL: usize = 101;
    pub const ALT: usize = 102;
    pub const LEFT: usize = 103;
    pub const RIGHT: usize = 104;
    pub const UP: usize = 105;
    pub const DOWN: usize = 106;
    pub const ESC: usize = 112;
    pub const PAGEUP: usize = 113;
    pub const PAGEDOWN: usize = 114;
    pub const HOME: usize = 115;
    pub const END: usize = 116;
    /// one past the last key (backslash)
    pub const COUNT: usize = 129;
}

/// Values the engine sets once at the start.
pub fn init(st: &mut State) {
    st.set_sys(OS_SOUND, false, &[], Value::Int(1));
    st.set_sys(PAI, false, &[], Value::Flt(std::f64::consts::PI));
    st.set_sys(SCREEN_COLOR, false, &[], Value::Int(32));
    st.set_sys(DXMODE, false, &[], Value::Int(0));
    for b in 0..256 {
        st.set_sys(CHR, true, &[b], Value::Str(vec![b as u8]));
    }
    for c in 1..code::COUNT {
        st.set_sys(CODE + c as u16 - 1, false, &[], Value::Int(c as i64));
        st.set_sys(INP, false, &[c as i64], Value::Int(-1));
        st.set_sys(STATE + c as u16 - 1, false, &[], Value::Int(-1));
    }
}

/// The buttons held now and the mouse, given to the engine each frame.
#[derive(Debug, Default, Clone)]
pub struct Input {
    pub held: Vec<bool>,
    /// wheel turned up / down since the last frame
    pub wheel: i32,
    pub x: i64,
    pub y: i64,
}

impl Input {
    pub fn new() -> Input {
        Input { held: vec![false; code::COUNT], ..Default::default() }
    }
}

/// Counts frames held and released, before the scripts run each frame.
pub fn update(st: &mut State, input: &Input) {
    st.set_sys(MOUSE_X, false, &[], Value::Int(input.x));
    st.set_sys(MOUSE_Y, false, &[], Value::Int(input.y));
    for c in 1..code::COUNT {
        let held = match c {
            code::MOUSE_U => input.wheel > 0,
            code::MOUSE_D => input.wheel < 0,
            _ => input.held.get(c).copied().unwrap_or(false),
        };
        // Scripts may set @_INP(code) themselves (to make a held button count as new).
        let old = st.get_sys(INP, false, &[c as i64]).int();
        let new = match (held, old > 0) {
            (true, true) => old + 1,
            (true, false) => 1,
            (false, true) => -1,
            (false, false) => old.min(0) - 1,
        };
        if new == 1 {
            let last = if c < code::A { MOUSELAST } else { KEYLAST };
            st.set_sys(last, false, &[], Value::Int(c as i64));
            let n = st.get_sys(COUNT + c as u16 - 1, false, &[]).int();
            st.set_sys(COUNT + c as u16 - 1, false, &[], Value::Int(n + 1));
        }
        st.set_sys(INP, false, &[c as i64], Value::Int(new));
        st.set_sys(STATE + c as u16 - 1, false, &[], Value::Int(new));
    }
}

/// @_TIME and @_OS_TIME. `now` is milliseconds since the start, `date` the local date and time
/// (year, month, day, weekday, h, m, s, ms).
pub fn clock(st: &mut State, now: f64, date: [i64; 8], start: [i64; 8]) {
    st.set_sys(OS_TIME, false, &[0], Value::Int(now as i64));
    st.set_sys(OS_TIME, false, &[1], Value::Int(now as i64));
    for (i, (d, s)) in date.iter().zip(start).enumerate() {
        st.set_sys(TIME, false, &[0, i as i64], Value::Int(*d));
        st.set_sys(TIME, false, &[1, i as i64], Value::Int(s));
    }
}
