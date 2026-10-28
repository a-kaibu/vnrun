//! Runs an Escu:de (ACPX) game: compiles its C programs, runs them on the [`Vm`] and does what
//! their built-in functions ask (planes, sprites, text, sounds, files, timers and input).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexas::files::Files;

use crate::archive::Archives;
use crate::c::compile::{Exports, compile};
use crate::gfx::{Mode, Planes, Rect, empty, intersect, read_bmp};
use crate::vm::{Host, Native, Stop, Vm};

/// The save file that keeps when each save file was written.
const TIMES: &str = "vnrun-times.txt";

/// Writes a SYSTEMTIME (year, month, weekday, day, hour, minute, second, ms; WORDs).
fn write_systemtime(vm: &mut Vm, p: u32, t: i64) -> Result<(), String> {
    let (days, s) = (t.div_euclid(86400), t.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let week = (days + 4).rem_euclid(7);
    for (k, v) in [y, m, week, d, s / 3600, s / 60 % 60, s % 60, 0]
        .iter()
        .enumerate()
    {
        vm.w16(p + 2 * k as u32, *v as u16)?;
    }
    Ok(())
}

/// Where `dir_get` says the documents folder is; files under it are saves.
const DOCS: &str = "C:\\Documents\\";

/// Instructions a frame may run before it is cut short.
const BUDGET: u64 = 4_000_000;

/// Field offsets of a struct the built-ins read.
#[derive(Default, Clone)]
struct Layout(HashMap<String, u32>);

impl Layout {
    fn of(types: &crate::c::ast::Types, name: &str) -> Layout {
        let mut m = HashMap::new();
        if let Some(s) = types.structs.iter().find(|s| s.name == name) {
            for f in &s.fields {
                m.insert(f.name.clone(), f.off);
            }
            m.insert("<size>".into(), s.size);
        }
        Layout(m)
    }

    fn at(&self, f: &str) -> u32 {
        self.0.get(f).copied().unwrap_or(0)
    }
}

/// A sound for the player: start (with data), stop, or change volume.
#[derive(Debug, Clone)]
pub struct SoundEvent {
    pub channel: i32,
    /// "play", "stop", "volume", "pause", "resume"
    pub op: &'static str,
    pub data: Option<Rc<Vec<u8>>>,
    /// 0..1
    pub volume: f32,
    pub repeat: bool,
}

struct Sound {
    data: Rc<Vec<u8>>,
    /// length in ms (0: not known)
    len: f64,
    volume: u32,
    /// clock when it started, while playing
    started: Option<f64>,
    looped: bool,
    paused_at: Option<f64>,
}

struct OpenFile {
    data: Vec<u8>,
    pos: usize,
    /// saved when closed (a save file being written)
    write: Option<String>,
}

#[derive(Clone, Copy)]
struct FontStyle {
    size: i32,
    style: u32,
}

pub struct State {
    files: Box<dyn Files>,
    arc: Archives,
    /// the main program's folder ("verdia\")
    base: String,
    root: Option<Rc<Exports>>,
    programs: HashMap<String, u32>,
    pub planes: Planes,
    width: i32,
    height: i32,
    apage: i32,
    vpage: i32,
    org: (i32, i32),
    // sprites
    sprites: Vec<u32>,
    sp_bg: i32,
    sp_bgorg: (i32, i32),
    sp_org: [(i32, i32); 32],
    sp_flag: u32,
    spr: Layout,
    rect_l: Layout,
    // the screen as last shown (RGBA)
    pub frame: Vec<u8>,
    pub frame_serial: u64,
    // time
    real: f64,
    clock: f64,
    polls: u32,
    counters: HashMap<i32, f64>,
    updated: bool,
    /// local time, seconds since 1970
    wall: i64,
    /// when each save file was written (local seconds), kept in the save file `TIMES`
    times: Option<HashMap<String, i64>>,
    // input
    mouse: (i32, i32),
    buttons: u32,
    latched_buttons: u32,
    keys: [bool; 256],
    latched_keys: [bool; 256],
    // sounds
    sounds: HashMap<i32, Sound>,
    pub events: Vec<SoundEvent>,
    // files
    open: HashMap<u32, OpenFile>,
    next_handle: u32,
    sysfiles: Vec<(String, u32, u32, Vec<u8>)>,
    // text
    font: Option<fontdue::Font>,
    fonts: [FontStyle; 8],
    font_now: usize,
    glyphs: HashMap<(char, i32), (fontdue::Metrics, Vec<u8>)>,
    seed: u32,
    next_plane: i32,
    warned: HashSet<String>,
    pub log: Vec<String>,
    pub ended: bool,
    /// time spent in each built-in (calls, seconds), when profiling
    pub profile: Option<HashMap<String, (u64, f64)>>,
    /// log every built-in called, with its arguments and where (for debugging)
    pub trace: bool,
}

pub struct Engine {
    pub vm: Vm,
    pub st: State,
}

fn rd_rect(vm: &Vm, p: u32) -> Result<Option<Rect>, String> {
    if p == 0 {
        return Ok(None);
    }
    Ok(Some([
        vm.ri32(p)?,
        vm.ri32(p + 4)?,
        vm.ri32(p + 8)?,
        vm.ri32(p + 12)?,
    ]))
}

fn wr_rect(vm: &mut Vm, p: u32, r: Rect) -> Result<(), String> {
    if p == 0 {
        return Ok(());
    }
    for (k, v) in r.iter().enumerate() {
        vm.w32(p + 4 * k as u32, *v as u32)?;
    }
    Ok(())
}

fn rd_mode(vm: &Vm, p: u32) -> Result<Mode, String> {
    if p == 0 {
        return Ok(Mode::NORMAL);
    }
    Ok(Mode {
        mode: vm.r8(p)?,
        mask: vm.r8(p + 1)?,
        level: vm.r8(p + 2)?,
        range: vm.r8(p + 3)?,
        trans: vm.r32(p + 4)?,
        color: vm.r32(p + 8)?,
    })
}

fn f(v: u32) -> f32 {
    f32::from_bits(v)
}

fn fv(x: f32) -> Native {
    Native::Value(x.to_bits())
}

fn b(x: bool) -> Native {
    Native::Value(x as u32)
}

/// Length in ms of an Ogg Vorbis file (from its last granule position).
fn ogg_ms(d: &[u8]) -> f64 {
    if d.len() < 64 || &d[..4] != b"OggS" {
        return 0.0;
    }
    let seg = d[26] as usize;
    let p = 27 + seg;
    if d.len() < p + 16 || &d[p + 1..p + 7] != b"vorbis" {
        return 0.0;
    }
    let rate = u32::from_le_bytes(d[p + 12..p + 16].try_into().unwrap()) as f64;
    let mut i = d.len().saturating_sub(65536);
    let mut last = 0u64;
    while i + 14 <= d.len() {
        if &d[i..i + 4] == b"OggS" {
            last = u64::from_le_bytes(d[i + 6..i + 14].try_into().unwrap());
        }
        i += 1;
    }
    if rate <= 0.0 || last == u64::MAX {
        return 0.0;
    }
    last as f64 * 1000.0 / rate
}

impl Engine {
    /// Opens the game in `files`: reads its .cfg and compiles the program it runs.
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arc = Archives::open(files.as_mut())?;
        // the .cfg that says what to run, or one with the screen size
        let mut cfg = String::new();
        for name in files.list() {
            if name.to_lowercase().ends_with(".cfg")
                && let Some(b) = files.read(&name)
            {
                let text = nexas::sjis::decode(&b);
                let has_exec = text.contains("Execute=");
                if has_exec || text.contains("Width=") {
                    cfg = text;
                    if has_exec {
                        break;
                    }
                }
            }
        }
        let get = |k: &str| {
            cfg.lines()
                .find_map(|l| l.trim().strip_prefix(k).and_then(|r| r.strip_prefix('=')))
                .map(|v| v.trim().to_string())
        };
        // without Execute= (Re;Lord), the main.c of an archive named like the exe
        let exec = match get("Execute") {
            Some(e) => e,
            None => {
                let exes: Vec<String> = files
                    .list()
                    .iter()
                    .filter(|n| n.to_lowercase().ends_with(".exe"))
                    .map(|n| n[..n.len() - 4].to_lowercase())
                    .collect();
                let mains: Vec<&String> = arc
                    .names
                    .iter()
                    .filter(|n| n.to_lowercase().ends_with("\\main.c"))
                    .collect();
                mains
                    .iter()
                    .find(|n| {
                        exes.iter()
                            .any(|e| n.to_lowercase() == format!("{e}\\main.c"))
                    })
                    .or(mains.first())
                    .map(|s| s.to_string())
                    .ok_or("no Execute= in a .cfg and no main.c")?
            }
        };
        let width = get("Width").and_then(|v| v.parse().ok()).unwrap_or(800);
        let height = get("Height").and_then(|v| v.parse().ok()).unwrap_or(600);
        let base = exec
            .rfind('\\')
            .map_or(String::new(), |n| exec[..n + 1].to_string());
        let mut vm = Vm::new();
        let mut st = State {
            files,
            arc,
            base,
            root: None,
            programs: HashMap::new(),
            planes: Planes::default(),
            width,
            height,
            apage: 0,
            vpage: 0,
            org: (0, 0),
            sprites: Vec::new(),
            sp_bg: -1,
            sp_bgorg: (0, 0),
            sp_org: [(0, 0); 32],
            sp_flag: 1,
            spr: Layout::default(),
            rect_l: Layout::default(),
            frame: vec![0; (width * height * 4) as usize],
            frame_serial: 0,
            real: 0.0,
            clock: 0.0,
            polls: 0,
            counters: HashMap::new(),
            updated: false,
            wall: 0,
            times: None,
            mouse: (0, 0),
            buttons: 0,
            latched_buttons: 0,
            keys: [false; 256],
            latched_keys: [false; 256],
            sounds: HashMap::new(),
            events: Vec::new(),
            open: HashMap::new(),
            next_handle: 1,
            sysfiles: Vec::new(),
            font: None,
            fonts: [FontStyle { size: 16, style: 0 }; 8],
            font_now: 0,
            glyphs: HashMap::new(),
            seed: 1,
            next_plane: 1000,
            warned: HashSet::new(),
            log: Vec::new(),
            ended: false,
            profile: None,
            trace: false,
        };
        st.planes.alloc(&mut vm, 0, width, height, 24);
        let start = st.load_program(&mut vm, &exec)?;
        vm.call(start, &[])?;
        Ok(Engine { vm, st })
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.st.font = Some(f);
        self.st.glyphs.clear();
        Ok(())
    }

    pub fn width(&self) -> i32 {
        self.st.width
    }

    pub fn height(&self) -> i32 {
        self.st.height
    }

    /// Runs the game until it waits for the next frame. `now`: ms since the game started.
    pub fn frame(&mut self, now: f64) -> Result<(), String> {
        if self.st.ended {
            return Ok(());
        }
        self.st.real = now;
        if self.st.clock < now - 50.0 {
            self.st.clock = now - 50.0;
        }
        self.st.updated = false;
        self.st.polls = 0;
        match self.vm.run(&mut self.st, BUDGET)? {
            Stop::Done => {
                self.st.save_sysfiles(&self.vm);
                self.st.ended = true;
            }
            Stop::Yield | Stop::Budget => {}
        }
        Ok(())
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.st.mouse = (x, y);
    }

    /// A mouse button went down (0 left, 1 right, 2 middle).
    pub fn button_down(&mut self, button: u32) {
        self.st.buttons |= 1 << button;
        self.st.latched_buttons |= 1 << button;
    }

    pub fn button_up(&mut self, button: u32) {
        self.st.buttons &= !(1 << button);
    }

    /// A key went down or up, by its Windows virtual-key code.
    pub fn key(&mut self, code: u32, down: bool) {
        let c = (code & 0xff) as usize;
        self.st.keys[c] = down;
        if down {
            self.st.latched_keys[c] = true;
        }
    }

    pub fn set_clock(&mut self, secs: i64) {
        self.st.wall = secs;
    }

    /// Compiles every program in the main program's folder; gives the errors.
    pub fn compile_all(&mut self) -> Vec<String> {
        let base = crate::archive::key(&self.st.base);
        let names: Vec<String> = self
            .st
            .arc
            .names
            .iter()
            .filter(|n| {
                let k = crate::archive::key(n);
                k.starts_with(&base) && k.ends_with(".c")
            })
            .cloned()
            .collect();
        let mut errors = Vec::new();
        for n in names {
            if let Err(e) = self.st.load_program(&mut self.vm, &n) {
                errors.push(e);
            }
        }
        errors
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.st.events)
    }
}

