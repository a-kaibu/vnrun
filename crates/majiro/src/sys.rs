//! The syscalls ("$name@MAJIRO_INTER"): what the engine itself does for the scripts. Arguments
//! arrive first argument first. The meanings come from how the scripts use them.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexas::gfx::Image;

use crate::engine::{Engine, Input, SoundEvent, syscall_hash};
use crate::gfx::{Anim, Page, Sprite};
use crate::value::{Array, Value};
use crate::vm::{Ret, Thread, Wait};

include!("names.rs");

/// A cfg file opened by `$file_open`, read token by token.
pub struct Reader {
    lines: Vec<String>,
    line: usize,
    tokens: Vec<String>,
}

impl Reader {
    fn new(text: &str) -> Reader {
        Reader {
            lines: text.lines().map(str::to_string).collect(),
            line: 0,
            tokens: Vec::new(),
        }
    }

    /// The next comma separated item, comments and blank lines skipped.
    fn token(&mut self) -> Option<String> {
        while self.tokens.is_empty() {
            let l = self.lines.get(self.line)?.clone();
            self.line += 1;
            let l = match l.find("//") {
                Some(i) => &l[..i],
                None => &l[..],
            };
            let toks: Vec<String> = l
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            self.tokens = toks.into_iter().rev().collect();
        }
        self.tokens.pop()
    }

    fn line(&mut self) -> Option<String> {
        self.tokens.clear();
        loop {
            let l = self.lines.get(self.line)?.clone();
            self.line += 1;
            let t = match l.find("//") {
                Some(i) => l[..i].trim().to_string(),
                None => l.trim().to_string(),
            };
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Area {
    pub n: i32,
    pub rect: [i32; 4],
    pub on: u32,
    pub off: u32,
    pub push: u32,
    pub other: u32,
}

#[derive(Default)]
pub struct State {
    pub names: HashMap<u32, &'static str>,
    pub pics: HashMap<String, Rc<Image>>,
    pub killed: HashSet<usize>,
    /// the click or key of this frame not yet taken (0 left/OK, 1 right/cancel, 2..: keys)
    pub clicked: Option<usize>,
    /// a click seen by the areas (left 0 / right 1)
    pub area_click: Option<usize>,
    pub sound_end: [f64; 16],
    pub files: HashMap<i32, Reader>,
    pub next_file: i32,
    pub effect_speed: i32,
    pub moji_speed: i32,
    pub auto_speed: i32,
    pub fast: bool,
    pub areas: Vec<Area>,
    pub area_default: (u32, u32),
    pub hovered: Option<usize>,
    /// callbacks `$areaevent_check` still has to run
    pub area_queue: Vec<(u32, Vec<Value>)>,
    pub fontout: Fontout,
    pub console: Console,
    pub rand: u32,
    pub frame_mouse: (i32, i32),
    pub libs: HashSet<String>,
}

pub use crate::console::{Console, Fontout};

impl State {
    pub fn new(env: &HashMap<String, String>) -> State {
        let mut s = State {
            effect_speed: 1000,
            moji_speed: 1000,
            auto_speed: 1000,
            rand: 0x1234_5678,
            next_file: 1,
            ..Default::default()
        };
        for n in NAMES {
            s.names.insert(syscall_hash(n), n);
        }
        let num = |k: &str, d: i32| env.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
        s.effect_speed = num("EFFECTSPEED", 1000);
        s.moji_speed = num("MOJISPEED", 1000);
        s.auto_speed = num("AUTOSPEED", 1000);
        let h = num("FC_HEIGHT", 24);
        s.console.font.yl = h;
        s.console.font.xl = h / 2;
        s.console.font.step = h * 3 / 2;
        s.fontout.font = s.console.font;
        s
    }

    pub fn frame_input(&mut self, input: &[Input], mouse: (i32, i32)) {
        self.frame_mouse = mouse;
        for ev in input {
            match ev {
                Input::Click(b, ..) => {
                    self.clicked = Some(*b);
                    self.area_click = Some(*b);
                }
                Input::Key(4) => {
                    self.clicked = Some(0);
                    self.area_click = Some(0);
                }
                Input::Key(5) => {
                    self.clicked = Some(1);
                    self.area_click = Some(1);
                }
                Input::Key(k) => self.clicked = Some(*k + 10),
                Input::Wheel(_) => {}
            }
        }
    }

    pub fn take_click(&mut self) -> Option<usize> {
        self.clicked.take()
    }

    pub fn skipping(&self) -> bool {
        false
    }

    fn rand(&mut self) -> u32 {
        self.rand ^= self.rand << 13;
        self.rand ^= self.rand >> 17;
        self.rand ^= self.rand << 5;
        self.rand
    }
}

/// The frame of a sprite's animation showing at `now`.
pub fn anim_frame(s: &Sprite, now: f64) -> Option<&(i32, i32, f64)> {
    if s.frames.is_empty() {
        return None;
    }
    let total: f64 = s.frames.iter().map(|f| f.2.max(1.0)).sum();
    let mut t = now - s.frame_start;
    if s.frame_loop {
        t %= total;
    }
    for f in &s.frames {
        if t < f.2.max(1.0) {
            return Some(f);
        }
        t -= f.2.max(1.0);
    }
    s.frames.last()
}

pub fn read_vars(b: &[u8], out: &mut HashMap<u32, Value>) {
    let mut p = 0;
    let u32_ = |p: &mut usize| -> Option<u32> {
        let v = u32::from_le_bytes(b.get(*p..*p + 4)?.try_into().ok()?);
        *p += 4;
        Some(v)
    };
    while let (Some(h), Some(t)) = (u32_(&mut p), u32_(&mut p)) {
        let v = match t {
            0 => Value::Int(u32_(&mut p).unwrap_or(0) as i32),
            1 => Value::Float(f32::from_bits(u32_(&mut p).unwrap_or(0))),
            _ => {
                let n = u32_(&mut p).unwrap_or(0) as usize;
                let s = b.get(p..p + n).unwrap_or_default();
                p += n;
                Value::string(String::from_utf8_lossy(s))
            }
        };
        out.insert(h, v);
    }
}

pub fn write_vars(vars: &HashMap<u32, Value>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut keys: Vec<_> = vars.keys().copied().collect();
    keys.sort();
    for k in keys {
        match &vars[&k] {
            Value::Int(i) => {
                out.extend(k.to_le_bytes());
                out.extend(0u32.to_le_bytes());
                out.extend(i.to_le_bytes());
            }
            Value::Float(x) => {
                out.extend(k.to_le_bytes());
                out.extend(1u32.to_le_bytes());
                out.extend(x.to_bits().to_le_bytes());
            }
            Value::Str(s) => {
                out.extend(k.to_le_bytes());
                out.extend(2u32.to_le_bytes());
                out.extend((s.len() as u32).to_le_bytes());
                out.extend(s.as_bytes());
            }
            Value::Arr(_) => {}
        }
    }
    out
}

fn color(c: u32) -> [u8; 4] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]
}

/// Alpha planes of pages are pages too: `$get_alfapage` hands out this bit set on the page id.
pub const ALPHA: i32 = 0x4000_0000;

/// A run of message text: a name ("【name】" first on a page) or letters for the console.
pub fn text(e: &mut Engine, th: &mut Thread, s: &str) -> Result<(), String> {
    let mut events: Vec<(&str, Vec<Value>)> = Vec::new();
    if !e.sys.console.on {
        e.sys.console.on = true;
        events.push(("CONSOLE_ON", vec![Value::Int(0), Value::Int(0)]));
    }
    if e.sys.console.text.is_empty() && s.starts_with('【') && s.contains('】') {
        e.sys.console.name = s.to_string();
        events.push(("NAME_DISP", vec![Value::string(s)]));
    } else {
        let page = e.sys.console.page;
        let yl = e.sys.console.font.yl;
        for (x, y, w) in crate::console::write(e, s) {
            let args = [page, x, y, w, yl, 0].map(Value::Int).to_vec();
            events.push(("CONSOLE_WROTE", args));
        }
    }
    fire(e, th, &events)
}

pub fn proc_text(_e: &mut Engine, _th: &mut Thread) {}

/// Runs the handlers of `events` on `th` (in that order, each event's in the order they were
/// hooked), or posts them to the threads that hooked them.
pub fn fire(e: &mut Engine, th: &mut Thread, events: &[(&str, Vec<Value>)]) -> Result<(), String> {
    for (f, args) in hooked(e, th, events).into_iter().rev() {
        th.call(e, f, args, crate::vm::Cont::Drop)?;
    }
    Ok(())
}

/// The handlers of `events` for `th`, in the order they run; those of other threads are posted.
fn hooked(e: &mut Engine, th: &Thread, events: &[(&str, Vec<Value>)]) -> Vec<(u32, Vec<Value>)> {
    let mut here = Vec::new();
    for (name, args) in events {
        if e.log > 1 {
            eprintln!("majiro: event {name} {args:?}");
        }
        let hooks: Vec<_> = e.hooks.iter().filter(|h| *h.0 == **name).cloned().collect();
        for (_, tid, f) in hooks {
            if tid == th.id {
                here.push((f, args.clone()));
            } else if !e.post_call(tid, f, args.clone()) {
                e.hooks.retain(|h| h.1 != tid);
            }
        }
    }
    here
}

/// `Ret::Calls` runs its calls last first.
fn calls(mut v: Vec<(u32, Vec<Value>)>) -> Result<Ret, String> {
    v.reverse();
    Ok(Ret::Calls(v))
}

fn has_hook(e: &Engine, name: &str) -> bool {
    e.hooks.iter().any(|h| *h.0 == *name)
}

pub fn ctrl(e: &mut Engine, th: &mut Thread, s: &str) -> Result<Option<Wait>, String> {
    match s {
        "p" => {
            if e.log > 1 {
                eprintln!("[{}] {}", e.sys.console.name, e.sys.console.text);
            }
            // console.mjo waits for the click itself
            if has_hook(e, "PAUSE") {
                fire(e, th, &[("PAUSE", Vec::new())])?;
                return Ok(None);
            }
            e.sys.clicked = None;
            Ok(Some(Wait::Page))
        }
        "w" => {
            cls(e);
            fire(e, th, &[("CONSOLE_CLS", Vec::new())])?;
            Ok(None)
        }
        "n" => {
            e.sys.console.newline();
            e.sys.console.text.push('\n');
            fire(e, th, &[("CRLF", Vec::new())])?;
            Ok(None)
        }
        _ => {
            e.warn(format!("ctrl {s:?}"));
            Ok(None)
        }
    }
}

/// A new page of text: the cursor goes home and the name is forgotten.
fn cls(e: &mut Engine) {
    let c = &mut e.sys.console;
    c.last = std::mem::take(&mut c.text);
    c.name.clear();
    c.home();
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}
fn int(a: &[Value], i: usize) -> i32 {
    a.get(i).map_or(0, Value::int)
}
fn float(a: &[Value], i: usize) -> f32 {
    a.get(i).map_or(0.0, Value::float)
}
fn string(a: &[Value], i: usize) -> Rc<str> {
    a.get(i).map_or_else(|| "".into(), Value::str)
}

fn ok(i: i32) -> Result<Ret, String> {
    Ok(Ret::Val(Value::Int(i)))
}

impl Engine {
    /// A page by id, or the page whose alpha plane the id names.
    fn page_img(&mut self, id: i32) -> Option<(&mut Image, bool)> {
        let alpha = id & ALPHA != 0;
        let p = self.gfx.pages.get_mut(&(id & !ALPHA))?;
        if alpha {
            p.alpha = true;
        }
        Some((p.px(), alpha))
    }

