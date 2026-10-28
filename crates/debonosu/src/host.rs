//! The program's functions the scripts call (`_layer`, `_effect`, `_page`, ...). The ones a
//! story scene needs do what the game does as far as we know it; the rest (the dungeon, the
//! battles, items, Live2D, 3D scenes, online features) answer with nothing, false or 0.

use luavm::{Lua, LuaError, LuaResult, Value};

use crate::engine::{Block, EF_ATTACH, Shared, Src, Step, TitleItem};

/// Every name the scripts read from the program (from the scripts themselves).
const NAMES: &str = include_str!("names.txt");

fn num(a: &[Value], i: usize) -> f64 {
    a.get(i).and_then(Value::to_num).unwrap_or(0.0)
}
fn num_or(a: &[Value], i: usize, d: f64) -> f64 {
    a.get(i).and_then(Value::to_num).unwrap_or(d)
}
fn text(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => nexas::sjis::decode(s),
        Some(v @ (Value::Int(_) | Value::Num(_))) => {
            v.to_num().map(|n| n.to_string()).unwrap_or_default()
        }
        _ => String::new(),
    }
}
fn key(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::Str(s)) => String::from_utf8_lossy(s).into_owned(),
        Some(v) => v.to_num().map(|n| n.to_string()).unwrap_or_default(),
        None => String::new(),
    }
}

