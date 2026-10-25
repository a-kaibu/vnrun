//! The engine as the Lua interface sees it: `cclass` (lua_Layers, lua_Sounds, ...), `cfunc`
//! (LegacyGame__lua_*, ...) and the LUA_* constants, as AdvHD.exe registers them with tolua.
//!
//! Class objects are their class table: `cclass.lua_Layers:create()` returns `cclass.lua_Layers`
//! and methods skip their `self`. Calls the engine doesn't know return nothing and are reported
//! once.

use crate::layers::{Effect, Snap, TextBox, SIZE};
use crate::state::State;
use luavm::{Lua, LuaError, Table, TableRef, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub type St = Rc<RefCell<State>>;

/// The constants AdvHD.exe gives Lua (those not listed are 0, as LUA_PARAM_POS is).
const CONSTS: &[(&str, f64)] = &[
    ("LUA_LAYER_PNA", 0.0), ("LUA_LAYER_STD", 1.0),
    ("LUA_PARAM_POS", 0.0), ("LUA_PARAM_ROTATE", 1.0), ("LUA_PARAM_SCALE", 2.0), ("LUA_PARAM_CENTERZ", 3.0),
    ("LUA_PARAM_CENTERR", 4.0), ("LUA_PARAM_OFFSET", 5.0), ("LUA_PARAM_COLOR", 6.0), ("LUA_PARAM_RECT", 7.0),
    ("LUA_PARAM_SIZE", 8.0), ("LUA_PARAM_DISSOLVE_PROGRESS", 9.0), ("LUA_PARAM_MASK_OFFSET", 10.0),
    ("LUA_EFFECT_OPACITY", 0.0), ("LUA_EFFECT_POS", 1.0), ("LUA_EFFECT_SCALE", 2.0), ("LUA_EFFECT_ROTATE", 3.0),
    ("LUA_EFFECT_OFFSET", 4.0), ("LUA_EFFECT_RECT", 5.0), ("LUA_EFFECT_GAZE", 6.0), ("LUA_EFFECT_ANGLE", 7.0),
    ("LUA_EFFECT_HERMITE", 8.0), ("LUA_EFFECT_MASK", 9.0), ("LUA_EFFECT_MASK_OFFSET", 10.0),
    ("LUA_SKIPSPEED_OMIT", 0.33), ("LUA_SKIPSPEED_NONE", 1e-6),
    ("EFFECT_LINEAR", 0.0), ("EFFECT_EASEIN", 1.0), ("EFFECT_EASEOUT", 2.0), ("EFFECT_EASEINOUT", 3.0),
    ("LUA_STATE_MSGSKIP", 0.0), ("LUA_STATE_MSGCTRLSKIP", 1.0), ("LUA_STATE_MSGAUTO", 2.0), ("LUA_STATE_MSGHIDE", 3.0),
    ("LUA_STATE_MSGOPEN", 4.0), ("LUA_STATE_DISABLEDATA", 10.0), ("LUA_STATE_ENABLELOAD", 11.0),
    ("LUA_STATE_ENABLEQLOAD", 12.0), ("LUA_STATE_ENABLESKIP", 13.0), ("LUA_STATE_ENABLEVOICEREPEAT", 14.0),
    ("LUA_STATE_ENABLELOG", 15.0), ("LUA_STATE_ENABLESELECTLOAD", 16.0), ("LUA_STATE_ENABLEMENUSAVE", 17.0),
    ("LUA_STATE_EXTENDGAME", 18.0), ("LUA_STATE_TRANSITION", 20.0),
    ("LUA_PROCESS_VOICEREPEAT", 1.0), ("LUA_PROCESS_QUIT", 2.0), ("LUA_PROCESS_LOG_VOICEPLAY", 3.0),
    ("LUA_PROCESS_GAME_PAUSED", 4.0), ("LUA_PROCESS_GAME_UNPAUSED", 5.0), ("LUA_PROCESS_TITLE", 6.0),
    ("LUA_PROCESS_SAVEGAME", 7.0), ("LUA_PROCESS_LOADGAME", 8.0), ("LUA_PROCESS_QUICKSAVEGAME", 9.0),
    ("LUA_PROCESS_QUICKLOADGAME", 10.0), ("LUA_PROCESS_AUTOSAVEGAME", 11.0), ("LUA_PROCESS_AUTOLOADGAME", 12.0),
    ("LUA_PROCESS_INTERRUPT", 13.0), ("LUA_PROCESS_SCREENMODE", 14.0), ("LUA_PROCESS_ASPECTMODE", 15.0),
    ("LUA_PROCESS_UPDATEPAN", 16.0), ("LUA_PROCESS_UPDATEVOLUME", 17.0), ("LUA_PROCESS_UPDATEBGV", 18.0),
    ("LUA_PROCESS_MINIMIZED", 19.0), ("LUA_PROCESS_UPDATEBGMCUT", 20.0), ("LUA_PROCESS_RTSNAP", 21.0),
    ("LUA_PROCESS_MOUSEMOVE", 22.0), ("LUA_PROCESS_PREVIEW", 23.0), ("LUA_PROCESS_UPDATEMSGSPEED", 24.0),
    ("LUA_PROCESS_BACKGAME", 25.0), ("LUA_PROCESS_SELECTSAVEGAME", 26.0), ("LUA_PROCESS_SELECTLOADGAME", 27.0),
    ("LUA_PROCESS_SAVEPREVIEW", 28.0), ("LUA_PROCESS_SHELL", 29.0), ("LUA_PROCESS_DELETEGAME", 30.0),
    ("LUA_PROCESS_QUICKDELETEGAME", 31.0), ("LUA_PROCESS_LOGPREVIEW", 32.0), ("LUA_PROCESS_GOFILE", 33.0),
    ("LUA_PROCESS_INETEVENT", 34.0), ("LUA_PROCESS_LOADFILE", 35.0), ("LUA_PROCESS_ACHIEVEMENT", 36.0),
    ("LUA_PROCESS_UPDATEDATA", 37.0), ("LUA_PROCESS_CHANGELANGUAGE", 38.0),
    ("LUA_CONFIRM_TITLE", 1.0), ("LUA_CONFIRM_ALBUM", 2.0), ("LUA_CONFIRM_OVERWRITE", 3.0), ("LUA_CONFIRM_LOAD", 4.0),
    ("LUA_CONFIRM_QUICKLOAD", 5.0), ("LUA_CONFIRM_QUIT", 6.0), ("LUA_CONFIRM_DEFAULT", 7.0), ("LUA_CONFIRM_MEMLOAD", 8.0),
    ("LUA_CONFIRM_SELECTLOAD", 9.0), ("LUA_CONFIRM_SAVE", 10.0),
    ("LUA_KEYCODE_PAGEUP", 33.0), ("LUA_KEYCODE_PAGEDOWN", 34.0), ("LUA_KEYCODE_HOME", 36.0), ("LUA_KEYCODE_END", 35.0),
    ("LUA_KEYCODE_UP", 38.0), ("LUA_KEYCODE_DOWN", 40.0), ("LUA_KEYCODE_LEFT", 37.0), ("LUA_KEYCODE_RIGHT", 39.0),
    ("LUA_KEYCODE_ENTER", 13.0), ("LUA_KEYCODE_SPACE", 32.0),
    ("LUA_TRANSITION_NORMAL", 0.0), ("LUA_TRANSITION_ERASE", 1.0),
    ("DATA_MODE_NORMAL", 0.0), ("DATA_MODE_QUICK", 1.0), ("DATA_MODE_AUTO", 2.0), ("DATA_MODE_SELECT", 3.0),
    ("DATA_SORT_UPDATEDATE", 0.0), ("DATA_SORT_CREATEDATE", 1.0), ("DATA_SORT_NUMBER", 2.0),
    ("DATA_ORDER_ASC", 0.0), ("DATA_ORDER_DESC", 1.0),
    ("ANSI_CHARSET", 0.0), ("SYMBOL_CHARSET", 2.0), ("SHIFTJIS_CHARSET", 128.0), ("HANGUL_CHARSET", 129.0),
    ("GB2312_CHARSET", 134.0), ("CHINESEBIG5_CHARSET", 136.0), ("GREEK_CHARSET", 161.0), ("TURKISH_CHARSET", 162.0),
    ("VIETNAMESE_CHARSET", 163.0), ("BALTIC_CHARSET", 186.0), ("RUSSIAN_CHARSET", 204.0), ("EASTEUROPE_CHARSET", 238.0),
];

/// Arguments of a host call.
struct A(Vec<Value>);

impl A {
    fn s(&self, i: usize) -> String {
        self.0.get(i).and_then(|v| v.as_bytes()).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
    }
    fn n(&self, i: usize) -> f64 {
        self.0.get(i).and_then(|v| v.to_num()).unwrap_or(0.0)
    }
    fn f(&self, i: usize) -> f32 {
        self.n(i) as f32
    }
    fn i(&self, i: usize) -> i64 {
        self.n(i) as i64
    }
    fn b(&self, i: usize) -> bool {
        match self.0.get(i) {
            Some(Value::Bool(b)) => *b,
            Some(v) => v.to_num().is_some_and(|n| n != 0.0),
            None => false,
        }
    }
    fn v4(&self, i: usize) -> [f32; 4] {
        [self.f(i), self.f(i + 1), self.f(i + 2), self.f(i + 3)]
    }
}

/// ADVHD_CALLS: list the interface's calls to the host.
fn log_calls() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ADVHD_CALLS").is_some())
}