    fn page_size(&self, id: i32) -> (i32, i32) {
        self.gfx
            .pages
            .get(&(id & !ALPHA))
            .map_or((0, 0), |p| (p.w(), p.h()))
    }

    /// Copies a rectangle between pages (or alpha planes); `blend` (an opacity, 255 for all of
    /// it) lays the source over by its alpha instead of replacing.
    ///
    /// Majiro's alpha values are transparencies (255: not seen). Pages here keep ordinary
    /// opacity in their own alpha; an alpha plane (`ALPHA` bit) reads and writes it as
    /// transparency, and a page's antidata page holds its transparency in its colour.
    #[allow(clippy::too_many_arguments)]
    pub fn blit(
        &mut self,
        dst: i32,
        dx: i32,
        dy: i32,
        w: i32,
        h: i32,
        src: i32,
        sx: i32,
        sy: i32,
        blend: Option<u32>,
    ) {
        let Some(sp) = self.gfx.pages.get(&(src & !ALPHA)) else {
            return;
        };
        let simg = sp.img.clone();
        let salpha = src & ALPHA != 0;
        let src_has_alpha = sp.alpha;
        let santi = sp
            .anti
            .and_then(|a| self.gfx.pages.get(&a))
            .map(|p| p.img.clone());
        let santi = santi.filter(|a| a.w == simg.w && a.h == simg.h);
        // the destination's antidata page takes the transparency along
        let danti = if dst & ALPHA == 0 {
            self.gfx.pages.get(&dst).and_then(|p| p.anti)
        } else {
            None
        };
        let (dw0, dh0) = self.page_size(dst);
        // taken out of the map while both are written
        let mut anti_page = danti
            .filter(|a| *a != dst)
            .and_then(|a| self.gfx.pages.remove(&a).map(|p| (a, p)));
        let mut anti = anti_page
            .as_mut()
            .filter(|(_, p)| p.w() == dw0 && p.h() == dh0)
            .map(|(_, p)| p.px());
        let Some((d, dalpha)) = self.page_img(dst) else {
            if let Some((a, p)) = anti_page {
                self.gfx.pages.insert(a, p);
            }
            return;
        };
        let (sw, sh, dw, dh) = (simg.w as i32, simg.h as i32, d.w as i32, d.h as i32);
        for y in 0..h {
            let (syy, dyy) = (sy + y, dy + y);
            if syy < 0 || syy >= sh || dyy < 0 || dyy >= dh {
                continue;
            }
            for x in 0..w {
                let (sxx, dxx) = (sx + x, dx + x);
                if sxx < 0 || sxx >= sw || dxx < 0 || dxx >= dw {
                    continue;
                }
                let si = ((syy * sw + sxx) * 4) as usize;
                let di = ((dyy * dw + dxx) * 4) as usize;
                let s = &simg.px[si..si + 4];
                // the source as colour and opacity
                let s: [u8; 4] = if salpha {
                    let t = 255 - s[3];
                    [t, t, t, 255]
                } else if let Some(a) = &santi {
                    [s[0], s[1], s[2], 255 - a.px[si + 2]]
                } else {
                    [s[0], s[1], s[2], if src_has_alpha { s[3] } else { 255 }]
                };
                if dalpha {
                    // the colour (its blue, the low byte of a Majiro colour) is a transparency
                    let v = 255 - s[2] as u32;
                    let o = blend.unwrap_or(255);
                    d.px[di + 3] = ((v * o + d.px[di + 3] as u32 * (255 - o)) / 255) as u8;
                    continue;
                }
                if let Some(o) = blend
                    && (s[3] < 255 || o < 255)
                {
                    let a = s[3] as u32 * o / 255;
                    for (dc, &sc) in d.px[di..di + 3].iter_mut().zip(&s[..3]) {
                        *dc = ((sc as u32 * a + *dc as u32 * (255 - a)) / 255) as u8;
                    }
                    d.px[di + 3] = (a + d.px[di + 3] as u32 * (255 - a) / 255) as u8;
                    if let Some(t) = anti.as_deref_mut() {
                        let v = (t.px[di + 2] as u32 * (255 - a) / 255) as u8;
                        t.px[di..di + 3].copy_from_slice(&[v, v, v]);
                    }
                } else {
                    d.px[di..di + 4].copy_from_slice(&s);
                    if let Some(t) = anti.as_deref_mut() {
                        let v = 255 - s[3];
                        t.px[di..di + 3].copy_from_slice(&[v, v, v]);
                    }
                }
            }
        }
        if let Some((a, p)) = anti_page {
            self.gfx.pages.insert(a, p);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn boxfill(&mut self, page: i32, x: i32, y: i32, w: i32, h: i32, c: u32, mul: bool) {
        let Some((d, alpha)) = self.page_img(page) else {
            return;
        };
        let (dw, dh) = (d.w as i32, d.h as i32);
        let rgba = color(c);
        for yy in y.max(0)..(y.saturating_add(h)).min(dh) {
            for xx in x.max(0)..(x.saturating_add(w)).min(dw) {
                let i = ((yy * dw + xx) * 4) as usize;
                if alpha {
                    // a transparency
                    d.px[i + 3] = 255 - (c & 0xff) as u8;
                } else if mul {
                    for (dc, &m) in d.px[i..i + 3].iter_mut().zip(&rgba[..3]) {
                        *dc = (*dc as u32 * m as u32 / 255) as u8;
                    }
                } else {
                    d.px[i..i + 3].copy_from_slice(&rgba[..3]);
                }
            }
        }
    }

    fn new_page_from(&mut self, name: &str, with_alpha: bool) -> i32 {
        match self.picture(name) {
            Some(img) => {
                let alpha = with_alpha || img.px.chunks_exact(4).any(|p| p[3] != 255);
                self.gfx.add_page(Page {
                    img: Rc::new((*img).clone()),
                    alpha,
                    anti: None,
                })
            }
            None => {
                self.warn(format!("no picture {name}"));
                0
            }
        }
    }

    fn sprite(&mut self, id: i32) -> Option<&mut Sprite> {
        self.gfx.sprites.get_mut(&id)
    }

    fn play(&mut self, channel: &str, name: &str, repeat: bool, volume: f32) -> bool {
        let file = if name.contains('.') {
            name.to_string()
        } else {
            [".ogg", ".wav"]
                .iter()
                .map(|x| format!("{name}{x}"))
                .find(|f| self.file_exists(f))
                .unwrap_or_else(|| format!("{name}.ogg"))
        };
        match self.read_file(&file) {
            Some(data) => {
                self.sounds.push(SoundEvent {
                    channel: channel.to_string(),
                    data: Some(data),
                    volume,
                    repeat,
                    fade_ms: 0.0,
                });
                true
            }
            None => {
                self.warn(format!("no sound {file}"));
                false
            }
        }
    }

    fn stop(&mut self, channel: &str, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }
}

/// Runs syscall `h`.
pub fn call(e: &mut Engine, th: &mut Thread, h: u32, a: Vec<Value>) -> Result<Ret, String> {
    let Some(&name) = e.sys.names.get(&h) else {
        e.warn(format!("unknown syscall ${h:08x} ({} args)", a.len()));
        return ok(0);
    };
    if e.log > 2 {
        eprintln!("  {name}{a:?}");
    }
    let now = e.now;
    match name {
        // --- flow ---
        "wait" => {
            let ms = int(&a, 0);
            if ms < 0 {
                return Ok(Ret::Wait(Wait::Tick));
            }
            if ms == 0 {
                return Ok(Ret::Wait(Wait::Frame));
            }
            return Ok(Ret::Wait(Wait::Until(now + ms as f64)));
        }
        "load_library" => {
            let n = string(&a, 0);
            if e.sys_loaded(&n) {
                return ok(0);
            }
            let main = e.script_main(&n)?;
            return Ok(Ret::Call(main, Vec::new()));
        }
        "exec" | "reset_exec" => {
            let n = string(&a, 0);
            let main = e.script_main(&n)?;
            cls(e);
            // a new game: console.mjo sets the window up again
            let first = if name == "reset_exec" {
                let args = vec![Value::Int(0), Value::string(""), Value::Int(1)];
                hooked(e, th, &[("HOT_RESET", args)])
            } else {
                Vec::new()
            };
            return Ok(Ret::Jump(main, first));
        }
        "call" => {
            let n = string(&a, 0);
            let main = e.script_main(&n)?;
            return Ok(Ret::Call(main, Vec::new()));
        }
        "system_end" | "exit" => {
            e.ended = true;
            return Ok(Ret::End);
        }
        "thread_begin" | "thread_begin_child" => {
            let f = int(&a, 0) as u32;
            let id = e.spawn(f, a[1.min(a.len())..].to_vec())?;
            return ok(id as i32);
        }
        "thread_end" => {
            let id = if a.is_empty() {
                th.id
            } else {
                int(&a, 0) as usize
            };
            if id == th.id {
                return Ok(Ret::End);
            }
            e.sys.killed.insert(id);
            e.spawned.retain(|t| t.id != id);
            return ok(0);
        }
        "thread_is_exist" => return ok(e.thread_alive(int(&a, 0) as usize) as i32),
        "event_hook" | "event_hook_after" => {
            e.hooks
                .push((string(&a, 0).to_string(), th.id, int(&a, 1) as u32));
            return ok(0);
        }
        "event_release" => {
            let n = string(&a, 0);
            e.hooks.retain(|h| *h.0 != *n);
            return ok(0);
        }
        "do_event" => {
            // each handler runs on the thread that made it
            let n = string(&a, 0);
            let args = a[1.min(a.len())..].to_vec();
            return calls(hooked(e, th, &[(&n, args)]));
        }
        "do_event_single" => {
            // (thread or -1, name, args...): every handler, on that thread
            let tid = match int(&a, 0) {
                -1 => th.id,
                t => t as usize,
            };
            let n = string(&a, 1);
            let args = a[2.min(a.len())..].to_vec();
            let fs: Vec<u32> = e
                .hooks
                .iter()
                .rev()
                .filter(|h| *h.0 == *n)
                .map(|h| h.2)
                .collect();
            if tid == th.id {
                return Ok(Ret::Calls(
                    fs.into_iter().map(|f| (f, args.clone())).collect(),
                ));
            }
            for f in fs {
                e.post_call(tid, f, args.clone());
            }
            return ok(0);
        }
        // --- numbers and strings ---
        "rand" => {
            let n = int(&a, 0);
            let r = e.sys.rand();
            return ok(if n > 0 { (r % n as u32) as i32 } else { 0 });
        }
        "max" => return ok(a.iter().map(Value::int).max().unwrap_or(0)),
        "min" => return ok(a.iter().map(Value::int).min().unwrap_or(0)),
        "abs" => return ok(int(&a, 0).wrapping_abs()),
        "sin%" => {
            return Ok(Ret::Val(Value::Float(float(&a, 0).to_radians().sin())));
        }
        "cos%" => {
            return Ok(Ret::Val(Value::Float(float(&a, 0).to_radians().cos())));
        }
        "rgb" => {
            let (r, g, b) = (int(&a, 0) as u32, int(&a, 1) as u32, int(&a, 2) as u32);
            return ok(((r & 255) << 16 | (g & 255) << 8 | (b & 255)) as i32);
        }
        "atoi" => return ok(string(&a, 0).trim().parse().unwrap_or(0)),
        "itoa$" => return Ok(Ret::Val(Value::string(int(&a, 0).to_string()))),
        "chr$" => {
            let c = int(&a, 0) as u32;
            let s = if c < 0x100 {
                char::from_u32(c).map(String::from).unwrap_or_default()
            } else {
                let b = [(c >> 8) as u8, c as u8];
                encoding_rs::SHIFT_JIS.decode(&b).0.into_owned()
            };
            return Ok(Ret::Val(Value::string(s)));
        }
        // lengths and positions count characters (the engine uses the _mbs functions);
        // a negative count is a huge unsigned one
        "strlen" => return ok(string(&a, 0).chars().count() as i32),
        "strleft$" => {
            let s = string(&a, 0);
            let n = int(&a, 1) as u32 as usize;
            return Ok(Ret::Val(Value::string(
                s.chars().take(n).collect::<String>(),
            )));
        }
        "strright$" => {
            let s = string(&a, 0);
            let n = int(&a, 1) as u32 as usize;
            let len = s.chars().count();
            return Ok(Ret::Val(Value::string(
                s.chars().skip(len.saturating_sub(n)).collect::<String>(),
            )));
        }
        "strmid$" => {
            let s = string(&a, 0);
            let start = int(&a, 1).max(0) as usize;
            let n = if a.len() > 2 {
                int(&a, 2) as u32 as usize
            } else {
                usize::MAX
            };
            return Ok(Ret::Val(Value::string(
                s.chars().skip(start).take(n).collect::<String>(),
            )));
        }
        "strstr" => {
            // ASCII letters match either case
            let s = string(&a, 0).to_ascii_uppercase();
            let t = string(&a, 1).to_ascii_uppercase();
            return ok(match s.find(&*t) {
                Some(i) => s[..i].chars().count() as i32,
                None => -1,
            });
        }
        "strupr$" => return Ok(Ret::Val(Value::string(string(&a, 0).to_ascii_uppercase()))),
        "stricmp" => {
            let (x, y) = (
                string(&a, 0).to_ascii_lowercase(),
                string(&a, 1).to_ascii_lowercase(),
            );
            return ok(x.cmp(&y) as i32);
        }
        "strmatch" => {
            let (s, p) = (string(&a, 0), string(&a, 1));
            return ok(wildcard(&p, &s) as i32);
        }
        "strsplit$" => {
            // (string, separator, index)
            let s = string(&a, 0);
            let sep = string(&a, 1);
            let i = int(&a, 2).max(0) as usize;
            let part = if sep.is_empty() {
                s.to_string()
            } else {
                s.split(&*sep).nth(i).unwrap_or("").to_string()
            };
            return Ok(Ret::Val(Value::string(part)));
        }
        "num_strsplit" => {
            let s = string(&a, 0);
            let sep = string(&a, 1);
            return ok(if sep.is_empty() {
                1
            } else {
                s.split(&*sep).count() as i32
            });
        }
        "dim_create#" | "dim_create$#" | "dim_create%#" => {
            let kind = if name == "dim_create$#" {
                2
            } else if name == "dim_create%#" {
                1
            } else {
                0
            };
            let dims: Vec<usize> = a.iter().map(|v| v.int().max(0) as usize).collect();
            let dims = if dims.is_empty() { vec![0] } else { dims };
            return Ok(Ret::Val(Value::Arr(Rc::new(std::cell::RefCell::new(
                Array::new(kind, dims),
            )))));
        }
        "timer" => return ok(now as i32),
        "timer_progress" => {
            // (start, length, end value): how far along `length` ms after `start` we are
            let (start, len, to) = (int(&a, 0), int(&a, 1), int(&a, 2));
            if len <= 0 {
                return ok(to);
            }
            let t = ((now as i32 - start) as f64 / len as f64).clamp(0.0, 1.0);
            return ok((to as f64 * t) as i32);
        }
        "timer_sync" => {
            // (start, ms): wait until `ms` after `start`
            let until = int(&a, 0) as f64 + int(&a, 1) as f64;
            if until > now {
                return Ok(Ret::Wait(Wait::Until(until)));
            }
            return ok(0);
        }
        "get_variable" | "get_variable_nowarn" => {
            let n = string(&a, 0);
            let h = crate::mjo::hash(&n);
            let v = e
                .persist
                .get(&h)
                .or_else(|| e.save.get(&h))
                .or_else(|| th.vars.get(&h))
                .cloned()
                .unwrap_or_default();
            return Ok(Ret::Val(v));
        }
        "set_variable" => {
            let n = string(&a, 0);
            let h = crate::mjo::hash(&n);
            let v = arg(&a, 1);
            match n.chars().next() {
                Some('#') => {
                    e.persist.insert(h, v);
                }
                Some('%') => {
                    th.vars.insert(h, v);
                }
                _ => {
                    e.save.insert(h, v);
                }
            }
            return ok(0);
        }
        "debugout" => {
            if e.log > 1 {
                eprintln!("debugout: {a:?}");
            }
            return ok(0);
        }
        // --- files ---
        "file_open" => {
            let n = string(&a, 0);
            let Some(b) = e.read_file(&n) else {
                e.warn(format!("no file {n}"));
                return ok(0);
            };
            let text = encoding_rs::SHIFT_JIS.decode(&b).0.into_owned();
            let id = e.sys.next_file;
            e.sys.next_file += 1;
            e.sys.files.insert(id, Reader::new(&text));
            return ok(id);
        }
        "file_close" => {
            e.sys.files.remove(&int(&a, 0));
            return ok(0);
        }
        "file_read" => {
            let t = e.sys.files.get_mut(&int(&a, 0)).and_then(Reader::token);
            return ok(t.map_or(0, |t| parse_int(&t)));
        }
        "file_read$" => {
            let t = e.sys.files.get_mut(&int(&a, 0)).and_then(Reader::token);
            return Ok(Ret::Val(Value::string(t.unwrap_or_default())));
        }
        "file_read_line$" => {
            let t = e.sys.files.get_mut(&int(&a, 0)).and_then(Reader::line);
            return Ok(Ret::Val(Value::string(t.unwrap_or_default())));
        }
        "pic_is_exist" => {
            let n = string(&a, 0);
            return ok(["rct", "rc8"]
                .iter()
                .any(|x| e.file_exists(&format!("{n}.{x}"))) as i32);
        }
        "sound_is_exist" | "voice_is_exist" => {
            let n = string(&a, 0);
            return ok(["ogg", "wav"]
                .iter()
                .any(|x| e.file_exists(&format!("{n}.{x}"))) as i32);
        }
        // --- screen ---
        "client_width" => return ok(e.width as i32),
        "client_height" => return ok(e.height as i32),
        "get_frontbuffer" => return ok(e.gfx.front),
        "set_frontbuffer" => {
            let p = int(&a, 0);
            if e.gfx.pages.contains_key(&p) {
                e.gfx.front = p;
            }
            return ok(0);
        }
        "invalidate_disable" => {
            let d = e.draws();
            e.frozen = Some(d);
            return ok(0);
        }
        "invalidate_enable" => {
            e.frozen = None;
            return ok(0);
        }
        "invalidate_rect" | "draw_wait" => {
            if name == "draw_wait" {
                return Ok(Ret::Wait(Wait::Frame));
            }
            return ok(0);
        }
        "page_create" | "page_create_withalfa" => {
            let (w, h) = (int(&a, 0), int(&a, 1));
            let mut p = Page::new(w.max(1) as u32, h.max(1) as u32);
            if a.len() > 2 {
                Rc::make_mut(&mut p.img).fill(color(int(&a, 2) as u32));
            }
            if name == "page_create_withalfa" {
                p.alpha = true;
                Rc::make_mut(&mut p.img).fill([0, 0, 0, 0]);
            }
            return ok(e.gfx.add_page(p));
        }
        "page_create_file" | "page_create_file_withalfa" => {
            let n = string(&a, 0);
            return ok(e.new_page_from(&n, name.ends_with("withalfa")));
        }
        "page_release" => {
            let p = int(&a, 0);
            if p != e.gfx.front {
                e.gfx.pages.remove(&p);
            }
            return ok(0);
        }
        "page_is_exist" => return ok(e.gfx.pages.contains_key(&int(&a, 0)) as i32),
        "page_len_x" => return ok(e.page_size(int(&a, 0)).0),
        "page_len_y" => return ok(e.page_size(int(&a, 0)).1),
        "get_alfapage" => return ok(int(&a, 0) | ALPHA),
        "pic_len_x" | "pic_len_y" => {
            let n = string(&a, 0);
            let img = e.picture(&n);
            return ok(img.map_or(0, |i| if name == "pic_len_x" { i.w } else { i.h } as i32));
        }
        "pic_unpack" => {
            // (page, name[, x, y])
            let (p, n) = (int(&a, 0), string(&a, 1));
            let (x, y) = (int(&a, 2), int(&a, 3));
            let Some(img) = e.picture(&n) else {
                e.warn(format!("no picture {n}"));
                return ok(0);
            };
            let has_alpha = img.px.chunks_exact(4).any(|p| p[3] != 255);
            let tmp = e.gfx.add_page(Page {
                img: img.clone(),
                alpha: has_alpha,
                anti: None,
            });
            e.blit(
                p,
                x,
                y,
                img.w as i32,
                img.h as i32,
                tmp,
                0,
                0,
                has_alpha.then_some(255),
            );
            e.gfx.pages.remove(&tmp);
            return ok(1);
        }
        "grp_copy" | "grp_extcopy" => {
            // (src, x, y, w, h, dst, x, y)
            let (s, sx, sy, w, h) = (int(&a, 0), int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4));
            let (d, dx, dy) = (int(&a, 5), int(&a, 6), int(&a, 7));
            // grp_extcopy's 9th: the copy's transparency
            let blend = (name == "grp_extcopy").then(|| 255 - int(&a, 8).clamp(0, 255) as u32);
            e.blit(d, dx, dy, w, h, s, sx, sy, blend);
            return ok(0);
        }
        "grp_swap" => {
            let (d, dx, dy, w, h) = (int(&a, 0), int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4));
            let (s, sx, sy) = (int(&a, 5), int(&a, 6), int(&a, 7));
            let (tw, th_) = (w.max(1) as u32, h.max(1) as u32);
            let tmp = e.gfx.add_page(Page::new(tw, th_));
            e.blit(tmp, 0, 0, w, h, d, dx, dy, None);
            e.blit(d, dx, dy, w, h, s, sx, sy, None);
            e.blit(s, sx, sy, w, h, tmp, 0, 0, None);
            e.gfx.pages.remove(&tmp);
            return ok(0);
        }
        "grp_boxfill" | "grp_mulboxfill" => {
            let (p, x, y, w, h) = (int(&a, 0), int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4));
            e.boxfill(p, x, y, w, h, int(&a, 5) as u32, name == "grp_mulboxfill");
            return ok(0);
        }
        "grp_modcopy" => {
            // (src, x, y, w, h, dst, x, y, w, h): scaled
            let s = int(&a, 0);
            let sr = [int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4)];
            let d = int(&a, 5);
            let dr = [int(&a, 6), int(&a, 7), int(&a, 8), int(&a, 9)];
            if let Some(sp) = e.gfx.pages.get(&(s & !ALPHA)) {
                let simg = sp.img.clone();
                if let Some((dimg, _)) = e.page_img(d) {
                    let p = nexas::gfx::Place {
                        x: 0.0,
                        y: 0.0,
                        sx: 1.0,
                        sy: 1.0,
                        cx: 0.0,
                        cy: 0.0,
                        rot: 0.0,
                        alpha: 1.0,
                        add: false,
                        flip: (false, false),
                        clip: [0.0, 0.0, dimg.w as f32, dimg.h as f32],
                    };
                    nexas::gfx::draw(
                        dimg,
                        &simg,
                        [sr[0], sr[1], sr[0] + sr[2], sr[1] + sr[3]],
                        [
                            dr[0] as f32,
                            dr[1] as f32,
                            (dr[0] + dr[2]) as f32,
                            (dr[1] + dr[3]) as f32,
                        ],
                        &p,
                    );
                }
            }
            return ok(0);
        }
        // --- sprites ---
        "sprite_create" | "sprite_create_ext" => {
            let p = int(&a, 0);
            let (pw, ph) = e.page_size(p);
            let src = if a.len() >= 5 {
                [int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4)]
            } else {
                [0, 0, pw, ph]
            };
            let mut s = Sprite::new(p, src);
            s.ext = name.ends_with("_ext");
            return ok(e.gfx.add_sprite(s));
        }
        "sprite_create_file" | "sprite_create_file_ext" => {
            let n = string(&a, 0);
            let p = e.new_page_from(&n, false);
            if p == 0 {
                return ok(0);
            }
            let (pw, ph) = e.page_size(p);
            let mut s = Sprite::new(p, [0, 0, pw, ph]);
            s.owns_page = true;
            s.ext = name.ends_with("_ext");
            return ok(e.gfx.add_sprite(s));
        }
        "sprite_release" => {
            let id = int(&a, 0);
            if let Some(s) = e.gfx.sprites.get(&id)
                && s.owns_page
            {
                let p = s.page;
                e.gfx.pages.remove(&p);
            }
            e.gfx.release_sprite(id);
            return ok(0);
        }
        "sprite_page_auto_release" => {
            if let Some(s) = e.sprite(int(&a, 0)) {
                s.owns_page = true;
            }
            return ok(0);
        }
        "sprite_is_exist" => return ok(e.gfx.sprites.contains_key(&int(&a, 0)) as i32),
        "sprite_get_page" => return ok(e.gfx.sprites.get(&int(&a, 0)).map_or(0, |s| s.page)),
        "sprite_len_x" => return ok(e.gfx.sprites.get(&int(&a, 0)).map_or(0, |s| s.src[2])),
        "sprite_len_y" => return ok(e.gfx.sprites.get(&int(&a, 0)).map_or(0, |s| s.src[3])),
        "sprite_get_pos_x" => {
            return ok(e
                .gfx
                .sprites
                .get(&int(&a, 0))
                .map_or(0, |s| s.x.round() as i32));
        }
        "sprite_get_pos_y" => {
            return ok(e
                .gfx
                .sprites
                .get(&int(&a, 0))
                .map_or(0, |s| s.y.round() as i32));
        }
        "sprite_move" => {
            // (sprite, x, y[, ms])
            let (x, y) = (int(&a, 1) as f32, int(&a, 2) as f32);
            // the fourth: ms, with the easing in bits 27..31
            let ms = if a.len() > 3 {
                (int(&a, 3) & 0x00ff_ffff) as f64
            } else {
                0.0
            };
            if let Some(s) = e.sprite(int(&a, 0)) {
                if ms > 0.0 && s.x < 50000.0 {
                    s.mv = Some(Anim {
                        from: (s.x, s.y),
                        to: (x, y),
                        start: now,
                        ms,
                    });
                } else {
                    s.mv = None;
                    s.x = x;
                    s.y = y;
                }
            }
            return ok(0);
        }
        "sprite_is_move" => {
            return ok(e.gfx.sprites.get(&int(&a, 0)).is_some_and(Sprite::moving) as i32);
        }
        "sprite_move_wait" | "sprite_alfa_wait" => {
            return Ok(Ret::Wait(Wait::Sprite(int(&a, 0))));
        }
        "sprite_alfa_set" => {
            // (sprite, transparency 0..255[, ms])
            let to = 1.0 - int(&a, 1).clamp(0, 255) as f32 / 255.0;
            let ms = if a.len() > 2 { int(&a, 2) as f64 } else { 0.0 };
            if let Some(s) = e.sprite(int(&a, 0)) {
                s.alpha_def.clear();
                if ms > 0.0 {
                    s.fade = Some((s.alpha, to, now, ms));
                } else {
                    s.fade = None;
                    s.alpha = to;
                }
            }
            return ok(0);
        }
        "sprite_alfa_define" | "sprite_alfa_define_abs" => {
            // (sprite, value, ms, value, ms, ...): -1 ms holds the last value
            let id = int(&a, 0);
            let keys: Vec<(f32, f64)> = a[1..]
                .chunks(2)
                .map(|c| {
                    (
                        1.0 - c[0].int().clamp(0, 255) as f32 / 255.0,
                        c.get(1).map_or(-1.0, |v| v.int() as f64),
                    )
                })
                .collect();
            if let Some(s) = e.sprite(id) {
                s.fade = None;
                s.alpha_def = keys;
                s.alpha_start = now;
            }
            return ok(0);
        }
        "sprite_priority_high" | "sprite_priority_high_single" => {
            e.gfx.reorder(int(&a, 0), int(&a, 1), true);
            return ok(0);
        }
        "sprite_priority_low" | "sprite_priority_low_single" => {
            e.gfx.reorder(int(&a, 0), int(&a, 1), false);
            return ok(0);
        }
        "sprite_paste" => {
            // (sprite, page): draws the sprite onto the page
            let (id, p) = (int(&a, 0), int(&a, 1));
            if let Some(s) = e.gfx.sprites.get(&id).cloned() {
                let (x, y) = if s.ext {
                    (s.x as i32 - s.src[2] / 2, s.y as i32 - s.src[3] / 2)
                } else {
                    (s.x as i32, s.y as i32)
                };
                e.blit(
                    p,
                    x,
                    y,
                    s.src[2],
                    s.src[3],
                    s.page,
                    s.src[0],
                    s.src[1],
                    Some(255),
                );
            }
            return ok(0);
        }
        "sprite_rotate" => {
            if let Some(s) = e.sprite(int(&a, 0)) {
                s.rot = float(&a, 1).to_radians();
            }
            return ok(0);
        }
        "sprite_xmodify_set" => {
            if let Some(s) = e.sprite(int(&a, 0)) {
                s.sx = float(&a, 1);
            }
            return ok(0);
        }
        "sprite_ymodify_set" => {
            if let Some(s) = e.sprite(int(&a, 0)) {
                s.sy = float(&a, 1);
            }
            return ok(0);
        }
        // --- areas ---
        "areaevent_set" => {
            let ar = Area {
                n: int(&a, 0),
                rect: [int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4)],
                on: int(&a, 5) as u32,
                off: int(&a, 6) as u32,
                push: int(&a, 7) as u32,
                other: int(&a, 8) as u32,
            };
            e.sys.areas.retain(|x| x.n != ar.n);
            e.sys.areas.push(ar);
            return ok(0);
        }
        "areaevent_setdefault" => {
            e.sys.area_default = (int(&a, 0) as u32, int(&a, 1) as u32);
            return ok(0);
        }
        "areaevent_reset" => {
            e.sys.areas.clear();
            e.sys.hovered = None;
            e.sys.area_queue.clear();
            e.sys.area_default = (0, 0);
            return ok(0);
        }
        "areaevent_check" => return area_check(e),
        // --- input ---
        "mk_wait" => {
            e.sys.clicked = None;
            return Ok(Ret::Wait(Wait::Key));
        }
        "mk_read" | "kb_read" => return ok(e.sys.take_click().map_or(0, |k| k as i32 + 1)),
        "mk_stat" => return ok(0),
        "mk_unwait" => {
            e.sys.clicked = None;
            return ok(0);
        }
        "curpos_x" => return ok(e.mouse.0),
        "curpos_y" => return ok(e.mouse.1),
        // --- sound ---
        "music" | "extmusic" => {
            let n = string(&a, 0);
            e.play("music", &n, true, 1.0);
            return ok(0);
        }
        "music_stop" => {
            e.stop("music", 0.0);
            return ok(0);
        }
        "music_fadeout" => {
            e.stop("music", int(&a, 0) as f32);
            return ok(0);
        }
        "sound" | "sound2" | "sound3" | "sound4" | "extsound" | "extsound2" | "extsound3"
        | "extsound4" => {
            let ch = name.trim_start_matches("ext").to_string();
            let n = string(&a, 0);
            let repeat = int(&a, 1) != 0 && a.len() > 1;
            e.play(&ch, &n, repeat, 1.0);
            return ok(0);
        }
        "sound_stop" | "sound2_stop" | "sound3_stop" | "sound4_stop" => {
            e.stop(name.trim_end_matches("_stop"), 0.0);
            return ok(0);
        }
        "sound_fadeout" | "sound2_fadeout" | "sound3_fadeout" | "sound4_fadeout" => {
            e.stop(name.trim_end_matches("_fadeout"), int(&a, 0) as f32);
            return ok(0);
        }
        "voice" | "voice_raw" => {
            let n = string(&a, 0);
            e.play("voice", &n, false, 1.0);
            return ok(0);
        }
        "voice_stop" => {
            e.stop("voice", 0.0);
            return ok(0);
        }
        // --- console ---
        "console_on" | "console_off" => {
            // ([fast, nowait]): console.mjo shows or hides the window
            let on = name == "console_on";
            if e.sys.console.on == on {
                return ok(0);
            }
            e.sys.console.on = on;
            let args = vec![arg(&a, 0), arg(&a, 1)];
            let ev = if on { "CONSOLE_ON" } else { "CONSOLE_OFF" };
            return calls(hooked(e, th, &[(ev, args)]));
        }
        "console_is_on" => return ok(e.sys.console.on as i32),
        "console_cls" => {
            cls(e);
            return calls(hooked(e, th, &[("CONSOLE_CLS", Vec::new())]));
        }
        "console_set" => {
            // (page, x, y, w, h)
            let c = &mut e.sys.console;
            c.page = int(&a, 0);
            c.area = [int(&a, 1), int(&a, 2), int(&a, 3), int(&a, 4)];
            if c.text.is_empty() {
                c.home();
            }
            return ok(0);
        }
        "console_font" | "fontout_font" => {
            if e.log > 1 {
                eprintln!("majiro: {name} {a:?}");
            }
            // (name, height, width[, line step], proportional) for the console;
            // (name, height, width, proportional) for fontout
            let f = if name == "console_font" {
                &mut e.sys.console.font
            } else {
                &mut e.sys.fontout.font
            };
            f.yl = int(&a, 1).max(6);
            f.xl = int(&a, 2).max(3);
            if name == "console_font" {
                f.step = int(&a, 3).max(f.yl);
                f.prop = int(&a, 4) != 0;
            } else {
                f.prop = int(&a, 3) != 0;
            }
            return ok(0);
        }
        "console_color" | "fontout_color" => {
            if e.log > 1 {
                eprintln!("majiro: {name} {a:?}");
            }
            // (letters, shadow | its transparency << 24)
            let f = if name == "console_color" {
                &mut e.sys.console.font
            } else {
                &mut e.sys.fontout.font
            };
            f.fore = int(&a, 0) as u32;
            f.back = if a.len() > 1 {
                int(&a, 1) as u32
            } else {
                0xff00_0000
            };
            return ok(0);
        }
        "console_curpos_x" => return ok(e.sys.console.x),
        "console_curpos_y" => return ok(e.sys.console.y),
        "console_locate" | "console_locate_force" => {
            e.sys.console.x = int(&a, 0);
            e.sys.console.y = int(&a, 1);
            return ok(0);
        }
        "fontout_locate" => {
            let f = &mut e.sys.fontout;
            f.page = int(&a, 0);
            f.x = int(&a, 1);
            f.y = int(&a, 2);
            return ok(0);
        }
        "fontout" => {
            let s = string(&a, 0);
            crate::console::fontout(e, &s);
            return ok(0);
        }
        // --- settings ---
        "get_effectspeed" => return ok(e.sys.effect_speed),
        "set_effectspeed" => {
            e.sys.effect_speed = int(&a, 0);
            return ok(0);
        }
        "get_mojispeed" => return ok(e.sys.moji_speed),
        "set_mojispeed" => {
            e.sys.moji_speed = int(&a, 0);
            return ok(0);
        }
        "get_autospeed" => return ok(e.sys.auto_speed),
        "set_autospeed" => {
            e.sys.auto_speed = int(&a, 0);
            return ok(0);
        }
        "is_fast_mode"
        | "is_ultra_fast_mode"
        | "is_auto_mode"
        | "is_testplay"
        | "get_fast_stat"
        | "is_read"
        | "is_log_no_pic"
        | "is_report_no_pic"
        | "is_voice_panning"
        | "is_save_menu_disable"
        | "get_caps_stat" => return ok(0),
        "is_talk_female" => return ok(1),
        "music_stat" | "sound_stat" | "voice_stat" | "movie_stat" => return ok(0),
        "sound_wait" | "sound2_wait" | "sound3_wait" | "voice_wait" | "music_wait"
        | "music_fadeout_wait" | "voice_ch_wait" | "movie_wait" => return ok(0),
        "get_save_info$" | "get_save_time$" | "get_last_msg$" | "sound_get_fn$"
        | "sound2_get_fn$" | "sound3_get_fn$" | "get_clipbord$" | "voicerect_name$"
        | "get_font_name$" | "font_dialog$" | "dialog_input$" => {
            return Ok(Ret::Val(Value::string("")));
        }
        "page_set_antidata" => {
            // (page, page): the second page's brightness is the first one's alpha
            let (p, q) = (int(&a, 0), int(&a, 1));
            if let Some(pg) = e.gfx.pages.get_mut(&p) {
                pg.anti = Some(q);
                pg.alpha = true;
            }
            return ok(0);
        }
        // settings with nothing to do here
        "pic_key_set"
        | "set_font_cache"
        | "fast_mode_config"
        | "set_voicevolume"
        | "set_musicvolume"
        | "set_soundvolume"
        | "set_fast_mode_flg"
        | "set_font_name"
        | "set_caps_stat"
        | "screen_mode"
        | "auto_mode_config"
        | "fast_mode_disable"
        | "fast_mode_enable"
        | "histbuff_enable"
        | "histbuff_disable"
        | "save_enable"
        | "save_disable"
        | "save_menu_enable"
        | "save_menu_disable"
        | "mouse_enable"
        | "mouse_disable"
        | "show_mouse_cursor"
        | "change_cursor"
        | "set_watchlist"
        | "time_accera"
        | "reset_fast_mode"
        | "reset_ultra_fast_mode"
        | "set_voice_skip"
        | "regist_arcfile"
        | "release_arcfile"
        | "write_pic_cache"
        | "set_fast_stat"
        | "set_fast_mode"
        | "set_auto_mode"
        | "console_setzerotime"
        | "console_resetzerotime"
        | "set_mouse_capture_left"
        | "set_mouse_capture_right"
        | "mouse_capture_init"
        | "mouse_capture_end"
        | "sprite_set_smooth_animation"
        | "voice_vol"
        | "voice_ch_vol"
        | "sound_vol"
        | "sound2_vol"
        | "sound3_vol"
        | "sound_ch_vol"
        | "music_vol"
        | "movie_volume"
        | "voice_pan"
        | "save_point"
        | "force_key_paste"
        | "set_clipbord" => return ok(0),
        _ => {}
    }
    e.warn(format!("syscall {name} {a:?} not done"));
    ok(0)
}