/// `{base/reading}` (a reading over the letters) as the base letters.
fn ruby_base(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) => {
                let inner = &after[..j];
                out.push_str(inner.split('/').next().unwrap_or(inner));
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// What a function we do not have answers.
fn stub(name: &str) -> Vec<Value> {
    let n = name.trim_start_matches('_');
    let bare = n
        .strip_prefix("BTL_")
        .or_else(|| n.strip_prefix("DAT_"))
        .unwrap_or(n);
    if bare.ends_with("Name")
        || bare.ends_with("Text")
        || bare.ends_with("String")
        || bare.ends_with("Path")
    {
        vec![Value::str("")]
    } else if bare.starts_with("Is") || bare.starts_with("Can") || bare.starts_with("Has") {
        vec![Value::Bool(false)]
    } else if bare.starts_with("Get") || bare.starts_with("get") || bare.starts_with("Find") {
        vec![Value::Int(0)]
    } else {
        vec![]
    }
}

pub fn include(lua: &mut Lua, st: &Shared, script: &str) -> LuaResult<()> {
    let b = st.borrow_mut().read(&format!("{script}.scb"));
    let Some(b) = b else {
        return Err(LuaError::new(format!("no script {script}")));
    };
    lua.exec(&b, script)?;
    Ok(())
}

fn reg(
    lua: &mut Lua,
    name: &'static str,
    f: impl Fn(&mut Lua, Vec<Value>) -> LuaResult<Vec<Value>> + 'static,
) {
    lua.set_global(name, Value::native(name, f));
}

/// A function that only needs the state.
fn on(
    st: &Shared,
    lua: &mut Lua,
    name: &'static str,
    f: impl Fn(&mut crate::engine::St, &[Value]) -> Vec<Value> + 'static,
) {
    let st = st.clone();
    reg(lua, name, move |_, a| Ok(f(&mut st.borrow_mut(), &a)));
}

pub fn install(lua: &mut Lua, st: &Shared) {
    for name in NAMES.split_whitespace() {
        let n: &'static str = Box::leak(name.to_string().into_boxed_str());
        let st = st.clone();
        reg(lua, n, move |_, _| {
            let mut s = st.borrow_mut();
            if s.log && s.unknown.insert(n.to_string()) {
                eprintln!("deb: not done: {n}");
            }
            Ok(stub(n))
        });
    }
    lua.set_global("QuietNaN", Value::Num(f64::NAN));
    lua.set_global("adult_pack", Value::Bool(true));
    lua.set_global("nude_pack", Value::Bool(false));
    lua.set_global("nurari_pack", Value::Bool(false));
    lua.set_global("USERICONINF", Value::Nil);
    install_flag32(lua);

    for (n, v) in [
        ("IsTrial", true),
        ("IsDebug", false),
        ("_GetReimeiki", true),
        ("_IsSteam", false),
    ] {
        reg(lua, n, move |_, _| Ok(vec![Value::Bool(v)]));
    }
    for (n, v) in [
        ("_ClientWidth", 1280),
        ("_ClientHeight", 720),
        ("_ClientWidthBase", 1280),
        ("_ClientHeightBase", 720),
        ("_VisualWidth", 1280),
        ("_VisualHeight", 720),
        ("_GetTextFontSizeBase", 26),
        ("_GetTextFontSize", 26),
        ("_GetTextHeightBase", 39),
        ("_GetTextHeight", 39),
        ("_GetTextColumn", 36),
    ] {
        reg(lua, n, move |_, _| Ok(vec![Value::Int(v)]));
    }

    // scripts and coroutines
    {
        let st = st.clone();
        reg(lua, "_include", move |lua, a| {
            let name = key(&a, 0);
            include(lua, &st, &name)?;
            Ok(vec![])
        });
    }
    reg(lua, "coroutine_resume", |lua, a| {
        let Some(Value::Thread(co)) = a.first() else {
            return Ok(vec![Value::Bool(false), Value::str("not a coroutine")]);
        };
        let co = co.clone();
        let args = a[1..].to_vec();
        Ok(match lua.resume(&co, args)? {
            Ok(mut v) => {
                v.insert(0, Value::Bool(true));
                v
            }
            Err(e) => vec![Value::Bool(false), Value::str(&e.to_string())],
        })
    });
    {
        let st = st.clone();
        reg(lua, "_is_exist_label", move |lua, a| {
            let (s, l) = (key(&a, 0), key(&a, 1));
            if lua.global(&format!("_{s}_exist_")).is_nil() && include(lua, &st, &s).is_err() {
                return Ok(vec![Value::Bool(false)]);
            }
            lua.call_global(&format!("_{s}_exist_"), vec![Value::str(&l)])
        });
    }
    on(st, lua, "_is_exist", |s, a| {
        let name = key(a, 0);
        let kind = num_or(a, 1, 1.0) as i32;
        let ok = match kind {
            3 => s.layers.iter().any(|l| l.name == name),
            1 | 2 => {
                let base = name.rsplit(['\\', '/']).next().unwrap_or(&name).to_string();
                s.index.has(&base)
            }
            _ => false,
        };
        vec![Value::Bool(ok)]
    });
    on(st, lua, "_DispDebugMsg", |s, a| {
        if s.log {
            eprintln!("deb: debug: {}", text(a, 0));
        }
        vec![]
    });
    on(st, lua, "_Message", |s, a| {
        if s.log {
            eprintln!("deb: message: {}", text(a, 0));
        }
        vec![]
    });
    on(st, lua, "GetTimer", |s, _| {
        vec![Value::Int((s.now * 1000.0) as i64)]
    });
    on(st, lua, "_random", |s, a| {
        let (lo, hi) = (num(a, 0) as i64, num(a, 1) as i64);
        let r = lo + (s.rand() * ((hi - lo + 1).max(1)) as f64) as i64;
        vec![Value::Int(r.min(hi.max(lo)))]
    });
    on(st, lua, "_frandom", |s, a| {
        let (lo, hi) = (num(a, 0), num(a, 1));
        vec![Value::Num(lo + s.rand() * (hi - lo))]
    });

    // variables and flags
    for (n, float) in [
        ("GetVar", false),
        ("GetVarF", true),
        ("GetGlobalVar", false),
    ] {
        let global = n == "GetGlobalVar";
        on(st, lua, n, move |s, a| {
            let k = key(a, 0);
            let m = if global { &s.globals } else { &s.vars };
            let v = m.get(&k).cloned().unwrap_or(if float {
                Value::Num(0.0)
            } else {
                Value::Int(0)
            });
            vec![v]
        });
    }
    for n in ["SetVar", "SetVarF", "SetGlobalVar", "InitGlobalVar"] {
        let global = n.contains("Global");
        on(st, lua, n, move |s, a| {
            let k = key(a, 0);
            let v = a.get(1).cloned().unwrap_or(Value::Int(0));
            if global {
                s.globals.insert(k, v);
            } else {
                s.vars.insert(k, v);
            }
            vec![]
        });
    }
    on(st, lua, "HasVar", |s, a| {
        vec![Value::Bool(s.vars.contains_key(&key(a, 0)))]
    });
    on(st, lua, "HasGlobalVar", |s, a| {
        vec![Value::Bool(s.globals.contains_key(&key(a, 0)))]
    });
    on(st, lua, "EraseVar", |s, a| {
        s.vars.remove(&key(a, 0));
        vec![]
    });
    on(st, lua, "_InitVar", |s, _| {
        s.vars.clear();
        vec![]
    });
    on(st, lua, "_SetSystemFlag", |s, a| {
        s.flags.insert(
            num(a, 0) as i64,
            a.get(1).cloned().unwrap_or(Value::Bool(false)),
        );
        vec![]
    });
    on(st, lua, "_GetSystemFlag", |s, a| {
        vec![
            s.flags
                .get(&(num(a, 0) as i64))
                .cloned()
                .unwrap_or(Value::Bool(false)),
        ]
    });
    on(st, lua, "_ResetSystemFlag", |s, a| {
        s.flags.remove(&(num(a, 0) as i64));
        vec![]
    });

    // text
    on(st, lua, "_text", |s, a| {
        let t = ruby_base(&text(a, 0));
        if s.log {
            eprintln!("deb: text {t}");
        }
        s.msg.push_str(&t);
        vec![]
    });
    on(st, lua, "_name", |s, a| {
        s.name = text(a, 0);
        vec![]
    });
    on(st, lua, "_page", |s, _| {
        s.block = Block::Page;
        let (name, msg) = (s.name.clone(), s.msg.clone());
        s.backlog.push(&name, &msg);
        vec![]
    });
    on(st, lua, "_GetMsgLength", |s, _| {
        vec![Value::Int(s.msg.chars().count() as i64)]
    });
    {
        let st = st.clone();
        reg(lua, "_select", move |lua, a| {
            let items: Vec<String> = (3..a.len())
                .map(|i| text(&a, i))
                .filter(|t| !t.is_empty())
                .collect();
            let rk = lua.global("Result");
            let mut s = st.borrow_mut();
            s.result_key = match &rk {
                Value::Str(b) => String::from_utf8_lossy(b).into_owned(),
                v => v
                    .to_num()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "Result".into()),
            };
            if !items.is_empty() {
                s.block = Block::Choice(items);
            }
            Ok(vec![])
        });
    }

    // pictures
    on(st, lua, "_layer", |s, a| {
        let (file, mut name) = (key(a, 0), key(a, 1));
        let prio = num_or(a, 6, -1.0) as i32;
        if s.log {
            eprintln!("deb: layer {a:?}");
        }
        if file.is_empty() {
            let c = [
                num(a, 3) as u8,
                num(a, 4) as u8,
                num(a, 5) as u8,
                num_or(a, 2, 255.0) as u8,
            ];
            s.add_layer(&name, Src::Color(c), prio);
        } else {
            if name.is_empty() {
                name = file.clone();
            }
            s.add_layer(&name, Src::File(file), prio);
        }
        vec![]
    });
    on(st, lua, "_capture", |s, a| {
        let name = key(a, 0);
        let prio = num_or(a, 2, -1.0) as i32;
        let img = s.shot();
        s.add_layer(&name, Src::Shot(img), prio);
        vec![]
    });
    on(st, lua, "_clear", |s, a| {
        s.clear(&key(a, 0));
        vec![]
    });
    on(st, lua, "_priority", |s, a| {
        let prio = num(a, 0) as i32;
        let last = s.last.clone();
        if let Some(l) = s.layer_mut(&last) {
            l.prio = prio;
        }
        vec![]
    });
    on(st, lua, "_rename", |s, a| {
        let (from, to) = (key(a, 0), key(a, 1));
        s.layers.retain(|l| l.name != to);
        if let Some(l) = s.layer_mut(&from) {
            l.name = to.clone();
        }
        if s.last == from {
            s.last = to;
        }
        vec![]
    });
    on(st, lua, "_effect", |s, a| {
        let kind = num(a, 0) as i32;
        let mut v = [num(a, 1) as f32, num(a, 2) as f32];
        if kind == crate::engine::EF_SCALE && a.get(2).is_none_or(|x| x.is_nil()) {
            v[1] = v[0];
        }
        let rgba = [
            num(a, 4) as u8,
            num(a, 5) as u8,
            num(a, 6) as u8,
            num(a, 3) as u8,
        ];
        let name = key(a, 7);
        // a time left out comes as 1000: at once
        let t = num_or(a, 8, 1000.0);
        let t = if t >= 1000.0 { 0.0 } else { t };
        if kind == EF_ATTACH {
            s.attach(&name);
        } else {
            s.steps.push(Step {
                kind,
                v,
                rgba,
                name,
                t,
            });
        }
        vec![]
    });
    on(st, lua, "_stand", |s, a| {
        let name = key(a, 0);
        if !s.stand.is_empty() {
            let old = s.stand.clone();
            s.layers.retain(|l| l.name != old);
        }
        s.stand = name.clone();
        if name.is_empty() {
            return vec![];
        }
        let prio = match num(a, 2) as i32 {
            p if p > 0 => p,
            _ => 110,
        };
        s.add_layer(&name, Src::File(name.clone()), prio);
        if let Some(img) = s.image(&name) {
            let (w, h) = (img.w as f32, img.h as f32);
            // a tall picture is fitted to the screen's height, at the bottom middle
            let k = (720.0 / h).min(1.0);
            if let Some(l) = s.layer_mut(&name) {
                l.base = [w / 2.0 / 1280.0, h / 720.0];
                l.off = [0.5, 1.0];
                l.scale = [k, k];
            }
        }
        vec![]
    });
    on(st, lua, "_face", |s, a| {
        let f = key(a, 0);
        s.face = if f == "*" { String::new() } else { f };
        vec![]
    });
    on(st, lua, "_wait", |s, a| {
        let first = a.get(2).is_some_and(Value::truthy);
        // wait(name): that layer's effect; wait(seconds) comes as ("", seconds); wait(): all
        let secs = match a.first() {
            Some(v @ (Value::Int(_) | Value::Num(_))) => v.to_num(),
            Some(Value::Str(b)) if b.is_empty() => {
                a.get(1).and_then(Value::to_num).filter(|&t| t > 0.0)
            }
            _ => None,
        };
        let done = match (secs, a.first()) {
            (Some(t), _) => {
                let k = "#time".to_string();
                if first {
                    let until = s.now + t;
                    s.waits.insert(k.clone(), until);
                }
                s.waits.get(&k).is_none_or(|&u| s.now >= u)
            }
            (None, Some(Value::Str(b))) if !b.is_empty() => {
                let n = String::from_utf8_lossy(b).into_owned();
                !s.layers.iter().any(|l| l.name == n && l.busy())
            }
            _ => !s.layers.iter().any(|l| l.busy()),
        };
        vec![Value::Int(if done { 1 } else { 2 })]
    });
    on(st, lua, "_strict_wait", |s, a| {
        let t = num(a, 0);
        let k = "#strict".to_string();
        if a.get(1).is_some_and(Value::truthy) || !s.waits.contains_key(&k) {
            let until = s.now + t;
            s.waits.insert(k.clone(), until);
        }
        let done = s.waits.get(&k).is_none_or(|&u| s.now >= u);
        if done {
            s.waits.remove(&k);
        }
        vec![Value::Int(if done { 1 } else { 2 })]
    });

    // sound
    on(st, lua, "_playmusic", |s, a| {
        let no = num(a, 0) as i32;
        let vol = (num_or(a, 2, 100.0) / 100.0) as f32;
        s.music(no, vol);
        vec![]
    });
    on(st, lua, "_stopmusic", |s, _| {
        s.stop_sound(0, 500);
        vec![]
    });
    on(st, lua, "_playsound", |s, a| {
        let name = key(a, 0);
        let vol = (num_or(a, 2, 100.0) / 100.0) as f32;
        let ch = num_or(a, 3, -1.0) as i32;
        let looped = num(a, 1) as i32 == 1;
        s.play_sound(&name, if ch >= 0 { 3 + ch } else { 3 }, vol, looped);
        vec![]
    });
    on(st, lua, "_stopsound", |s, a| {
        let ch = num_or(a, 0, -1.0) as i32;
        if ch >= 0 {
            s.stop_sound(3 + ch, 0);
        } else {
            for c in 3..12 {
                s.stop_sound(c, 0);
            }
        }
        vec![]
    });
    on(st, lua, "_fadesound", |s, a| {
        let ch = num_or(a, 0, -1.0) as i32;
        let ms = (num(a, 2) * 1000.0) as u32;
        if num_or(a, 1, 0.0) <= 0.0 {
            s.stop_sound(if ch >= 0 { 3 + ch } else { 0 }, ms);
        }
        vec![]
    });
    on(st, lua, "_playvoice", |s, a| {
        let name = key(a, 0);
        s.play_sound(&name, 1, 1.0, false);
        vec![]
    });
    on(st, lua, "_stopvoice", |s, _| {
        s.stop_sound(1, 0);
        vec![]
    });
    on(st, lua, "_stopbackvoice", |s, _| {
        s.stop_sound(2, 0);
        vec![]
    });
    on(st, lua, "_getvoicevolume", |_, _| vec![Value::Int(100)]);

    // the title menu: tiItemTitle.dat, 252-byte records (the first the background)
    on(st, lua, "_ExecTitle", |s, _| {
        if let Some(b) = s.read("tiItemTitle.dat") {
            s.title_items.clear();
            for (i, r) in b.chunks_exact(252).enumerate() {
                let i32at = |o: usize| i32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                let name = |o: usize| {
                    let e = r[o..o + 32].iter().position(|&c| c == 0).unwrap_or(32);
                    String::from_utf8_lossy(&r[o..o + e]).into_owned()
                };
                if i == 0 {
                    s.title_bg = name(4);
                    continue;
                }
                s.title_items.push(TitleItem {
                    code: i32at(236),
                    normal: name(4),
                    over: name(36),
                    src_y: [i32at(152), i32at(160)],
                    size: [i32at(124), i32at(128)],
                    dst: [i32at(140), i32at(144)],
                });
            }
        }
        s.block = Block::Title;
        vec![]
    });
    on(st, lua, "GetTitlePage", |_, _| vec![Value::Int(1)]);
    on(st, lua, "GetTitleMode", |_, _| vec![Value::Int(0)]);
}