impl State {
    fn warn(&mut self, what: String) {
        if self.warned.insert(what.clone()) {
            self.log.push(what);
        }
    }

    // ---- files

    fn read_game_file(&mut self, path: &str) -> Option<Vec<u8>> {
        if let Some(rest) = path.strip_prefix(DOCS) {
            // a deleted save is written empty
            return self
                .files
                .read_save(&rest.replace('\\', "/"))
                .filter(|d| !d.is_empty());
        }
        let p = path.trim_start_matches(".\\");
        if let Some(d) = self.arc.read(self.files.as_mut(), p) {
            return Some(d);
        }
        self.files.read(&p.replace('\\', "/"))
    }

    fn path(&self, vm: &Vm, p: u32) -> Result<String, String> {
        Ok(nexas::sjis::decode(&vm.cstr(p)?))
    }

    fn save_sysfiles(&mut self, vm: &Vm) {
        for i in 0..self.sysfiles.len() {
            let (name, addr, size, old) = &self.sysfiles[i];
            let Ok(now) = vm.bytes(*addr, *size) else {
                continue;
            };
            if now != &old[..] {
                let (name, now) = (name.clone(), now.to_vec());
                self.write_file(&name, &now);
                self.sysfiles[i].3 = now;
            }
        }
    }

    fn write_file(&mut self, path: &str, data: &[u8]) {
        let rest = path.strip_prefix(DOCS).unwrap_or(path).replace('\\', "/");
        self.files.write_save(&rest, data);
        let wall = self.wall;
        let times = self.times();
        if data.is_empty() {
            times.remove(&rest);
        } else {
            times.insert(rest, wall);
        }
        let mut list: Vec<_> = times.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
        list.sort();
        self.files.write_save(TIMES, list.concat().as_bytes());
    }