fn parse_int(t: &str) -> i32 {
    let t = t.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).unwrap_or(0) as i32;
    }
    t.parse::<f64>().map(|v| v as i32).unwrap_or(0)
}

/// `*` and `?` wildcards.
fn wildcard(p: &str, s: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (p.chars().collect(), s.chars().collect());
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| go(&p[1..], &s[i..])),
            Some('?') => !s.is_empty() && go(&p[1..], &s[1..]),
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    go(&p, &s)
}

/// `$areaevent_check`: runs the callback for the pointer entering or leaving an area, or
/// for a click, one a call.
fn area_check(e: &mut Engine) -> Result<Ret, String> {
    if e.sys.area_queue.is_empty() {
        let (mx, my) = e.mouse;
        let hit = e.sys.areas.iter().rposition(|ar| {
            let r = ar.rect;
            mx >= r[0] && my >= r[1] && mx < r[0] + r[2] && my < r[1] + r[3]
        });
        let args = |ar: &Area| {
            vec![
                Value::Int(ar.n),
                Value::Int(ar.rect[0]),
                Value::Int(ar.rect[1]),
                Value::Int(ar.rect[2]),
                Value::Int(ar.rect[3]),
            ]
        };
        if hit != e.sys.hovered {
            if let Some(old) = e.sys.hovered.and_then(|i| e.sys.areas.get(i)).copied()
                && old.off != 0
                && old.off != u32::MAX
            {
                e.sys.area_queue.push((old.off, args(&old)));
            }
            if let Some(new) = hit.and_then(|i| e.sys.areas.get(i)).copied()
                && new.on != 0
                && new.on != u32::MAX
            {
                e.sys.area_queue.push((new.on, args(&new)));
            }
            e.sys.hovered = hit;
        }
        if let Some(b) = e.sys.area_click.take() {
            e.sys.clicked = None;
            let pos = vec![Value::Int(mx), Value::Int(my)];
            match (b, hit.and_then(|i| e.sys.areas.get(i)).copied()) {
                (0, Some(ar)) if ar.push != 0 && ar.push != u32::MAX => {
                    e.sys.area_queue.push((ar.push, args(&ar)))
                }
                (0, None) if e.sys.area_default.0 != 0 => {
                    e.sys.area_queue.push((e.sys.area_default.0, pos))
                }
                (1, _) if e.sys.area_default.1 != 0 => {
                    e.sys.area_queue.push((e.sys.area_default.1, pos))
                }
                _ => {}
            }
        }
        e.sys.area_queue.reverse();
    }
    match e.sys.area_queue.pop() {
        Some((f, args)) => Ok(Ret::Call(f, args)),
        None => ok(0),
    }
}

impl Engine {
    /// Remembers a loaded library; true when it was loaded before.
    fn sys_loaded(&mut self, name: &str) -> bool {
        !self.sys.libs.insert(name.to_lowercase())
    }
}