type HostFn = fn(&mut State, &A) -> Vec<Value>;

fn ok(b: bool) -> Vec<Value> {
    vec![Value::Bool(b)]
}

fn nums(v: [f32; 4]) -> Vec<Value> {
    v.iter().map(|&x| Value::Num(x as f64)).collect()
}

fn table() -> TableRef {
    Rc::new(RefCell::new(Table::default()))
}

/// A table of host functions; `method`: called with `self` first. Missing names are reported.
fn host_table(st: &St, name: &'static str, fns: &[(&'static str, HostFn)], method: bool) -> TableRef {
    let t = table();
    for &(n, f) in fns {
        let st = st.clone();
        let skip = method as usize;
        t.borrow_mut().set_str(
            n,
            Value::native(n, move |lua, args: Vec<Value>| {
                let a = A(args.into_iter().skip(skip).collect());
                let r = f(&mut st.borrow_mut(), &a);
                if log_calls() {
                    let show = |v: &[Value]| v.iter().map(|x| String::from_utf8_lossy(&lua.tostring_raw(x)).into_owned()).collect::<Vec<_>>().join(", ");
                    eprintln!("{name}.{n}({}) -> {}", show(&a.0), show(&r));
                }
                Ok(r)
            }),
        );
    }
    let meta = table();
    let st = st.clone();
    meta.borrow_mut().set_str(
        "__index",
        Value::native("__index", move |_, args: Vec<Value>| {
            let key = A(args).s(1);
            st.borrow_mut().unknown(&format!("{name}.{key}"));
            Ok(vec![Value::native("unknown", |_, _| Ok(vec![]))])
        }),
    );
    t.borrow_mut().meta = Some(meta);
    t
}