/// flag32.up/down/set/test: bit `n` of a 32-bit value.
fn install_flag32(lua: &mut Lua) {
    let mut t = luavm::Table::default();
    let bits = |a: &[Value]| -> Vec<u32> {
        a[1..]
            .iter()
            .filter_map(|v| v.to_num())
            .map(|n| n as u32 & 31)
            .collect()
    };
    t.set_str(
        "up",
        Value::native("flag32.up", move |_, a| {
            let v = a.first().and_then(Value::to_num).unwrap_or(0.0) as i64 as u32;
            let r = bits(&a).iter().fold(v, |v, b| v | (1 << b));
            Ok(vec![Value::Int(r as i64)])
        }),
    );
    t.set_str(
        "down",
        Value::native("flag32.down", move |_, a| {
            let v = a.first().and_then(Value::to_num).unwrap_or(0.0) as i64 as u32;
            let r = bits(&a).iter().fold(v, |v, b| v & !(1 << b));
            Ok(vec![Value::Int(r as i64)])
        }),
    );
    t.set_str(
        "set",
        Value::native("flag32.set", |_, a| {
            let v = a.first().and_then(Value::to_num).unwrap_or(0.0) as i64 as u32;
            let b = a.get(1).and_then(Value::to_num).unwrap_or(0.0) as u32 & 31;
            let on = a.get(2).is_some_and(Value::truthy);
            let r = if on { v | (1 << b) } else { v & !(1 << b) };
            Ok(vec![Value::Int(r as i64)])
        }),
    );
    t.set_str(
        "test",
        Value::native("flag32.test", move |_, a| {
            let v = a.first().and_then(Value::to_num).unwrap_or(0.0) as i64 as u32;
            Ok(vec![Value::Bool(
                bits(&a).iter().all(|b| v & (1 << b) != 0),
            )])
        }),
    );
    lua.set_global("flag32", Value::table(t));
}

#[cfg(test)]
mod tests {
    #[test]
    fn ruby() {
        assert_eq!(super::ruby_base("神主が{訥々/とつとつ}と"), "神主が訥々と");
        assert_eq!(super::ruby_base("a{b"), "a{b");
    }

    #[test]
    fn wrap() {
        let l = crate::engine::wrap("あいうえお、かきくけこ", 5);
        assert_eq!(l, vec!["あいうえお、", "かきくけこ"]);
    }
}