    fn times(&mut self) -> &mut HashMap<String, i64> {
        if self.times.is_none() {
            let text = self.files.read_save(TIMES).unwrap_or_default();
            let map = String::from_utf8_lossy(&text)
                .lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once('\t')?;
                    Some((k.to_string(), v.parse().ok()?))
                })
                .collect();
            self.times = Some(map);
        }
        self.times.as_mut().unwrap()
    }

    /// Compiles a program (once) and gives its start.
    fn load_program(&mut self, vm: &mut Vm, path: &str) -> Result<u32, String> {
        if let Some(&s) = self.programs.get(path) {
            return Ok(s);
        }
        let base = self.base.clone();
        let mut load = |p: &str| -> Option<Vec<u8>> {
            self.arc
                .read(self.files.as_mut(), p)
                .or_else(|| self.files.read(&p.replace('\\', "/")))
        };
        let unit = crate::c::parse_program(path, &base, &mut load)?;
        let root = self.root.clone();
        let c = compile(&unit, vm, root.as_deref()).map_err(|e| format!("{path}: {e}"))?;
        if self.root.is_none() {
            self.spr = Layout::of(&c.exports.types, "_sprt_");
            if self.spr.0.is_empty() {
                self.spr = Layout::of(&c.exports.types, "sprite_t");
            }
            self.rect_l = Layout::of(&c.exports.types, "_rect_");
            self.root = Some(Rc::new(c.exports));
        }
        self.programs.insert(path.to_string(), c.start);
        Ok(c.start)
    }

    // ---- planes

    /// A plane number, or a pointer to where a new plane's number goes.
    fn plane_arg(&mut self, vm: &mut Vm, v: u32) -> Result<i32, String> {
        if v >= 0x1_0000 {
            let mut n = self.next_plane;
            while self.planes.map.contains_key(&n) {
                n += 1;
            }
            self.next_plane = n + 1;
            if self.next_plane > 60000 {
                self.next_plane = 1000;
            }
            vm.w32(v, n as u32)?;
            Ok(n)
        } else {
            Ok(v as i32)
        }
    }

    fn load_picture(&mut self, vm: &mut Vm, n: i32, path: &str) -> Result<bool, String> {
        let Some(data) = self.read_game_file(path) else {
            self.warn(format!("gload: no {path}"));
            return Ok(false);
        };
        let Some((w, h, bits, px, pal)) = read_bmp(&data) else {
            self.warn(format!("gload: {path} is not a BMP"));
            return Ok(false);
        };
        let depth = if bits <= 8 { 8 } else { bits };
        self.planes.alloc(vm, n, w, h, depth);
        let p = self.planes.get(n).unwrap().clone();
        if bits < 8 {
            // unpack to a byte a pixel
            let src_stride = (w as usize * bits as usize).div_ceil(32) * 4;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let bit = x * bits as usize;
                    let byte = px[y * src_stride + bit / 8];
                    let v = (byte >> (8 - bits as usize - bit % 8)) & ((1 << bits) - 1);
                    vm.mem[p.addr as usize + y * p.stride as usize + x] = v;
                }
            }
        } else {
            let n_bytes = (p.stride * p.h as u32) as usize;
            vm.mem[p.addr as usize..p.addr as usize + n_bytes].copy_from_slice(&px[..n_bytes]);
        }
        if let Some(pal) = pal
            && let Some(pl) = self.planes.map.get_mut(&n)
        {
            pl.pal = Some(pal);
        }
        Ok(true)
    }

    /// Shows plane `vpage` (moved by the origin) as the frame.
    fn present(&mut self, vm: &Vm) {
        let (w, h) = (self.width, self.height);
        self.frame.fill(0);
        if let Some(p) = self.planes.get(self.vpage) {
            let (ox, oy) = self.org;
            let r = intersect([ox, oy, ox + w, oy + h], [0, 0, p.w, p.h]);
            if !empty(r) {
                let mut row = Vec::with_capacity((r[2] - r[0]) as usize);
                for y in r[1]..r[3] {
                    row.clear();
                    crate::gfx::read_row(&vm.mem, p, None, y, r[0], r[2], &mut row);
                    let at = (((y - oy) * w + (r[0] - ox)) * 4) as usize;
                    for (d, c) in self.frame[at..at + row.len() * 4]
                        .chunks_exact_mut(4)
                        .zip(&row)
                    {
                        d.copy_from_slice(&[c[2], c[1], c[0], 255]);
                    }
                }
            }
        }
        for a in self.frame.chunks_exact_mut(4) {
            a[3] = 255;
        }
        self.frame_serial += 1;
    }

    // ---- sprites

    fn sprite_world(
        &self,
        vm: &Vm,
        sp: u32,
        depth: u32,
    ) -> Result<(f32, f32, f32, f32, f32), String> {
        let l = &self.spr;
        let fix = vm.r16(sp + l.at("fix"))? as u32;
        let scale = (1u32 << fix.min(30)) as f32;
        let x = vm.ri32(sp + l.at("x"))? as f32 / scale;
        let y = vm.ri32(sp + l.at("y"))? as f32 / scale;
        let zx = vm.r32(sp + l.at("zx"))? as f32 / 65536.0;
        let zy = vm.r32(sp + l.at("zy"))? as f32 / 65536.0;
        let rot = vm.r16(sp + l.at("rot"))? as f32;
        let parent = vm.r32(sp + l.at("parent"))?;
        if parent != 0 && depth < 16 && self.sprites.contains(&parent) {
            let (px, py, pzx, pzy, prot) = self.sprite_world(vm, parent, depth + 1)?;
            let t = prot * std::f32::consts::TAU / 2048.0;
            let (s, c) = t.sin_cos();
            let (lx, ly) = (x * pzx, y * pzy);
            return Ok((
                px + c * lx + s * ly,
                py - s * lx + c * ly,
                zx * pzx,
                zy * pzy,
                prot + rot,
            ));
        }
        let pri = vm.r8(sp + l.at("priority"))? as usize & 31;
        let o = self.sp_org[pri];
        Ok((x - o.0 as f32, y - o.1 as f32, zx, zy, rot))
    }

    fn draw_sprite(&mut self, vm: &mut Vm, sp: u32, target: i32) -> Result<(), String> {
        let l = self.spr.clone();
        let page = vm.ri32(sp + l.at("page"))?;
        let Some(p) = self.planes.get(page) else {
            return Ok(());
        };
        let (pw, ph) = (p.w, p.h);
        let rc = rd_rect(vm, sp + l.at("rc"))?.unwrap_or([0, 0, pw, ph]);
        let m = rd_mode(vm, sp + l.at("bm"))?;
        if m.level == 0 && matches!(m.mode, 3 | 4 | 7 | 9 | 10 | 13 | 24) {
            return Ok(());
        }
        let (x, y, zx, zy, rot) = self.sprite_world(vm, sp, 0)?;
        vm.w32(sp + l.at("real_zx"), (zx * 65536.0) as u32)?;
        vm.w32(sp + l.at("real_zy"), (zy * 65536.0) as u32)?;
        vm.w16(sp + l.at("real_rot"), rot as i32 as u16)?;
        let cx = vm.ri32(sp + l.at("cx"))? as f32;
        let cy = vm.ri32(sp + l.at("cy"))? as f32;
        let rev_h = vm.r8(sp + l.at("rev_h"))? != 0;
        let rev_v = vm.r8(sp + l.at("rev_v"))? != 0;
        let (w, h) = ((rc[2] - rc[0]) as f32, (rc[3] - rc[1]) as f32);
        if zx <= 0.0 || zy <= 0.0 || w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        let rot_m = (rot as i32).rem_euclid(2048);
        if rot_m == 0
            && zx == 1.0
            && zy == 1.0
            && !rev_h
            && !rev_v
            && (x - cx).fract() == 0.0
            && (y - cy).fract() == 0.0
        {
            let (dx, dy) = ((x - cx) as i32, (y - cy) as i32);
            let r = [dx, dy, dx + w as i32, dy + h as i32];
            wr_rect(vm, sp + l.at("update"), r)?;
            wr_rect(vm, sp + l.at("blit"), r)?;
            self.planes.copy(vm, target, dx, dy, page, Some(rc), &m);
            return Ok(());
        }
        let t = rot_m as f32 * std::f32::consts::TAU / 2048.0;
        let (s, c) = t.sin_cos();
        // corners of the picture on the screen
        let fwd = |u: f32, v: f32| {
            let (lx, ly) = ((u - cx) * zx, (v - cy) * zy);
            (x + c * lx + s * ly, y - s * lx + c * ly)
        };
        let pts = [fwd(0.0, 0.0), fwd(w, 0.0), fwd(0.0, h), fwd(w, h)];
        let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
        let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
        let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
        let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
        let area = [x0, y0, x1, y1];
        wr_rect(vm, sp + l.at("update"), area)?;
        let bx = [
            (x - cx * zx) as i32,
            (y - cy * zy) as i32,
            (x + (w - cx) * zx) as i32,
            (y + (h - cy) * zy) as i32,
        ];
        wr_rect(vm, sp + l.at("blit"), bx)?;
        // screen -> picture: l = R p / z + c
        let (ax, ay) = (1.0 / zx, 1.0 / zy);
        let mut inv = [c * ax, -s * ax, 0.0, s * ay, c * ay, 0.0];
        inv[2] = -(inv[0] * x + inv[1] * y) + cx;
        inv[5] = -(inv[3] * x + inv[4] * y) + cy;
        if rev_h {
            inv = [-inv[0], -inv[1], w - inv[2], inv[3], inv[4], inv[5]];
        }
        if rev_v {
            inv = [inv[0], inv[1], inv[2], -inv[3], -inv[4], h - inv[5]];
        }
        inv[2] += rc[0] as f32;
        inv[5] += rc[1] as f32;
        self.planes
            .affine(vm, target, area, page, rc, inv, &m, false);
        Ok(())
    }

    fn sp_draw(&mut self, vm: &mut Vm, mask: u32, out: u32) -> Result<(), String> {
        let target = self.apage;
        let Some(t) = self.planes.get(target) else {
            return Ok(());
        };
        let full = [0, 0, t.w, t.h];
        if self.sp_bg >= 0 && self.planes.get(self.sp_bg).is_some() {
            let (ox, oy) = self.sp_bgorg;
            self.planes
                .copy(vm, target, -ox, -oy, self.sp_bg, None, &Mode::NORMAL);
        }
        let l = self.spr.clone();
        let mut list: Vec<(u8, usize, u32)> = Vec::new();
        for (k, &sp) in self.sprites.iter().enumerate() {
            let ty = vm.r8(sp + l.at("type"))?;
            let pri = vm.r8(sp + l.at("priority"))?;
            if ty == 2 && mask & (1 << (pri & 31)) != 0 {
                list.push((pri, k, sp));
            }
        }
        list.sort();
        let debug = std::env::var_os("ESCUDE_SPRITES").is_some() && self.frame_serial % 100 == 1;
        for (_, _, sp) in list {
            #[cfg(not(target_arch = "wasm32"))]
            let t = std::time::Instant::now();
            self.draw_sprite(vm, sp, target)?;
            vm.w8(sp + l.at("draw"), 1)?;
            #[cfg(not(target_arch = "wasm32"))]
            if debug {
                let page = vm.ri32(sp + l.at("page"))?;
                let rc = rd_rect(vm, sp + l.at("rc"))?;
                let m = rd_mode(vm, sp + l.at("bm"))?;
                let (zx, rot) = (vm.r32(sp + l.at("zx"))?, vm.r16(sp + l.at("rot"))?);
                let depth = self.planes.get(page).map_or(0, |p| p.depth);
                eprintln!(
                    "sprite page {page} ({depth}) {rc:?} mode {} level {} zoom {zx:x} rot {rot}: {:.2}ms",
                    m.mode,
                    m.level,
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
        wr_rect(vm, out, full)?;
        Ok(())
    }

    // ---- text

    fn glyph(&mut self, c: char, size: i32) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let font = self.font.as_ref()?;
        Some(
            self.glyphs
                .entry((c, size))
                .or_insert_with(|| font.rasterize(c, size as f32)),
        )
    }

    fn gtext(
        &mut self,
        vm: &mut Vm,
        n: i32,
        x: i32,
        y: i32,
        col: u32,
        text: &[u8],
    ) -> Result<(), String> {
        let Some(p) = self.planes.get(n).cloned() else {
            return Ok(());
        };
        let st = self.fonts[self.font_now];
        let size = st.size.abs().max(4);
        let ascent = self
            .font
            .as_ref()
            .and_then(|f| f.horizontal_line_metrics(size as f32))
            .map_or(size as f32 * 0.86, |m| {
                m.ascent * size as f32 / (m.ascent - m.descent)
            });
        let mut pen = x;
        let bounds = p.bounds();
        let s = nexas::sjis::decode(text);
        for ch in s.chars() {
            let half = (ch as u32) < 0x80 || ('\u{ff61}'..='\u{ff9f}').contains(&ch);
            let adv = if half { size / 2 } else { size };
            if let Some((m, cov)) = self.glyph(ch, size).cloned() {
                let gx = pen
                    + m.xmin
                    + if half {
                        0
                    } else {
                        ((size - m.advance_width as i32) / 2).max(0)
                    };
                let gy = y + ascent.round() as i32 - m.height as i32 - m.ymin;
                for yy in 0..m.height as i32 {
                    for xx in 0..m.width as i32 {
                        let a = cov[(yy * m.width as i32 + xx) as usize] as u32;
                        if a == 0 {
                            continue;
                        }
                        let (px, py) = (gx + xx, gy + yy);
                        if px < bounds[0] || py < bounds[1] || px >= bounds[2] || py >= bounds[3] {
                            continue;
                        }
                        let row = p.row(py);
                        let mix =
                            |d: u8, s: u8| ((d as u32 * (255 - a) + s as u32 * a) / 255) as u8;
                        match p.depth {
                            8 => {
                                let i = row + px as usize;
                                vm.mem[i] = mix(vm.mem[i], col as u8);
                            }
                            24 | 32 => {
                                let bpp = p.bpp();
                                let i = row + px as usize * bpp;
                                vm.mem[i] = mix(vm.mem[i], col as u8);
                                vm.mem[i + 1] = mix(vm.mem[i + 1], (col >> 8) as u8);
                                vm.mem[i + 2] = mix(vm.mem[i + 2], (col >> 16) as u8);
                                if bpp == 4 {
                                    vm.mem[i + 3] = vm.mem[i + 3].max(a as u8);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            pen += adv;
        }
        Ok(())
    }

    // ---- sprintf

    fn format(&self, vm: &Vm, fmt: &[u8], args: &[u32]) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut i = 0;
        let mut a = 0;
        let mut next = || {
            let v = args.get(a).copied().unwrap_or(0);
            a += 1;
            v
        };
        while i < fmt.len() {
            let c = fmt[i];
            if c != b'%' {
                if crate::c::lex::is_lead(c) && i + 1 < fmt.len() {
                    out.extend_from_slice(&fmt[i..i + 2]);
                    i += 2;
                } else {
                    out.push(c);
                    i += 1;
                }
                continue;
            }
            i += 1;
            let (mut left, mut zero, mut plus, mut space) = (false, false, false, false);
            while i < fmt.len() && matches!(fmt[i], b'-' | b'0' | b'+' | b' ' | b'#') {
                match fmt[i] {
                    b'-' => left = true,
                    b'0' => zero = true,
                    b'+' => plus = true,
                    b' ' => space = true,
                    _ => {}
                }
                i += 1;
            }
            let mut width = 0usize;
            if fmt.get(i) == Some(&b'*') {
                width = next() as i32 as usize;
                i += 1;
            }
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                width = width * 10 + (fmt[i] - b'0') as usize;
                i += 1;
            }
            let mut prec: Option<usize> = None;
            if fmt.get(i) == Some(&b'.') {
                i += 1;
                let mut p = 0;
                if fmt.get(i) == Some(&b'*') {
                    p = next() as usize;
                    i += 1;
                }
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    p = p * 10 + (fmt[i] - b'0') as usize;
                    i += 1;
                }
                prec = Some(p);
            }
            while i < fmt.len() && matches!(fmt[i], b'l' | b'h' | b'L') {
                i += 1;
            }
            let Some(&conv) = fmt.get(i) else { break };
            i += 1;
            let mut body: Vec<u8> = match conv {
                b'd' | b'i' => {
                    let v = next() as i32;
                    let mut s = v.unsigned_abs().to_string();
                    if let Some(p) = prec {
                        while s.len() < p {
                            s.insert(0, '0');
                        }
                    }
                    let sign = if v < 0 {
                        "-"
                    } else if plus {
                        "+"
                    } else if space {
                        " "
                    } else {
                        ""
                    };
                    format!("{sign}{s}").into_bytes()
                }
                b'u' => next().to_string().into_bytes(),
                b'x' => format!("{:x}", next()).into_bytes(),
                b'X' => format!("{:X}", next()).into_bytes(),
                b'o' => format!("{:o}", next()).into_bytes(),
                b'c' => {
                    let v = next();
                    if v > 0xff {
                        vec![(v >> 8) as u8, v as u8]
                    } else {
                        vec![v as u8]
                    }
                }
                b's' => {
                    let p = next();
                    let mut s = if p == 0 {
                        b"(null)".to_vec()
                    } else {
                        vm.cstr(p)?
                    };
                    if let Some(n) = prec {
                        s.truncate(n);
                    }
                    s
                }
                b'f' | b'e' | b'g' => {
                    let v = f(next());
                    format!("{:.*}", prec.unwrap_or(6), v).into_bytes()
                }
                b'p' => format!("{:08X}", next()).into_bytes(),
                b'%' => b"%".to_vec(),
                _ => vec![conv],
            };
            if body.len() < width {
                let pad = width - body.len();
                if left {
                    body.extend(std::iter::repeat_n(b' ', pad));
                } else if zero && conv != b's' {
                    let at = usize::from(matches!(body.first(), Some(b'-' | b'+')));
                    for _ in 0..pad {
                        body.insert(at, b'0');
                    }
                } else {
                    let mut p = vec![b' '; pad];
                    p.extend(body);
                    body = p;
                }
            }
            out.extend(body);
        }
        Ok(out)
    }

    fn sound_playing(&self, n: i32) -> bool {
        match self.sounds.get(&n) {
            Some(s) => match s.started {
                Some(t0) => s.looped || s.len <= 0.0 || self.clock - t0 < s.len,
                None => false,
            },
            None => false,
        }
    }

    fn vol(v: u32) -> f32 {
        (v.min(100) as f32 / 100.0).powi(2)
    }
}

fn strcmp_i(a: &[u8], b: &[u8], fold: bool) -> i32 {
    let n = a.len().max(b.len());
    for i in 0..=n {
        let (mut x, mut y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if fold {
            x = x.to_ascii_lowercase();
            y = y.to_ascii_lowercase();
        }
        if x != y {
            return x as i32 - y as i32;
        }
        if x == 0 {
            break;
        }
    }
    0
}

/// The byte offset of a character in a Shift_JIS string.
fn mbs_find(s: &[u8], ch: u32) -> Option<usize> {
    let mut i = 0;
    while i < s.len() {
        let two = crate::c::lex::is_lead(s[i]) && i + 1 < s.len();
        let c = if two {
            ((s[i] as u32) << 8) | s[i + 1] as u32
        } else {
            s[i] as u32
        };
        if c == ch {
            return Some(i);
        }
        i += if two { 2 } else { 1 };
    }
    None
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xedb8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

impl State {
    fn call_native(&mut self, vm: &mut Vm, id: u16, a: &[u32]) -> Result<Native, String> {
        let name = vm.natives[id as usize].clone();
        let arg = |k: usize| a.get(k).copied().unwrap_or(0);
        let i = |k: usize| arg(k) as i32;
        if name != "getcount" {
            self.polls = 0;
        }
        let v = match name.as_str() {
            // ---- programs
            "call" | "jump" => {
                let file = self.path(vm, arg(0))?;
                // beside the main program, or as it is
                let near = format!("{}{file}", self.base);
                let path = if self.arc.has(&near) || !self.arc.has(&file) {
                    near
                } else {
                    file
                };
                #[cfg(not(target_arch = "wasm32"))]
                if std::env::var_os("ESCUDE_CALLS").is_some() {
                    self.log.push(format!("{name} {path}"));
                }
                let start = self.load_program(vm, &path)?;
                return Ok(Native::Call(start));
            }
            "exit" => {
                self.save_sysfiles(vm);
                return Ok(Native::Exit);
            }
            "sleep" => {
                self.clock += arg(0) as f64;
                if self.clock > self.real {
                    return Ok(Native::Yield(0));
                }
                Native::Value(0)
            }
            // ---- time
            "getcount" => {
                self.polls += 1;
                if self.polls > 8 {
                    self.clock += 1.0;
                }
                let base = self.counters.get(&i(0)).copied().unwrap_or(0.0);
                let v = (self.clock - base).max(0.0) as u32;
                if self.polls > 8 && self.clock > self.real {
                    return Ok(Native::Yield(v));
                }
                Native::Value(v)
            }
            "setcount" => {
                self.counters.insert(i(0), self.clock - i(1) as f64);
                b(true)
            }
            "gettime" => {
                write_systemtime(vm, arg(0), self.wall)?;
                b(true)
            }
            "fps" => Native::Value(60),
            // ---- display
            "update" => {
                self.present(vm);
                if self.updated {
                    return Ok(Native::Yield(1));
                }
                self.updated = true;
                b(true)
            }
            "apage" => {
                let old = self.apage;
                self.apage = i(0);
                Native::Value(old as u32)
            }
            "vpage" => {
                let old = self.vpage;
                self.vpage = i(0);
                Native::Value(old as u32)
            }
            "setorg" => {
                self.org = (i(0), i(1));
                b(true)
            }
            // ---- planes
            "ginit" => b(true),
            "galloc" => {
                let n = self.plane_arg(vm, arg(0))?;
                self.planes.alloc(vm, n, i(1), i(2), arg(3));
                b(true)
            }
            "gfree" => {
                // the screen stays (games free handles they never allocated, which are 0)
                let n = i(0);
                b(n != 0 && self.planes.free(vm, n))
            }
            "gload" => {
                let n = self.plane_arg(vm, arg(0))?;
                let path = self.path(vm, arg(1))?;
                b(self.load_picture(vm, n, &path)?)
            }
            "dload" => {
                let path = self.path(vm, arg(3))?;
                let tmp = -77;
                if self.load_picture(vm, tmp, &path)? {
                    let r = rd_rect(vm, arg(4))?;
                    self.planes
                        .copy(vm, i(0), i(1), i(2), tmp, r, &Mode::NORMAL);
                    self.planes.free(vm, tmp);
                    b(true)
                } else {
                    b(false)
                }
            }
            "gclip" => {
                let r = rd_rect(vm, arg(1))?;
                if let Some(p) = self.planes.map.get_mut(&i(0)) {
                    p.clip = r;
                }
                b(true)
            }
            "gattach" => {
                if let Some(p) = self.planes.map.get_mut(&i(0)) {
                    p.alpha = Some(i(1));
                }
                b(true)
            }
            "gdetach" => {
                if let Some(p) = self.planes.map.get_mut(&i(0)) {
                    p.alpha = None;
                }
                b(true)
            }
            "ginfo" => {
                let Some(p) = self.planes.get(i(0)).cloned() else {
                    return Ok(b(false));
                };
                let o = arg(1);
                vm.w32(o, p.w as u32)?;
                vm.w32(o + 4, p.h as u32)?;
                vm.w32(o + 8, p.depth)?;
                vm.w32(o + 12, p.addr + (p.h as u32 - 1) * p.stride)?;
                vm.w32(o + 16, (p.stride as i32).wrapping_neg() as u32)?;
                vm.w32(o + 20, 0)?;
                b(true)
            }
            "getpixel" => {
                let Some(p) = self.planes.get(i(0)) else {
                    return Ok(Native::Value(0));
                };
                if i(1) < 0 || i(2) < 0 || i(1) >= p.w || i(2) >= p.h {
                    return Ok(Native::Value(0));
                }
                let px = self.planes.read(vm, i(0), [i(1), i(2), i(1) + 1, i(2) + 1])[0];
                Native::Value(
                    ((px[3] as u32) << 24)
                        | ((px[2] as u32) << 16)
                        | ((px[1] as u32) << 8)
                        | px[0] as u32,
                )
            }
            "gsetpal" | "ggetpal" => {
                // (plane, index, count, rgbs)
                let set = name == "gsetpal";
                if let Some(p) = self.planes.map.get_mut(&i(0))
                    && let Some(pal) = &mut p.pal
                {
                    for k in 0..i(2).max(0) as u32 {
                        let idx = (i(1) as u32 + k) as usize;
                        if idx >= 256 {
                            break;
                        }
                        if set {
                            pal[idx] = vm.r32(arg(3) + 4 * k)? & 0xffffff;
                        } else {
                            vm.w32(arg(3) + 4 * k, pal[idx])?;
                        }
                    }
                }
                Native::Value(i(2) as u32)
            }
            // gfill(plane, rect, colour); in later engines gfill(plane, x, y, w, h, rgba) and
            // gclear(plane, rect, rgba)
            "gfill" | "gclear" => {
                let (r, c) = if a.len() >= 6 {
                    (Some([i(1), i(2), i(1) + i(3), i(2) + i(4)]), arg(5))
                } else {
                    (rd_rect(vm, arg(1))?, arg(2))
                };
                self.planes.fill(vm, i(0), r, c);
                b(true)
            }
            "gcopy" => {
                let r = rd_rect(vm, arg(4))?;
                let m = rd_mode(vm, arg(5))?;
                self.planes.copy(vm, i(0), i(1), i(2), i(3), r, &m);
                b(true)
            }
            "gblit" | "smooth" | "rotate" => {
                // (dst, dst rect, src, src rect, mode | palette, rotation)
                let Some(dp) = self.planes.get(i(0)) else {
                    return Ok(b(false));
                };
                let Some(sp) = self.planes.get(i(2)) else {
                    return Ok(b(false));
                };
                let dr = rd_rect(vm, arg(1))?.unwrap_or([0, 0, dp.w, dp.h]);
                let sr = rd_rect(vm, arg(3))?.unwrap_or([0, 0, sp.w, sp.h]);
                let m = if name == "gblit" {
                    rd_mode(vm, arg(4))?
                } else {
                    Mode::NORMAL
                };
                let rot = match name.as_str() {
                    "gblit" => i(5),
                    "rotate" => i(4),
                    _ => 0,
                };
                if empty(dr) || empty(sr) {
                    return Ok(b(true));
                }
                let (sx, sy) = (
                    (sr[2] - sr[0]) as f32 / (dr[2] - dr[0]) as f32,
                    (sr[3] - sr[1]) as f32 / (dr[3] - dr[1]) as f32,
                );
                let (cx, cy) = ((dr[0] + dr[2]) as f32 / 2.0, (dr[1] + dr[3]) as f32 / 2.0);
                let t = (rot.rem_euclid(2048)) as f32 * std::f32::consts::TAU / 2048.0;
                let (s, c) = t.sin_cos();
                let scx = (sr[0] + sr[2]) as f32 / 2.0;
                let scy = (sr[1] + sr[3]) as f32 / 2.0;
                let mut inv = [c * sx, -s * sx, 0.0, s * sy, c * sy, 0.0];
                inv[2] = scx - (inv[0] * cx + inv[1] * cy);
                inv[5] = scy - (inv[3] * cx + inv[4] * cy);
                let area = if rot.rem_euclid(2048) == 0 {
                    dr
                } else {
                    let rr =
                        (((dr[2] - dr[0]).pow(2) + (dr[3] - dr[1]).pow(2)) as f32).sqrt() / 2.0;
                    [
                        (cx - rr) as i32,
                        (cy - rr) as i32,
                        (cx + rr) as i32 + 1,
                        (cy + rr) as i32 + 1,
                    ]
                };
                self.planes
                    .affine(vm, i(0), area, i(2), sr, inv, &m, name == "smooth");
                b(true)
            }
            "freemap" => {
                // (dst, 4 points, src, src rect, palette): an affine map from three corners
                let Some(sp) = self.planes.get(i(2)) else {
                    return Ok(b(false));
                };
                let sr = rd_rect(vm, arg(3))?.unwrap_or([0, 0, sp.w, sp.h]);
                let pt = arg(1);
                let mut q = [(0f32, 0f32); 4];
                for (k, qk) in q.iter_mut().enumerate() {
                    *qk = (
                        vm.ri32(pt + 8 * k as u32)? as f32,
                        vm.ri32(pt + 8 * k as u32 + 4)? as f32,
                    );
                }
                let (w, h) = ((sr[2] - sr[0]) as f32, (sr[3] - sr[1]) as f32);
                // forward: p = q0 + (q1-q0) u/w + (q3-q0) v/h  (q: top left, top right, bottom right, bottom left)
                let ax = (q[1].0 - q[0].0) / w;
                let ay = (q[1].1 - q[0].1) / w;
                let bx = (q[3].0 - q[0].0) / h;
                let by = (q[3].1 - q[0].1) / h;
                let det = ax * by - bx * ay;
                if det.abs() < 1e-6 {
                    return Ok(b(true));
                }
                let (ia, ib, ic, id) = (by / det, -bx / det, -ay / det, ax / det);
                let inv = [
                    ia,
                    ib,
                    -(ia * q[0].0 + ib * q[0].1) + sr[0] as f32,
                    ic,
                    id,
                    -(ic * q[0].0 + id * q[0].1) + sr[1] as f32,
                ];
                let xs = q.iter().map(|p| p.0);
                let ys = q.iter().map(|p| p.1);
                let area = [
                    xs.clone().fold(f32::MAX, f32::min).floor() as i32,
                    ys.clone().fold(f32::MAX, f32::min).floor() as i32,
                    xs.fold(f32::MIN, f32::max).ceil() as i32,
                    ys.fold(f32::MIN, f32::max).ceil() as i32,
                ];
                self.planes
                    .affine(vm, i(0), area, i(2), sr, inv, &Mode::NORMAL, false);
                b(true)
            }
            "mosaic" | "blur" | "gradation" => {
                // (dst, x, y, src, src rect, ...)
                let Some(sp) = self.planes.get(i(3)) else {
                    return Ok(b(false));
                };
                let sr = intersect(
                    rd_rect(vm, arg(4))?.unwrap_or([0, 0, sp.w, sp.h]),
                    [0, 0, sp.w, sp.h],
                );
                if empty(sr) {
                    return Ok(b(true));
                }
                let mut px = self.planes.read(vm, i(3), sr);
                let (w, h) = ((sr[2] - sr[0]) as usize, (sr[3] - sr[1]) as usize);
                if name == "mosaic" {
                    let (bx, by) = (i(5).max(1) as usize, i(6).max(1) as usize);
                    for y0 in (0..h).step_by(by) {
                        for x0 in (0..w).step_by(bx) {
                            let c = px[y0 * w + x0];
                            for y in y0..(y0 + by).min(h) {
                                for x in x0..(x0 + bx).min(w) {
                                    px[y * w + x] = c;
                                }
                            }
                        }
                    }
                } else if name == "blur" {
                    let r = i(5).max(0) as usize;
                    if r > 0 {
                        let src = px.clone();
                        for y in 0..h {
                            for x in 0..w {
                                let mut sum = [0u32; 4];
                                let mut n = 0;
                                for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                                    for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                                        let p = src[yy * w + xx];
                                        for k in 0..4 {
                                            sum[k] += p[k] as u32;
                                        }
                                        n += 1;
                                    }
                                }
                                px[y * w + x] = sum.map(|s| (s / n) as u8);
                            }
                        }
                    }
                }
                let dr = [i(1), i(2), i(1) + w as i32, i(2) + h as i32];
                let Some(dp) = self.planes.get(i(0)) else {
                    return Ok(b(false));
                };
                let c = intersect(dr, dp.bounds());
                if !empty(c) {
                    let mut part = Vec::new();
                    for y in c[1]..c[3] {
                        for x in c[0]..c[2] {
                            part.push(px[(y - dr[1]) as usize * w + (x - dr[0]) as usize]);
                        }
                    }
                    self.planes
                        .compose(vm, i(0), c, &part, &[], 24, &Mode::NORMAL, None);
                }
                b(true)
            }
            "monotone" | "noise" | "slice" | "fire" | "scroll" | "tiled" => {
                self.warn(format!("{name}: not drawn"));
                b(true)
            }
            "ef_init" | "ef_free" | "ef_rotate" | "ef_tunnel" | "ef_land" | "ef_ball"
            | "ef_whirl" | "ef_raster" | "ef_blob" | "ef_water" => {
                self.warn(format!("{name}: not drawn"));
                b(true)
            }
            // ---- sprites
            "sp_flag" => {
                let old = self.sp_flag;
                self.sp_flag = arg(0) & 0xff;
                Native::Value(old)
            }
            "sp_init" => {
                let mask = arg(0);
                let l = self.spr.clone();
                let mut keep = Vec::new();
                for sp in std::mem::take(&mut self.sprites) {
                    let pri = vm.r8(sp + l.at("priority"))? as u32;
                    if mask & (1 << (pri & 31)) != 0 {
                        vm.w8(sp + l.at("type"), 0)?;
                        vm.free(sp);
                    } else {
                        keep.push(sp);
                    }
                }
                self.sprites = keep;
                b(true)
            }
            "sp_setbg" => {
                let old = self.sp_bg;
                self.sp_bg = i(0);
                Native::Value(old as u32)
            }
            "sp_getbg" => Native::Value(self.sp_bg as u32),
            "sp_bgorg" => {
                self.sp_bgorg = (i(0), i(1));
                b(true)
            }
            "sp_org" => {
                for p in 0..32 {
                    if arg(0) & (1 << p) != 0 {
                        self.sp_org[p] = (i(1), i(2));
                    }
                }
                b(true)
            }
            "sp_redraw" | "sp_sort" => {
                if name == "sp_sort" {
                    let l = self.spr.clone();
                    let mut keyed = Vec::new();
                    for (k, &sp) in self.sprites.iter().enumerate() {
                        let pri = vm.r8(sp + l.at("priority"))? as u32;
                        let key = if arg(0) & (1 << (pri & 31)) != 0 {
                            vm.ri32(sp + l.at("sort"))?
                        } else {
                            0
                        };
                        keyed.push((pri, key, k, sp));
                    }
                    // only sprites of the given priorities move, among themselves
                    let mut sorted: Vec<_> = keyed
                        .iter()
                        .filter(|e| arg(0) & (1 << (e.0 & 31)) != 0)
                        .cloned()
                        .collect();
                    sorted.sort_by_key(|e| (e.1, e.2));
                    let mut it = sorted.into_iter();
                    self.sprites = keyed
                        .iter()
                        .map(|e| {
                            if arg(0) & (1 << (e.0 & 31)) != 0 {
                                it.next().unwrap().3
                            } else {
                                e.3
                            }
                        })
                        .collect();
                }
                b(true)
            }
            "sp_set" => {
                let l = self.spr.clone();
                let size = l.at("<size>").max(140);
                let sp = vm.malloc(size);
                vm.w8(sp + l.at("type"), i(1) as u8)?;
                vm.w8(sp + l.at("priority"), (i(0) & 31) as u8)?;
                vm.w8(sp + l.at("flag"), self.sp_flag as u8)?;
                vm.w32(sp + l.at("page"), i(4) as u32)?;
                vm.w32(sp + l.at("x"), i(2) as u32)?;
                vm.w32(sp + l.at("y"), i(3) as u32)?;
                vm.w32(sp + l.at("zx"), 65536)?;
                vm.w32(sp + l.at("zy"), 65536)?;
                vm.w32(sp + l.at("real_zx"), 65536)?;
                vm.w32(sp + l.at("real_zy"), 65536)?;
                let rc = match rd_rect(vm, arg(5))? {
                    Some(r) => r,
                    None => self
                        .planes
                        .get(i(4))
                        .map_or([0, 0, 0, 0], |p| [0, 0, p.w, p.h]),
                };
                wr_rect(vm, sp + l.at("rc"), rc)?;
                let m = rd_mode(vm, arg(6))?;
                let bm = sp + l.at("bm");
                vm.w8(bm, m.mode)?;
                vm.w8(bm + 1, m.mask)?;
                vm.w8(bm + 2, m.level)?;
                vm.w8(bm + 3, m.range)?;
                vm.w32(bm + 4, m.trans)?;
                vm.w32(bm + 8, m.color)?;
                self.sprites.push(sp);
                Native::Value(sp)
            }
            "sp_reset" => {
                let l = self.spr.clone();
                if i(0) == -1 {
                    for sp in std::mem::take(&mut self.sprites) {
                        vm.w8(sp + l.at("type"), 0)?;
                        vm.free(sp);
                    }
                } else if let Some(k) = self.sprites.iter().position(|&s| s == arg(0)) {
                    let sp = self.sprites.remove(k);
                    self.unlink(vm, sp)?;
                    if l.0.contains_key("child_count") {
                        let kids = vm.r32(sp + l.at("child"))?;
                        if kids != 0 {
                            vm.free(kids);
                        }
                    }
                    // children lose their parent
                    for &c in &self.sprites {
                        if vm.r32(c + l.at("parent"))? == sp {
                            vm.w32(c + l.at("parent"), 0)?;
                        }
                    }
                    vm.w8(sp + l.at("type"), 0)?;
                    vm.free(sp);
                }
                b(true)
            }
            "sp_link" => {
                let (parent, child) = (arg(0), arg(1));
                if child != 0 {
                    self.unlink(vm, child)?;
                    let l = self.spr.clone();
                    vm.w32(child + l.at("parent"), parent)?;
                    if parent != 0 {
                        if l.0.contains_key("child_count") {
                            // a parent keeps an array of its children
                            let mut kids = self.children(vm, parent)?;
                            kids.push(child);
                            self.set_children(vm, parent, &kids)?;
                        } else {
                            vm.w32(parent + l.at("child"), child)?;
                        }
                    }
                }
                b(true)
            }
            "sp_unlink" => {
                if arg(0) != 0 {
                    self.unlink(vm, arg(0))?;
                }
                b(true)
            }
            "sp_draw" => {
                self.sp_draw(vm, arg(0), arg(1))?;
                b(true)
            }
            "sp_rect" => {
                let l = self.spr.clone();
                let r = rd_rect(vm, arg(0) + l.at("update"))?.unwrap_or([0; 4]);
                wr_rect(vm, arg(1), r)?;
                b(true)
            }
            // ---- text
            "font_set" => {
                let n = (i(0) as usize).min(7);
                let p = arg(1);
                let size = vm.ri32(p + 32)?;
                let style = vm.r32(p + 40)?;
                self.fonts[n] = FontStyle { size, style };
                b(true)
            }
            "font_get" => {
                let n = (i(0) as usize).min(7);
                let p = arg(1);
                vm.bytes_mut(p, 44)?.fill(0);
                vm.w32(p + 32, self.fonts[n].size as u32)?;
                vm.w32(p + 40, self.fonts[n].style)?;
                b(true)
            }
            "font_use" => {
                self.font_now = (i(0) as usize).min(7);
                b(true)
            }
            "font_list" => Native::Value(0),
            "gtext" => {
                let text = vm.cstr(arg(4))?;
                self.gtext(vm, i(0), i(1), i(2), arg(3), &text)?;
                b(true)
            }
            // ---- sounds
            "snd_load" | "snd_loads" => {
                let path = self.path(vm, arg(1))?;
                match self.read_game_file(&path) {
                    Some(d) => {
                        let len = ogg_ms(&d);
                        let vol = self.sounds.get(&i(0)).map_or(100, |s| s.volume);
                        self.stop_sound(i(0));
                        self.sounds.insert(
                            i(0),
                            Sound {
                                data: Rc::new(d),
                                len,
                                volume: vol,
                                started: None,
                                looped: false,
                                paused_at: None,
                            },
                        );
                        b(true)
                    }
                    None => {
                        self.warn(format!("snd_load: no {path}"));
                        b(false)
                    }
                }
            }
            "snd_free" => {
                self.stop_sound(i(0));
                self.sounds.remove(&i(0));
                b(true)
            }
            "snd_play" => {
                let clock = self.clock;
                if let Some(s) = self.sounds.get_mut(&i(0)) {
                    s.started = Some(clock);
                    s.looped = arg(1) != 0;
                    s.paused_at = None;
                    let ev = SoundEvent {
                        channel: i(0),
                        op: "play",
                        data: Some(s.data.clone()),
                        volume: State::vol(s.volume),
                        repeat: s.looped,
                    };
                    self.events.push(ev);
                    b(true)
                } else {
                    b(false)
                }
            }
            "snd_stop" => {
                self.stop_sound(i(0));
                b(true)
            }
            "snd_pause" => {
                let clock = self.clock;
                if let Some(s) = self.sounds.get_mut(&i(0))
                    && s.started.is_some()
                {
                    s.paused_at = Some(clock);
                    self.events.push(SoundEvent {
                        channel: i(0),
                        op: "pause",
                        data: None,
                        volume: 0.0,
                        repeat: false,
                    });
                }
                b(true)
            }
            "snd_replay" => {
                let clock = self.clock;
                if let Some(s) = self.sounds.get_mut(&i(0))
                    && let Some(p) = s.paused_at.take()
                {
                    if let Some(t0) = s.started {
                        s.started = Some(t0 + clock - p);
                    }
                    self.events.push(SoundEvent {
                        channel: i(0),
                        op: "resume",
                        data: None,
                        volume: 0.0,
                        repeat: false,
                    });
                }
                b(true)
            }
            "snd_setvol" => {
                let v = arg(1).min(100);
                if let Some(s) = self.sounds.get_mut(&i(0)) {
                    if s.volume != v {
                        s.volume = v;
                        if s.started.is_some() {
                            self.events.push(SoundEvent {
                                channel: i(0),
                                op: "volume",
                                data: None,
                                volume: State::vol(v),
                                repeat: false,
                            });
                        }
                    }
                } else {
                    self.sounds.insert(
                        i(0),
                        Sound {
                            data: Rc::new(Vec::new()),
                            len: 0.0,
                            volume: v,
                            started: None,
                            looped: false,
                            paused_at: None,
                        },
                    );
                }
                b(true)
            }
            "snd_getvol" => Native::Value(self.sounds.get(&i(0)).map_or(100, |s| s.volume)),
            "snd_isplay" => b(self.sound_playing(i(0))),
            "snd_getpos" => {
                let v = match self.sounds.get(&i(0)).and_then(|s| s.started) {
                    Some(t0) => (self.clock - t0).max(0.0) as u32,
                    None => 0,
                };
                Native::Value(v)
            }
            "snd_getlen" => Native::Value(self.sounds.get(&i(0)).map_or(0, |s| s.len as u32)),
            "snd_gettext" => {
                vm.w8(arg(1), 0)?;
                b(false)
            }
            n if n.starts_with("snd_") || n.starts_with("midi_") || n.starts_with("cd_") => b(true),
            n if n.starts_with("media_") => b(false),
            // ---- input
            "getkey" => {
                let p = arg(0);
                for k in 0..256 {
                    let down = self.keys[k] || self.latched_keys[k];
                    vm.w8(p + k as u32, if down { 0x80 } else { 0 })?;
                }
                self.latched_keys = [false; 256];
                b(true)
            }
            "getpoint" | "getmouse" => {
                let p = arg(0);
                let bt = self.buttons | self.latched_buttons;
                self.latched_buttons = 0;
                vm.w32(p, self.mouse.0 as u32)?;
                vm.w32(p + 4, self.mouse.1 as u32)?;
                vm.w32(p + 8, 0)?;
                vm.w32(p + 12, bt & 7)?;
                Native::Value(1)
            }
            "getjoy" => b(false),
            "cur_set" | "cur_clip" | "cur_setpos" => Native::Value(0),
            "key_init" => Native::Value(0),
            "key_get" => Native::Value(0),
            // ---- files
            "pack_open" | "fopen" => {
                let path = self.path(vm, arg(0))?;
                let mode = if name == "fopen" {
                    vm.cstr(arg(1))?
                } else {
                    b"rb".to_vec()
                };
                let writing = mode.contains(&b'w') || mode.contains(&b'a');
                let data = if writing {
                    if mode.contains(&b'a') {
                        self.read_game_file(&path).unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                } else {
                    match self.read_game_file(&path) {
                        Some(d) => d,
                        None => return Ok(Native::Value(0)),
                    }
                };
                let h = self.next_handle;
                self.next_handle += 1;
                let pos = if mode.contains(&b'a') { data.len() } else { 0 };
                self.open.insert(
                    h,
                    OpenFile {
                        data,
                        pos,
                        write: writing.then_some(path),
                    },
                );
                Native::Value(h)
            }
            "pack_close" | "fclose" => {
                if let Some(f) = self.open.remove(&arg(0))
                    && let Some(path) = f.write
                {
                    self.write_file(&path, &f.data);
                }
                b(true)
            }
            "pack_read" | "fread" => {
                let (h, buf, n) = if name == "fread" {
                    (arg(3), arg(0), arg(1).wrapping_mul(arg(2)))
                } else {
                    (arg(0), arg(1), arg(2))
                };
                let Some(f) = self.open.get_mut(&h) else {
                    return Ok(Native::Value(0));
                };
                let n = (n as usize).min(f.data.len().saturating_sub(f.pos));
                vm.bytes_mut(buf, n as u32)?
                    .copy_from_slice(&f.data[f.pos..f.pos + n]);
                f.pos += n;
                let got = if name == "fread" && arg(1) > 0 {
                    n as u32 / arg(1)
                } else {
                    n as u32
                };
                Native::Value(got)
            }
            "fwrite" => {
                let n = arg(1).wrapping_mul(arg(2));
                let bytes = vm.bytes(arg(0), n)?.to_vec();
                let Some(f) = self.open.get_mut(&arg(3)) else {
                    return Ok(Native::Value(0));
                };
                let end = f.pos + bytes.len();
                if f.data.len() < end {
                    f.data.resize(end, 0);
                }
                f.data[f.pos..end].copy_from_slice(&bytes);
                f.pos = end;
                Native::Value(arg(2))
            }
            "pack_seek" | "fseek" => {
                let Some(f) = self.open.get_mut(&arg(0)) else {
                    return Ok(Native::Value(u32::MAX));
                };
                let base = match i(2) {
                    1 => f.pos as i64,
                    2 => f.data.len() as i64,
                    _ => 0,
                };
                f.pos = (base + i(1) as i64).clamp(0, f.data.len() as i64) as usize;
                Native::Value(if name == "fseek" { 0 } else { f.pos as u32 })
            }
            "ftell" => Native::Value(self.open.get(&arg(0)).map_or(0, |f| f.pos as u32)),
            "pack_size" => Native::Value(self.open.get(&arg(0)).map_or(0, |f| f.data.len() as u32)),
            "fgets" => {
                let Some(f) = self.open.get_mut(&arg(2)) else {
                    return Ok(b(false));
                };
                if f.pos >= f.data.len() {
                    return Ok(b(false));
                }
                let max = (i(1).max(1) - 1) as usize;
                let mut line = Vec::new();
                while f.pos < f.data.len() && line.len() < max {
                    let c = f.data[f.pos];
                    f.pos += 1;
                    line.push(c);
                    if c == b'\n' {
                        break;
                    }
                }
                vm.write_cstr(arg(0), &line)?;
                b(true)
            }
            "pack_save" => {
                let path = self.path(vm, arg(0))?;
                let bytes = vm.bytes(arg(1), arg(2))?.to_vec();
                self.write_file(&path, &bytes);
                b(true)
            }
            "sysfile" => {
                let path = self.path(vm, arg(0))?;
                let (p, n) = (arg(1), arg(2));
                let mut ok = false;
                if let Some(d) = self.read_game_file(&path) {
                    let k = d.len().min(n as usize);
                    vm.bytes_mut(p, k as u32)?.copy_from_slice(&d[..k]);
                    ok = true;
                }
                let now = vm.bytes(p, n)?.to_vec();
                self.sysfiles.retain(|s| s.0 != path);
                self.sysfiles
                    .push((path, p, n, if ok { now } else { Vec::new() }));
                b(ok)
            }
            "sysio" => {
                if arg(0) != 0 {
                    self.save_sysfiles(vm);
                }
                b(true)
            }
            "file_check" | "file_size" => {
                let path = self.path(vm, arg(0))?;
                let d = self.read_game_file(&path);
                if name == "file_size" {
                    Native::Value(d.map_or(0, |d| d.len() as u32))
                } else {
                    b(d.is_some())
                }
            }
            "file_delete" => {
                let path = self.path(vm, arg(0))?;
                self.write_file(&path, &[]);
                b(true)
            }
            "file_move" | "file_copy" => {
                let from = self.path(vm, arg(0))?;
                let to = self.path(vm, arg(1))?;
                if let Some(d) = self.read_game_file(&from) {
                    self.write_file(&to, &d);
                    if name == "file_move" {
                        self.write_file(&from, &[]);
                    }
                    b(true)
                } else {
                    b(false)
                }
            }
            "file_gettime" => {
                let path = self.path(vm, arg(0))?;
                if self.read_game_file(&path).is_none() {
                    return Ok(b(false));
                }
                let rest = path.strip_prefix(DOCS).unwrap_or(&path).replace('\\', "/");
                let wall = self.wall;
                let t = self.times().get(&rest).copied().unwrap_or(wall);
                for k in 1..4 {
                    if arg(k) != 0 {
                        write_systemtime(vm, arg(k), t)?;
                    }
                }
                b(true)
            }
            "file_getattr" => Native::Value(0x80),
            "file_setattr" | "file_settime" => b(true),
            "dir_get" => {
                vm.write_cstr(arg(1), DOCS.as_bytes())?;
                Native::Value(DOCS.len() as u32)
            }
            "dir_getcur" => {
                vm.write_cstr(arg(0), b".\\")?;
                b(true)
            }
            "dir_create" | "dir_setcur" | "dir_delete" => b(true),
            "find_first" => Native::Value(u32::MAX),
            "find_next" | "find_close" => b(false),
            "ini_geti" => Native::Value(arg(2)),
            "ini_gets" => {
                let d = if arg(2) == 0 {
                    Vec::new()
                } else {
                    vm.cstr(arg(2))?
                };
                let n = (i(4).max(1) - 1) as usize;
                vm.write_cstr(arg(3), &d[..d.len().min(n)])?;
                Native::Value(d.len().min(n) as u32)
            }
            "ini_seti" | "ini_sets" => b(true),
            // ---- memory
            "malloc" => Native::Value(vm.malloc(arg(0))),
            "free" => b(vm.free(arg(0))),
            "realloc" => {
                let old = arg(0);
                let n = vm.malloc(arg(1));
                if old != 0 {
                    let k = vm.block_size(old).unwrap_or(0).min(arg(1));
                    let bytes = vm.bytes(old, k)?.to_vec();
                    vm.bytes_mut(n, k)?.copy_from_slice(&bytes);
                    vm.free(old);
                }
                Native::Value(n)
            }
            "mload" => {
                let path = self.path(vm, arg(0))?;
                match self.read_game_file(&path) {
                    Some(d) => {
                        let start = (arg(1) as usize).min(d.len());
                        let n = if arg(2) == 0 {
                            d.len() - start
                        } else {
                            (arg(2) as usize).min(d.len() - start)
                        };
                        let p = vm.malloc(n as u32);
                        vm.bytes_mut(p, n as u32)?
                            .copy_from_slice(&d[start..start + n]);
                        Native::Value(p)
                    }
                    None => Native::Value(0),
                }
            }
            "memcpy" | "memmove" => {
                let n = arg(2);
                let s = vm.bytes(arg(1), n)?.to_vec();
                vm.bytes_mut(arg(0), n)?.copy_from_slice(&s);
                Native::Value(arg(0))
            }
            "memset" => {
                vm.bytes_mut(arg(0), arg(2))?.fill(arg(1) as u8);
                Native::Value(arg(0))
            }
            "memcmp" => {
                let x = vm.bytes(arg(0), arg(2))?;
                let y = vm.bytes(arg(1), arg(2))?;
                Native::Value(match x.cmp(y) {
                    std::cmp::Ordering::Less => -1i32 as u32,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })
            }
            "memswap" => {
                let n = arg(2);
                let x = vm.bytes(arg(0), n)?.to_vec();
                let y = vm.bytes(arg(1), n)?.to_vec();
                vm.bytes_mut(arg(0), n)?.copy_from_slice(&y);
                vm.bytes_mut(arg(1), n)?.copy_from_slice(&x);
                Native::Value(0)
            }
            "crc32" => Native::Value(crc32(vm.bytes(arg(0), arg(1))?)),
            "crc16" => Native::Value(crc32(vm.bytes(arg(0), arg(1))?) & 0xffff),
            // ---- strings
            "strlen" | "mbslen" => {
                let s = vm.cstr(arg(0))?;
                if name == "mbslen" {
                    Native::Value(nexas::sjis::chars(&s).len() as u32)
                } else {
                    Native::Value(s.len() as u32)
                }
            }
            "strcpy" | "mbscpy" => {
                let s = vm.cstr(arg(1))?;
                vm.write_cstr(arg(0), &s)?;
                Native::Value(arg(0))
            }
            "strncpy" => {
                let mut s = vm.cstr(arg(1))?;
                let n = arg(2) as usize;
                s.truncate(n);
                let k = s.len();
                vm.bytes_mut(arg(0), k as u32)?.copy_from_slice(&s);
                if k < n {
                    vm.bytes_mut(arg(0) + k as u32, (n - k) as u32)?.fill(0);
                }
                Native::Value(arg(0))
            }
            "strcat" | "mbscat" => {
                let d = vm.cstr(arg(0))?;
                let s = vm.cstr(arg(1))?;
                vm.write_cstr(arg(0) + d.len() as u32, &s)?;
                Native::Value(arg(0))
            }
            "strcmp" | "mbscmp" => {
                Native::Value(strcmp_i(&vm.cstr(arg(0))?, &vm.cstr(arg(1))?, false) as u32)
            }
            "strcmpi" | "mbscmpi" | "stricmp" => {
                Native::Value(strcmp_i(&vm.cstr(arg(0))?, &vm.cstr(arg(1))?, true) as u32)
            }
            "strncmp" => {
                let mut x = vm.cstr(arg(0))?;
                let mut y = vm.cstr(arg(1))?;
                x.truncate(arg(2) as usize);
                y.truncate(arg(2) as usize);
                Native::Value(strcmp_i(&x, &y, false) as u32)
            }
            "strchr" | "mbschr" => {
                let s = vm.cstr(arg(0))?;
                let found = if name == "mbschr" {
                    mbs_find(&s, arg(1))
                } else {
                    s.iter().position(|&c| c == arg(1) as u8)
                };
                Native::Value(found.map_or(0, |k| arg(0) + k as u32))
            }
            "strstr" => {
                let s = vm.cstr(arg(0))?;
                let t = vm.cstr(arg(1))?;
                let k = if t.is_empty() {
                    Some(0)
                } else {
                    s.windows(t.len()).position(|w| w == &t[..])
                };
                Native::Value(k.map_or(0, |k| arg(0) + k as u32))
            }
            "strlwr" | "strupr" | "mbslwr" | "mbsupr" => {
                let mut s = vm.cstr(arg(0))?;
                let mut k = 0;
                while k < s.len() {
                    if crate::c::lex::is_lead(s[k]) {
                        k += 2;
                        continue;
                    }
                    s[k] = if name.ends_with("lwr") {
                        s[k].to_ascii_lowercase()
                    } else {
                        s[k].to_ascii_uppercase()
                    };
                    k += 1;
                }
                vm.write_cstr(arg(0), &s)?;
                Native::Value(arg(0))
            }
            "strsub" | "mbssub" => {
                let s = vm.cstr(arg(1))?;
                let part: Vec<u8> = if name == "mbssub" {
                    let cs = nexas::sjis::chars(&s);
                    let from = (arg(2) as usize).min(cs.len());
                    let to = (from + arg(3) as usize).min(cs.len());
                    cs[from..to].concat()
                } else {
                    let from = (arg(2) as usize).min(s.len());
                    let to = (from + arg(3) as usize).min(s.len());
                    s[from..to].to_vec()
                };
                vm.write_cstr(arg(0), &part)?;
                Native::Value(arg(0))
            }
            "strdup" | "strndup" => {
                let mut s = vm.cstr(arg(0))?;
                if name == "strndup" {
                    s.truncate(arg(1) as usize);
                }
                let p = vm.malloc(s.len() as u32 + 1);
                vm.write_cstr(p, &s)?;
                Native::Value(p)
            }
            "strclen" => Native::Value(nexas::sjis::chars(&vm.cstr(arg(0))?).len() as u32),
            "sprintf" | "log_out" | "gtextf" => {
                let (dst, fmt_at, rest) = match name.as_str() {
                    "sprintf" => (arg(0), arg(1), &a[2.min(a.len())..]),
                    "gtextf" => (0, arg(4), &a[5.min(a.len())..]),
                    _ => (0, arg(0), &a[1.min(a.len())..]),
                };
                let fmt = vm.cstr(fmt_at)?;
                let s = self.format(vm, &fmt, rest)?;
                match name.as_str() {
                    "sprintf" => {
                        vm.write_cstr(dst, &s)?;
                        Native::Value(s.len() as u32)
                    }
                    "gtextf" => {
                        self.gtext(vm, i(0), i(1), i(2), arg(3), &s)?;
                        b(true)
                    }
                    _ => {
                        self.log.push(nexas::sjis::decode(&s));
                        b(true)
                    }
                }
            }
            "atoi" | "strtol" => {
                let s = vm.cstr(arg(0))?;
                let t = String::from_utf8_lossy(&s);
                let t = t.trim_start();
                let base = if name == "strtol" && arg(2) != 0 {
                    arg(2)
                } else {
                    10
                };
                let (neg, digits) = match t.strip_prefix('-') {
                    Some(r) => (true, r),
                    None => (false, t.strip_prefix('+').unwrap_or(t)),
                };
                let digits = if base == 16 {
                    digits.trim_start_matches("0x").trim_start_matches("0X")
                } else {
                    digits
                };
                let n: String = digits.chars().take_while(|c| c.is_digit(base)).collect();
                let v = i64::from_str_radix(&n, base).unwrap_or(0);
                if name == "strtol" && arg(1) != 0 {
                    let used = (s.len() - t.len()) + usize::from(neg) + n.len();
                    vm.w32(arg(1), arg(0) + used as u32)?;
                }
                Native::Value((if neg { -v } else { v }) as i32 as u32)
            }
            "ftoa" => {
                let s = format!("{}", f(arg(1)));
                vm.write_cstr(arg(0), s.as_bytes())?;
                b(true)
            }
            // ---- maths
            "srand" => {
                self.seed = arg(0);
                Native::Value(0)
            }
            "rand" => {
                self.seed = self.seed.wrapping_mul(214013).wrapping_add(2531011);
                Native::Value((self.seed >> 16) & 0x7fff)
            }
            "random" => {
                self.seed = self.seed.wrapping_mul(214013).wrapping_add(2531011);
                fv(((self.seed >> 16) & 0x7fff) as f32 / 32768.0)
            }
            "abs" => Native::Value(i(0).unsigned_abs()),
            "sgn" => Native::Value(i(0).signum() as u32),
            "_sin" | "_cos" => {
                let t = i(0) as f64 * std::f64::consts::TAU / 2048.0;
                let v = if name == "_sin" { t.sin() } else { t.cos() };
                Native::Value((v * 65536.0).round() as i32 as u32)
            }
            "_atan" => {
                let t = (i(1) as f64).atan2(i(0) as f64);
                Native::Value(
                    ((t * 2048.0 / std::f64::consts::TAU).round() as i32).rem_euclid(2048) as u32,
                )
            }
            "fabs" => fv(f(arg(0)).abs()),
            "sqrt" => fv(f(arg(0)).sqrt()),
            "pow" => fv(f(arg(0)).powf(f(arg(1)))),
            "log" => fv(f(arg(0)).ln()),
            "exp" => fv(f(arg(0)).exp()),
            "floor" => fv(f(arg(0)).floor()),
            "ceil" => fv(f(arg(0)).ceil()),
            "sin" => fv(f(arg(0)).sin()),
            "cos" => fv(f(arg(0)).cos()),
            "tan" => fv(f(arg(0)).tan()),
            "asin" => fv(f(arg(0)).asin()),
            "acos" => fv(f(arg(0)).acos()),
            "atan" => fv(f(arg(0)).atan()),
            "linear" => fv(f(arg(0)) + (f(arg(1)) - f(arg(0))) * f(arg(2))),
            "bezier" => {
                let (p0, p1, p2, t) = (f(arg(0)), f(arg(1)), f(arg(2)), f(arg(3)));
                fv((1.0 - t) * (1.0 - t) * p0 + 2.0 * (1.0 - t) * t * p1 + t * t * p2)
            }
            "neville" => {
                let (p0, p1, p2, t) = (f(arg(0)), f(arg(1)), f(arg(2)), f(arg(3)));
                // through p0 at 0, p1 at 0.5, p2 at 1
                let l0 = 2.0 * (t - 0.5) * (t - 1.0);
                let l1 = -4.0 * t * (t - 1.0);
                let l2 = 2.0 * t * (t - 0.5);
                fv(p0 * l0 + p1 * l1 + p2 * l2)
            }
            "kf_set" => {
                vm.w32(arg(0), arg(1))?;
                vm.w32(arg(0) + 4, arg(2))?;
                Native::Value(0)
            }
            "kf_calc" => Native::Value(0),
            "kf_getindex" | "kf_getkey" => {
                let (p, n, t) = (arg(0), i(1).max(0) as u32, f(arg(2)));
                let key = |k: u32| -> Result<(f32, f32), String> {
                    Ok((f(vm.r32(p + 16 * k)?), f(vm.r32(p + 16 * k + 4)?)))
                };
                let mut idx = 0;
                while idx + 1 < n && key(idx + 1)?.0 <= t {
                    idx += 1;
                }
                if name == "kf_getindex" {
                    return Ok(Native::Value(idx));
                }
                if n == 0 {
                    return Ok(fv(0.0));
                }
                let (t0, v0) = key(idx)?;
                if idx + 1 >= n || i(3) == 0 {
                    return Ok(fv(v0));
                }
                let (t1, v1) = key(idx + 1)?;
                let mut u = if t1 > t0 {
                    ((t - t0) / (t1 - t0)).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                match i(3) {
                    2 => u = u * u,
                    3 => u = 1.0 - (1.0 - u) * (1.0 - u),
                    4..=9 => u = u * u * (3.0 - 2.0 * u),
                    _ => {}
                }
                fv(v0 + (v1 - v0) * u)
            }
            "rgbtohsv" | "hsvtorgb" => {
                let v = arg(0);
                if name == "hsvtorgb" {
                    // 0xHHSSVV (h 0..255)
                    let (h, s, vv) = (
                        ((v >> 16) & 0xff) as f32 / 256.0 * 6.0,
                        ((v >> 8) & 0xff) as f32 / 255.0,
                        (v & 0xff) as f32,
                    );
                    let k = h.floor();
                    let fr = h - k;
                    let (p, q, t) = (
                        vv * (1.0 - s),
                        vv * (1.0 - s * fr),
                        vv * (1.0 - s * (1.0 - fr)),
                    );
                    let (r, g, bb) = match k as i32 % 6 {
                        0 => (vv, t, p),
                        1 => (q, vv, p),
                        2 => (p, vv, t),
                        3 => (p, q, vv),
                        4 => (t, p, vv),
                        _ => (vv, p, q),
                    };
                    Native::Value(((r as u32) << 16) | ((g as u32) << 8) | bb as u32)
                } else {
                    Native::Value(v)
                }
            }
            // ---- rectangles
            "rc_set" | "rc_box" => {
                let r = if name == "rc_box" {
                    [i(1), i(2), i(1) + i(3), i(2) + i(4)]
                } else {
                    [i(1), i(2), i(3), i(4)]
                };
                wr_rect(vm, arg(0), r)?;
                b(true)
            }
            "rc_empty" => {
                wr_rect(vm, arg(0), [0; 4])?;
                b(true)
            }
            "rc_isempty" => b(rd_rect(vm, arg(0))?.is_none_or(empty)),
            "rc_check" => b(!rd_rect(vm, arg(0))?.is_none_or(empty)),
            "rc_copy" => {
                let r = rd_rect(vm, arg(1))?.unwrap_or([0; 4]);
                wr_rect(vm, arg(0), r)?;
                b(true)
            }
            "rc_move" => {
                let r = rd_rect(vm, arg(0))?.unwrap_or([0; 4]);
                wr_rect(
                    vm,
                    arg(0),
                    [r[0] + i(1), r[1] + i(2), r[2] + i(1), r[3] + i(2)],
                )?;
                b(true)
            }
            "rc_inflate" => {
                let r = rd_rect(vm, arg(0))?.unwrap_or([0; 4]);
                wr_rect(
                    vm,
                    arg(0),
                    [r[0] - i(1), r[1] - i(2), r[2] + i(1), r[3] + i(2)],
                )?;
                b(true)
            }
            "rc_intersect" => {
                let x = rd_rect(vm, arg(1))?.unwrap_or([0; 4]);
                let y = rd_rect(vm, arg(2))?.unwrap_or([0; 4]);
                let r = intersect(x, y);
                let ok = !empty(r);
                wr_rect(vm, arg(0), if ok { r } else { [0; 4] })?;
                b(ok)
            }
            "rc_union" => {
                let x = rd_rect(vm, arg(1))?.unwrap_or([0; 4]);
                let y = rd_rect(vm, arg(2))?.unwrap_or([0; 4]);
                let r = if empty(x) {
                    y
                } else if empty(y) {
                    x
                } else {
                    [
                        x[0].min(y[0]),
                        x[1].min(y[1]),
                        x[2].max(y[2]),
                        x[3].max(y[3]),
                    ]
                };
                wr_rect(vm, arg(0), r)?;
                b(!empty(r))
            }
            "rc_in" => {
                let r = rd_rect(vm, arg(0))?.unwrap_or([0; 4]);
                b(i(1) >= r[0] && i(1) < r[2] && i(2) >= r[1] && i(2) < r[3])
            }
            // ---- windows
            "getsystem" => Native::Value(0),
            "setsystem" | "caption" | "setwallpaper" | "shell" | "exec" | "setrgn" | "setdrag" => {
                b(true)
            }
            "mbox" => {
                let msg = self.path(vm, arg(2))?;
                self.log.push(format!("mbox: {msg}"));
                Native::Value(match i(0) {
                    1 => 1,
                    3 | 4 => 6,
                    _ => 1,
                })
            }
            n if n.starts_with("menu_") => b(true),
            "log_clear" => b(true),
            "pal_get" => Native::Value(0),
            n if n.starts_with("pal_") || n.starts_with("drt_") => b(true),
            n if n.starts_with("hit_")
                || n.starts_with("p3d_")
                || n.starts_with("net_")
                || n.starts_with("dll_") =>
            {
                self.warn(format!("{n}: not supported"));
                Native::Value(0)
            }
            "thd_begin" | "thd_end" | "thd_suspend" | "thd_resume" => {
                self.warn(format!("{name}: not supported"));
                Native::Value(0)
            }
            _ => {
                self.warn(format!("{name}: not supported"));
                Native::Value(0)
            }
        };
        Ok(v)
    }
}

impl Host for State {
    fn native(&mut self, vm: &mut Vm, id: u16, a: &[u32]) -> Result<Native, String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(prof) = &mut self.profile {
            let t = std::time::Instant::now();
            let steps = vm.steps;
            let _ = prof;
            let r = self.call_native(vm, id, a);
            let e = t.elapsed().as_secs_f64();
            let prof = self.profile.as_mut().unwrap();
            let k = prof
                .entry(vm.natives[id as usize].clone())
                .or_insert((0, 0.0));
            k.0 += 1;
            k.1 += e;
            let _ = steps;
            return r;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.trace {
            let name = vm.natives[id as usize].clone();
            if !matches!(
                name.as_str(),
                "getcount" | "rc_in" | "rc_copy" | "rc_union" | "rc_box" | "rc_empty"
            ) {
                self.log.push(format!("{name}{a:x?} in {}", vm.backtrace()));
            }
        }
        self.call_native(vm, id, a)
    }
}

impl State {
    /// A sprite's children (sprites with a child array).
    fn children(&self, vm: &Vm, sp: u32) -> Result<Vec<u32>, String> {
        let l = &self.spr;
        let n = vm.ri32(sp + l.at("child_count"))?.max(0) as u32;
        let arr = vm.r32(sp + l.at("child"))?;
        let mut out = Vec::new();
        if arr != 0 {
            for k in 0..n {
                out.push(vm.r32(arr + 4 * k)?);
            }
        }
        Ok(out)
    }

    fn set_children(&self, vm: &mut Vm, sp: u32, kids: &[u32]) -> Result<(), String> {
        let l = &self.spr;
        let old = vm.r32(sp + l.at("child"))?;
        if old != 0 {
            vm.free(old);
        }
        let arr = if kids.is_empty() {
            0
        } else {
            vm.malloc(4 * kids.len() as u32)
        };
        for (k, c) in kids.iter().enumerate() {
            vm.w32(arr + 4 * k as u32, *c)?;
        }
        vm.w32(sp + l.at("child"), arr)?;
        vm.w32(sp + l.at("child_count"), kids.len() as u32)?;
        Ok(())
    }

    /// Takes a sprite from its parent.
    fn unlink(&self, vm: &mut Vm, sp: u32) -> Result<(), String> {
        let l = self.spr.clone();
        let parent = vm.r32(sp + l.at("parent"))?;
        if parent == 0 {
            return Ok(());
        }
        vm.w32(sp + l.at("parent"), 0)?;
        if !self.sprites.contains(&parent) {
            return Ok(());
        }
        if l.0.contains_key("child_count") {
            let mut kids = self.children(vm, parent)?;
            kids.retain(|&c| c != sp);
            self.set_children(vm, parent, &kids)?;
        } else if vm.r32(parent + l.at("child"))? == sp {
            vm.w32(parent + l.at("child"), 0)?;
        }
        Ok(())
    }

    fn stop_sound(&mut self, n: i32) {
        if let Some(s) = self.sounds.get_mut(&n)
            && s.started.take().is_some()
        {
            self.events.push(SoundEvent {
                channel: n,
                op: "stop",
                data: None,
                volume: 0.0,
                repeat: false,
            });
        }
    }
}