fn class(st: &St, name: &'static str, fns: &[(&'static str, HostFn)]) -> TableRef {
    let t = host_table(st, name, fns, true);
    // create / new / new_local return the class table, delete does nothing
    for n in ["create", "new", "new_local", "getInstance"] {
        let me = Rc::downgrade(&t);
        t.borrow_mut().set_str(
            n,
            Value::native(n, move |_, _| Ok(me.upgrade().map(Value::Table).into_iter().collect())),
        );
    }
    t.borrow_mut().set_str("delete", Value::native("delete", |_, _| Ok(vec![])));
    t
}

fn effect(a: &A, at: usize) -> Effect {
    // name, kind, x y z w, ease, time (ms), delay
    Effect::new(&a.s(at), a.i(at + 1) as i32, a.v4(at + 2), a.i(at + 6) as i32, a.n(at + 7), a.n(at + 8))
}

const LAYERS: &[(&str, HostFn)] = &[
    ("add", |s, a| {
        s.screen.add(&a.s(0));
        ok(true)
    }),
    ("remove", |s, a| {
        let n = a.s(0);
        s.screen.sets.retain(|x| x.name != n);
        ok(true)
    }),
    ("insertLayer", |s, a| {
        // set, name, LUA_LAYER_PNA / STD, archive, file
        let l = s.picture_layer(&a.s(1), &a.s(4));
        let found = !matches!(l.content, crate::layers::Content::None);
        s.screen.add(&a.s(0)).insert(l);
        ok(found)
    }),
    ("insertTextLayer", |s, a| {
        // set, name, width, height, margins left top right bottom, line gap
        let b = TextBox {
            w: a.i(2) as i32,
            h: a.i(3) as i32,
            margin: [a.i(4) as i32, a.i(5) as i32, a.i(6) as i32, a.i(7) as i32],
            line_gap: a.f(8),
            image: None,
        };
        s.text_layer(&a.s(0), &a.s(1), b);
        ok(true)
    }),
    ("setVisible", |s, a| {
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return ok(false) };
        l.visible = a.b(2);
        ok(true)
    }),
    ("isVisible", |s, a| ok(s.screen.layer(&a.s(0), &a.s(1)).is_some_and(|l| l.visible))),
    ("isAvailable", |s, a| ok(s.screen.layer(&a.s(0), &a.s(1)).is_some())),
    ("initSubLayer", |s, a| {
        let Some(sub) = s.screen.layer(&a.s(0), &a.s(1)).and_then(|l| l.sub(a.i(2) as i32)) else { return ok(false) };
        sub.visible = a.b(3);
        ok(true)
    }),
    ("setSubLayerVisible", |s, a| {
        let Some(sub) = s.screen.layer(&a.s(0), &a.s(1)).and_then(|l| l.sub(a.i(2) as i32)) else { return ok(false) };
        sub.visible = a.b(3);
        ok(true)
    }),
    ("setParam", |s, a| {
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return ok(false) };
        l.p.set(a.i(2) as i32, a.v4(3));
        ok(true)
    }),
    ("getParam", |s, a| {
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return nums([0.0; 4]) };
        let k = a.i(2) as i32;
        if k == SIZE {
            let (w, h) = l.size();
            return nums([w, h, 0.0, 0.0]);
        }
        nums(l.p.get(k))
    }),
    ("setSubLayerParam", |s, a| {
        let Some(sub) = s.screen.layer(&a.s(0), &a.s(1)).and_then(|l| l.sub(a.i(2) as i32)) else { return ok(false) };
        sub.p.set(a.i(3) as i32, a.v4(4));
        ok(true)
    }),
    ("getSubLayerParam", |s, a| {
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return nums([0.0; 4]) };
        let (id, k) = (a.i(2) as i32, a.i(3) as i32);
        if k == SIZE {
            if let crate::layers::Content::Pna(p) = &l.content
                && let Some(part) = p.part(id)
            {
                return nums([part.w as f32, part.h as f32, 0.0, 0.0]);
            }
            return nums([0.0; 4]);
        }
        nums(l.sub(id).map_or([0.0; 4], |sub| sub.p.get(k)))
    }),
    ("setEffect", |s, a| {
        let e = effect(a, 2);
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return ok(false) };
        l.effects.push(e);
        ok(true)
    }),
    ("startEffect", |s, a| {
        let now = s.now;
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return ok(false) };
        l.start_effect(&a.s(2), now);
        ok(true)
    }),
    ("setSubLayerEffect", |s, a| {
        // set, layer, name, sub id, kind, x y z w, ease, time, delay
        let e = Effect::new(&a.s(2), a.i(4) as i32, a.v4(5), a.i(9) as i32, a.n(10), a.n(11));
        let Some(sub) = s.screen.layer(&a.s(0), &a.s(1)).and_then(|l| l.sub(a.i(3) as i32)) else { return ok(false) };
        sub.effects.push(e);
        ok(true)
    }),
    ("startSubLayerEffect", |s, a| {
        let (now, name) = (s.now, a.s(2));
        let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) else { return ok(false) };
        for sub in &mut l.subs {
            for e in sub.effects.iter_mut().filter(|e| e.name == name && e.start.is_none()) {
                e.start = Some(now);
            }
        }
        ok(true)
    }),
    ("isEffect", |s, a| ok(s.screen.layer(&a.s(0), &a.s(1)).is_some_and(|l| l.busy()))),
    ("cancelEffect", |s, a| {
        if let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) {
            l.finish_effects(&a.s(2));
        }
        ok(true)
    }),
    ("cancelSubLayerEffect", |s, a| {
        if let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) {
            l.finish_effects(&a.s(3));
        }
        ok(true)
    }),
    ("isTransition", |s, a| ok(s.screen.set(&a.s(0)).is_some_and(|x| x.fading()))),
    ("setSubLayerHitTest", |s, a| {
        let Some(sub) = s.screen.layer(&a.s(0), &a.s(1)).and_then(|l| l.sub(a.i(2) as i32)) else { return ok(false) };
        sub.hit = a.b(3);
        ok(true)
    }),
    ("setHitTest", |s, a| {
        if let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) {
            l.hit = a.b(2);
        }
        vec![]
    }),
    ("clearHitTestAll", |s, a| {
        if let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) {
            l.hit = false;
            l.subs.iter_mut().for_each(|x| x.hit = false);
        }
        vec![]
    }),
    ("removeLayer", |s, a| {
        if let Some(set) = s.screen.set(&a.s(0)) {
            set.remove(&a.s(1));
        }
        ok(true)
    }),
    ("removeSubLayer", |s, a| {
        if let Some(sub) = s.screen.layer(&a.s(0), &a.s(1)).and_then(|l| l.sub(a.i(2) as i32)) {
            sub.visible = false;
        }
        ok(true)
    }),
    ("getCursorPos", |s, a| {
        let n = s.screen.set(&a.s(0)).and_then(|x| x.hit(a.f(1), a.f(2))).map_or(0, |h| h.0);
        vec![Value::Int(n as i64)]
    }),
    ("setText", |s, a| {
        s.set_text(&a.s(0), &a.s(1), &a.s(2), None);
        ok(true)
    }),
    ("AddText", |s, a| {
        let key = (a.s(0), a.s(1));
        let old: String = s.texts.get(&key).map(|t| t.chars.iter().collect()).unwrap_or_default();
        s.set_text(&key.0, &key.1, &(old + &a.s(2)), None);
        ok(true)
    }),
    ("clearText", |s, a| {
        s.set_text(&a.s(0), &a.s(1), "", None);
        ok(true)
    }),
    ("getString", |s, a| {
        let t: Option<String> = s.texts.get(&(a.s(0), a.s(1))).map(|t| t.chars.iter().collect());
        vec![t.map_or(Value::Nil, |t| Value::str(&t))]
    }),
    ("messageLength", |s, a| {
        vec![Value::Int(s.texts.get(&(a.s(0), a.s(1))).map_or(0, |t| t.chars.len()) as i64)]
    }),
    ("setSnap", |s, a| snap(s, &a.s(0))),
    ("setSnap2", |s, a| snap(s, &a.s(0))),
    ("rtSnap", |s, a| snap(s, &a.s(0))),
    ("copySnap", |s, a| {
        let img = s.screen.set(&a.s(1)).and_then(|x| x.snap.as_ref().map(|p| p.image.clone()));
        if let Some(image) = img {
            s.screen.add(&a.s(0)).snap = Some(Snap { image, fade: None });
        }
        vec![]
    }),
    ("removeSnap", |s, a| {
        if let Some(x) = s.screen.set(&a.s(0)) {
            x.snap = None;
        }
        vec![]
    }),
    ("startTransition", |s, a| {
        // set, from, to, time (ms), ?, LUA_TRANSITION_NORMAL / ERASE
        let (now, len) = (s.now, a.n(3).max(0.0));
        if let Some(x) = s.screen.set(&a.s(0)) {
            match &mut x.snap {
                Some(p) if a.i(5) == 0 => p.fade = Some((now, len)),
                _ => x.until = now + len.max(1.0),
            }
        }
        vec![]
    }),
    ("swapLayerOrder", |s, a| {
        if let Some(set) = s.screen.set(&a.s(0)) {
            let find = |n: &str| set.layers.iter().position(|l| l.name.eq_ignore_ascii_case(n));
            if let (Some(i), Some(j)) = (find(&a.s(1)), find(&a.s(2))) {
                set.layers.swap(i, j);
                return ok(true);
            }
        }
        ok(false)
    }),
    ("forwardLayerOrder", |s, a| {
        // puts layer 1 just above layer 2
        if let Some(set) = s.screen.set(&a.s(0)) {
            let find = |set: &crate::layers::Set, n: &str| set.layers.iter().position(|l| l.name.eq_ignore_ascii_case(n));
            if let Some(i) = find(set, &a.s(1)) {
                let l = set.layers.remove(i);
                let at = find(set, &a.s(2)).map_or(set.layers.len(), |j| j + 1);
                set.layers.insert(at, l);
                return ok(true);
            }
        }
        ok(false)
    }),
    ("setParent", |s, a| {
        if let Some(l) = s.screen.layer(&a.s(0), &a.s(1)) {
            let p = a.s(2);
            l.parent = (!p.is_empty()).then_some(p);
        }
        vec![]
    }),
    ("getScreenAspect", |s, _| {
        let (w, h) = (s.screen.w as f32, s.screen.h as f32);
        nums([0.0, 0.0, w, h])
    }),
    ("setForceBG", |_, _| vec![]),
    ("setOverlaySet", |_, _| vec![]),
    ("setInputID", |_, _| vec![]),
    ("setCameraLayer", |_, _| ok(true)),
    ("setBlurCount", |_, _| ok(true)),
    ("setAnimation", |_, _| ok(false)),
    ("startAnimation", |_, _| ok(false)),
    ("stopAnimation", |_, _| ok(true)),
    ("getAnimationState", |_, _| ok(false)),
    ("updateTextColor", |_, _| ok(true)),
    ("initPNALayoutMode", |_, _| vec![]),
    ("isMosaic", |_, _| ok(false)),
];

/// Freezes a set as the screen shows it up to that set.
fn snap(s: &mut State, set: &str) -> Vec<Value> {
    let image = Rc::new(s.screen.render(s.now, Some(set)));
    s.screen.add(set).snap = Some(Snap { image, fade: None });
    vec![]
}

const SOUNDS: &[(&str, HostFn)] = &[
    ("SndPlay", |_, _| ok(true)),
    ("SndStop", |_, _| vec![]),
    ("BgmPlay", |_, _| ok(true)),
    ("BgmStop", |_, _| vec![]),
    ("IsPlaying", |_, _| ok(false)),
];

const CURSOR: &[(&str, HostFn)] = &[
    ("AutoCursor", |_, _| vec![]),
    ("RestoreCursor", |_, _| vec![]),
    ("GetCursorPos", |s, _| {
        vec![Value::Num(s.cursor.0 as f64), Value::Num(s.cursor.1 as f64)]
    }),
    ("SetCursor", |_, _| vec![]),
    ("IsMove", |_, _| ok(false)),
];

const REGISTRY: &[(&str, HostFn)] = &[
    ("IsKeyDWORD", |s, a| ok(s.registry.contains_key(&a.s(0)))),
    ("GetDWORD", |s, a| vec![Value::Int(s.registry.get(&a.s(0)).copied().unwrap_or(0))]),
    ("SetDWORD", |s, a| {
        s.registry.insert(a.s(0), a.i(1));
        ok(true)
    }),
    ("IsKeyString", |_, _| ok(false)),
    ("GetString", |_, _| vec![Value::str("")]),
    ("SetString", |_, _| ok(true)),
    ("Delete", |_, _| ok(true)),
];

const MIXER: &[(&str, HostFn)] = &[
    ("getMasterVolume", |_, _| vec![Value::Int(100)]),
    ("setMasterVolume", |_, _| vec![]),
    ("updownMasterVolume", |_, _| vec![]),
    ("getMasterMute", |_, _| ok(false)),
    ("setMasterMute", |_, _| vec![]),
    ("reverseMasterMute", |_, _| vec![]),
];

const MEASURE: &[(&str, HostFn)] = &[("diff", |s, _| vec![Value::Num(s.now)])];

const FONT: &[(&str, HostFn)] = &[("getFontID", |_, _| vec![Value::Int(0)])];

const CFUNC: &[(&str, HostFn)] = &[
    ("LegacyGame__lua_GetFlag", |s, a| ok(s.flag(a.i(0)))),
    ("LegacyGame__lua_SetFlag", |s, a| {
        s.set_flag(a.i(0), a.b(1));
        vec![]
    }),
    ("LegacyGame__lua_GetVariable", |s, a| vec![Value::Num(s.var(a.i(0)) as f64)]),
    ("LegacyGame__lua_SetVariable", |s, a| {
        s.set_var(a.i(0), a.f(1));
        vec![]
    }),
    ("LegacyGame__lua_GetString", |s, a| vec![Value::str(s.strings.get(&a.i(0)).map_or("", |x| x))]),
    ("LegacyGame__lua_SetString", |s, a| {
        s.strings.insert(a.i(0), a.s(1));
        vec![]
    }),
    ("LegacyGame__lua_GetGameState", |s, a| vec![Value::Num(s.game_state.get(&a.i(0)).copied().unwrap_or(0.0))]),
    ("LegacyGame__lua_SetGameState", |s, a| {
        s.game_state.insert(a.i(0), a.n(1));
        vec![]
    }),
    ("LegacyGame__lua_NeedProcess", |s, a| {
        match a.i(0) {
            4 => s.paused = true,
            5 => s.paused = false,
            k => s.requests.push((k, a.i(1))),
        }
        vec![]
    }),
    ("LegacyGame__lua_SelectItem", |s, a| {
        s.requests.push((-1, a.i(0)));
        vec![]
    }),
    ("LegacyGame__lua_NeedInterrupt", |_, _| vec![]),
    ("LegacyGame__lua_NeedPreview", |_, _| vec![]),
    ("LegacyGame__lua_NeedShell", |_, _| vec![]),
    ("LegacyGame__lua_NeedVolumeUpdate", |_, _| vec![]),
    ("LegacyGame__lua_SetLoadProc", |_, _| vec![]),
    ("LegacyGame__lua_SetSaveComment", |_, _| vec![]),
    ("LegacyGame__lua_GetTipsFlag", |_, _| ok(false)),
    ("LegacyGame__lua_GetChapter", |_, _| vec![Value::str("")]),
    ("CSaveDataManager__lua_IsData", |_, _| ok(false)),
    ("CSaveDataManager__lua_GetEnabled", |_, _| ok(true)),
    ("CSaveDataManager__lua_FindNewData", |_, _| vec![Value::Int(-1)]),
    ("CSaveDataManager__lua_GetEmptyNo", |_, _| vec![Value::Int(0)]),
    ("CSaveDataManager__lua_GetSaveDataDate", |_, _| vec![Value::str("")]),
    ("CSaveDataManager__lua_GetSaveDataText", |_, _| vec![Value::str("")]),
    ("CSaveDataManager__lua_GetSaveDataChapter", |_, _| vec![Value::str("")]),
    ("CSaveDataManager__lua_GetSavePath", |_, _| vec![Value::str("")]),
    ("CSaveDataManager__lua_GetDataPath", |_, _| vec![Value::str("")]),
    ("CSaveDataManager__lua_GetThumbnailPath", |_, _| vec![Value::str("")]),
    ("D3DApp__lua_PostMouseMove", |_, _| vec![]),
    ("MySystem__lua_IsJapaneseUI", |_, _| ok(true)),
    ("MyConv__lua_utf8tosjis", |_, a| vec![a.0.first().cloned().unwrap_or_default()]),
    ("MyConv__lua_sjistoutf8", |_, a| vec![a.0.first().cloned().unwrap_or_default()]),
];

/// Gives Lua the host's classes, functions and constants.
pub fn install(lua: &mut Lua, st: &St) {
    for &(n, v) in CONSTS {
        lua.set_global(n, v);
    }
    let cclass = table();
    for (name, fns) in [
        ("lua_Layers", LAYERS),
        ("lua_Sounds", SOUNDS),
        ("lua_AutoCursor", CURSOR),
        ("lua_Registry", REGISTRY),
        ("lua_Mixer", MIXER),
        ("lua_MeasureTime", MEASURE),
        ("CLegacyFontInfo", FONT),
        ("lua_DataSort", &[]),
        ("lua_VoiceFavorite", &[]),
        ("lua_VoiceFavoriteData", &[]),
        ("lua_utf8split", &[]),
    ] {
        cclass.borrow_mut().set_str(name, Value::Table(class(st, name, fns)));
    }
    lua.set_global("cclass", Value::Table(cclass));
    lua.set_global("cfunc", Value::Table(host_table(st, "cfunc", CFUNC, false)));
    crate::io::install(lua, st);
    // "include" loads another script of Script.arc
    let st2 = st.clone();
    lua.set_global(
        "include",
        Value::native("include", move |lua, args| {
            let name = A(args).s(0);
            let file = if name.contains('.') { name.clone() } else { format!("{name}.lua") };
            let chunk = st2.borrow_mut().read(&file);
            match chunk {
                Some(c) => lua.exec(&c, &file),
                None => Err(LuaError::new(format!("no {file}"))),
            }
        }),
    );
}
