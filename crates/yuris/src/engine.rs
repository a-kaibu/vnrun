//! The engine around the VM: CG layers, text, sound, save files and the other commands the
//! scripts give. A frontend (the browser, or `yuris-run`) calls [`Engine::frame`] once a frame,
//! draws [`Engine::draws`] and plays the [`Event`]s.

use std::collections::HashMap;

use crate::expr::{self, Kind, Place, Value, Var, VarId};
use crate::files::{Archives, Files};
use crate::image::{Image, Rect, adjust};
use crate::layers::{self, Draw, Layers};
use crate::product;
use crate::sjis;
use crate::sysvar::{self, Input};
use crate::text::{Font, Text};
use crate::vm::{Call, Flow, Host, Program, State, Vm};

/// What the frontend has to do besides drawing.
#[derive(Debug, Clone)]
pub enum Event {
    /// play a sound (Ogg or WAV bytes) in a sound layer, replacing what it played
    Play { id: String, data: Vec<u8>, looped: bool, volume: f32, fade_ms: f64, start_ms: f64 },
    Stop { id: String, fade_ms: f64 },
    Volume { id: String, volume: f32, fade_ms: f64 },
    Pause { id: String, paused: bool },
    /// the window's title
    Title(String),
}

#[derive(Debug, Clone, Default)]
pub struct Sound {
    pub file: String,
    pub playing: bool,
    pub paused: bool,
    pub looped: i64,
    pub volume: i64,
    pub e: bool,
}

pub struct World {
    pub layers: Layers,
    pub files: Box<dyn Files>,
    archives: Archives,
    text: Text,
    font: Font,
    pub sounds: HashMap<String, Sound>,
    pub events: Vec<Event>,
    /// SAVE data given without a FILE, written by the next SAVE with one
    reserved: Vec<(u32, Vec<u8>)>,
    pub title: String,
    start: [i64; 8],
    /// commands to print with their arguments, for debugging
    pub log: Vec<String>,
    /// 222 and 263: `ID=a NO=n` names the layer or sound `a.n` (263 scripts build such names themselves)
    numbered: bool,
    /// 222
    old: bool,
}

pub struct Engine {
    pub vm: Vm,
    pub world: World,
    pub input: Input,
}

impl Engine {
    /// Loads ysbin from the game's archives. `font` draws every text.
    pub fn new(mut files: Box<dyn Files>, font: Option<&[u8]>) -> Result<Engine, String> {
        let archives = Archives::open(files.as_mut());
        let mut read = |name: &str| archives.read(files.as_mut(), &format!("ysbin/{name}"));
        let prog = Program::load(&mut read)?;
        let (w, h) = (prog.config.width.max(1), prog.config.height.max(1));
        let title = prog.config.title.clone();
        let (numbered, old) = (prog.numbered, prog.old);
        let vm = Vm::new(prog)?;
        let world = World {
            layers: Layers::new(w, h),
            files,
            archives,
            text: Text::new(font),
            font: Font::default(),
            sounds: HashMap::new(),
            events: vec![Event::Title(title.clone())],
            reserved: Vec::new(),
            title,
            start: [0; 8],
            log: Vec::new(),
            numbered,
            old,
        };
        Ok(Engine { vm, world, input: Input::new() })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.world.layers.w, self.world.layers.h)
    }

    /// Runs one frame. `now` is milliseconds since the start, `date` the local time (year,
    /// month, day, weekday, h, m, s, ms).
    pub fn frame(&mut self, now: f64, date: [i64; 8]) -> Result<(), String> {
        if now == 0.0 || self.world.start == [0; 8] {
            self.world.start = date;
        }
        sysvar::clock(&mut self.vm.state, now, date, self.world.start);
        sysvar::update(&mut self.vm.state, &self.input);
        self.input.wheel = 0;
        self.vm.frame(now, &mut self.world)
    }

    pub fn draws(&self) -> Vec<Draw> {
        self.world.layers.screen()
    }

    /// The pixels of a layer by its texture number (what [`Draw::tex`] names).
    pub fn texture(&self, tex: u32) -> Option<&Image> {
        self.world.layers.map.values().find(|l| l.tex == tex).map(|l| &l.img)
    }

    /// The screen as one picture.
    pub fn screenshot(&self) -> Image {
        self.world.layers.render(layers::SCREEN)
    }

    /// A sound the frontend played has ended.
    pub fn sound_ended(&mut self, id: &str) {
        if let Some(s) = self.world.sounds.get_mut(id) {
            s.playing = false;
        }
    }
}

/// The keyword's value, if it is given.
fn int(st: &State, c: &Call, k: &str) -> Result<Option<i64>, String> {
    c.arg(k).map(|e| st.int(e)).transpose()
}

fn text(st: &State, c: &Call, k: &str) -> Result<Option<String>, String> {
    c.arg(k).map(|e| st.str(e)).transpose()
}

fn on(st: &State, c: &Call, k: &str) -> Result<bool, String> {
    Ok(int(st, c, k)?.unwrap_or(0) != 0)
}

/// Applies `X=`, `X+=`, ... to a number.
fn set(st: &State, c: &Call, k: &str, v: &mut i64) -> Result<(), String> {
    if let Some((op, e)) = c.arg_op(k) {
        *v = op.apply(Value::Int(*v), Value::Int(st.int(e)?)).int();
    }
    Ok(())
}

fn store(st: &mut State, c: &Call, v: Value) -> Result<(), String> {
    match c.arg("LET") {
        Some(e) => st.assign(e, v),
        None => Ok(()),
    }
}

const IMAGE_EXTS: [&str; 4] = ["png", "bmp", "jpg", "gif"];
const SOUND_EXTS: [&str; 2] = ["ogg", "wav"];

impl World {
    fn load_image(&mut self, name: &str) -> Result<Image, String> {
        if name.is_empty() {
            return Ok(Image::black());
        }
        let found = self.archives.find(self.files.as_mut(), name, &IMAGE_EXTS).ok_or_else(|| format!("no image {name}"))?;
        let bytes = self.archives.read(self.files.as_mut(), &found).ok_or_else(|| format!("cannot read {found}"))?;
        Image::decode(&bytes).map_err(|e| format!("{found}: {e}"))
    }

    /// A layer's top left on the screen.
    fn screen_pos(&self, name: &str) -> (i64, i64) {
        let mut path = layers::resolve(name);
        let (mut x, mut y) = (0, 0);
        while let Some(l) = self.layers.map.get(&path) {
            x += l.x;
            y += l.y;
            let up = layers::parent(&path);
            if up == ":/" || up == path {
                break;
            }
            path = up.to_string();
        }
        (x, y)
    }

    fn cg(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let id = text(st, c, "ID")?.ok_or("CG without ID")?;
        let file = text(st, c, "FILE")?;
        let img = match &file {
            // FILE="" makes a black picture: 1x1, or SX x SY when given (ERIS sizes its
            // buffers so)
            Some(f) if f.is_empty() => {
                let (w, h) = (int(st, c, "SX")?.unwrap_or(1).max(1), int(st, c, "SY")?.unwrap_or(1).max(1));
                let mut img = Image::new(w as u32, h as u32);
                img.px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
                Some(img)
            }
            Some(f) => Some(self.load_image(f).unwrap_or_else(|e| {
                eprintln!("CG: {e}");
                Image::black()
            })),
            None => None,
        };
        let l = self.layers.ensure(&id);
        if let Some(img) = img {
            l.img = img;
            l.touch();
        }
        let mut e = l.e as i64;
        set(st, c, "E", &mut e)?;
        l.e = e != 0;
        set(st, c, "A", &mut l.a)?;
        set(st, c, "X", &mut l.x)?;
        set(st, c, "Y", &mut l.y)?;
        set(st, c, "Z", &mut l.z)?;
        set(st, c, "MODE", &mut l.mode)?;
        set(st, c, "T", &mut l.t)?;
        set(st, c, "TA", &mut l.ta)?;
        set(st, c, "LINT", &mut l.lint[0])?;
        set(st, c, "LINT2", &mut l.lint[1])?;
        if let Some(m) = text(st, c, "MID")? {
            l.mid = m;
        }
        if let Some(t) = text(st, c, "TID")? {
            l.tid = t;
        }
        if l.set {
            let (mut w, mut h) = (l.img.w as i64, l.img.h as i64);
            set(st, c, "SX", &mut w)?;
            set(st, c, "SY", &mut h)?;
            l.img.w = w.max(0) as u32;
            l.img.h = h.max(0) as u32;
        }
        Ok(Flow::Next)
    }

    fn cgact(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let id = text(st, c, "ID")?.ok_or("CGACT without ID")?;
        let id2 = text(st, c, "ID2")?;
        if on(st, c, "SWAP")? || on(st, c, "SWAP2")? {
            if let Some(id2) = id2 {
                self.layers.swap(&id, &id2, on(st, c, "SWAP")?);
            }
            return Ok(Flow::Next);
        }
        let Some(src) = self.layers.get(&id) else {
            eprintln!("CGACT: no layer {id}");
            return Ok(Flow::Next);
        };
        // the layer without its pixels; those are copied only where needed
        let attrs = src.clone_attrs();
        let (set, w0, h0) = (src.set, src.w() as i64, src.h() as i64);
        let size = on(st, c, "SIZE")? || on(st, c, "SIZE2")?;
        let mut img = if size && !set {
            Image::default()
        } else if set {
            self.layers.render(&id)
        } else {
            src.img.clone()
        };
        // the part of the layer to work on: X..=X2, Y..=Y2
        // (SIZE2 may give FX, FY, ... in hundredths of a pixel instead)
        let get = |st: &mut State, k: &str| -> Result<Option<i64>, String> {
            Ok(match int(st, c, k)? {
                Some(v) => Some(v),
                None => int(st, c, &format!("F{k}"))?.map(|v| v.div_euclid(100)),
            })
        };
        let x0 = get(st, "X")?.unwrap_or(0);
        let y0 = get(st, "Y")?.unwrap_or(0);
        let x1 = get(st, "X2")?.map_or(w0, |v| v + 1);
        let y1 = get(st, "Y2")?.map_or(h0, |v| v + 1);
        let area = Rect { x0, y0, x1, y1 };
        let value = int(st, c, "SET")?.unwrap_or(0);
        let dest = id2.clone().unwrap_or_else(|| id.clone());
        let mut moved = (0, 0);

        if on(st, c, "COPY")? || on(st, c, "COPY2")? {
            img = img.crop(area);
            if id2.is_none() {
                // a part of the layer onto itself: keep the rest of it
                let mut full = self.layers.get(&id).map(|l| l.img.clone()).unwrap_or_default();
                full.put(&img, x0, y0);
                img = full;
            }
        } else if size {
            // the part X..=X2, Y..=Y2 (all of it by default) scaled to SX x SY
            let w = get(st, "SX")?.unwrap_or(area.w()).max(1) as u32;
            let h = get(st, "SY")?.unwrap_or(area.h()).max(1) as u32;
            // the destination's old pixels, to write into
            let reuse = match self.layers.get_mut(&dest) {
                Some(l) if dest != id => std::mem::take(&mut l.img),
                _ => Image::default(),
            };
            let from = if set { &img } else { &self.layers.get(&id).ok_or("CGACT: no layer")?.img };
            img = from.scale_area(area, w, h, on(st, c, "SIZE2")?, reuse);
        } else if on(st, c, "RESIZE")? {
            img = img.resize_edges(x0, y0, int(st, c, "X2")?.unwrap_or(0), int(st, c, "Y2")?.unwrap_or(0));
            moved = (-x0, -y0);
        } else if on(st, c, "TRIM")? {
            let (t, dx, dy) = img.trim(if c.arg("SET").is_some() { value } else { 15 });
            img = t;
            moved = (dx, dy);
        } else if on(st, c, "TRANSITION")? {
            // ID fading into ID2, SET of 256 of the way, stored in ID3 (ERIS changing a
            // standing picture)
            let Some(b) = id2.as_deref().and_then(|d| self.layers.get(d)) else { return Ok(Flow::Next) };
            let mixed = img.mix(&b.img, value);
            let Some(id3) = text(st, c, "ID3")? else { return Ok(Flow::Next) };
            let to = self.layers.ensure(&id3);
            to.img = mixed;
            to.touch();
            return Ok(Flow::Next);
        } else if on(st, c, "MERGE")? || on(st, c, "MERGE2")? {
            let Some(to) = id2.as_deref().and_then(|d| self.layers.get_mut(d)) else { return Ok(Flow::Next) };
            if on(st, c, "MERGE2")? {
                let a = to.a.clamp(0, 256) as u32;
                to.img.px.chunks_exact_mut(4).for_each(|p| p[3] = (p[3] as u32 * a / 256) as u8);
                to.a = 256;
            }
            // X, Y place it in the layer it goes into (ERIS drawing text a glyph at a time);
            // otherwise both stay where they are on the screen
            let (dx, dy) = if c.arg("X").is_some() || c.arg("Y").is_some() {
                (x0, y0)
            } else {
                (attrs.x - to.x, attrs.y - to.y)
            };
            let clip = to.img.rect();
            to.img.draw(&img, dx, dy, attrs.a, attrs.mode, clip);
            to.touch();
            return Ok(Flow::Next);
        } else if on(st, c, "VERTICAL")? || on(st, c, "HORIZONTAL")? {
            img.flip(on(st, c, "VERTICAL")?);
        } else if on(st, c, "TEXT")? {
            let s = text(st, c, "SETSTR")?.unwrap_or_default();
            let mut to = match id2.as_deref().and_then(|d| self.layers.get(d)) {
                Some(l) => l.img.clone(),
                None => img,
            };
            // SET=1 draws the edge under the text
            let edge = if int(st, c, "SET")?.unwrap_or(0) == 1 { 2 } else { 0 };
            self.text.draw(&mut to, &s, x0, y0, &self.font, edge);
            img = to;
        } else {
            let f: Box<dyn Fn(&mut [u8])> = if on(st, c, "SAIDO")? {
                Box::new(move |p| adjust::saturation(p, value))
            } else if on(st, c, "MEIDO")? {
                Box::new(move |p| adjust::channels(p, value))
            } else if on(st, c, "AKARUSA")? {
                Box::new(move |p| adjust::brightness(p, value))
            } else if on(st, c, "MONO")? {
                Box::new(move |p| adjust::mono(p, value))
            } else if on(st, c, "REVERSE")? {
                Box::new(adjust::invert)
            } else if on(st, c, "RECTPAINT")? && self.old {
                // 222: transparency in the top byte (its text layers are cleared with 0xff000000)
                let a = 255 - ((value >> 24) & 0xff) as u8;
                Box::new(move |p| p.copy_from_slice(&[(value >> 16) as u8, (value >> 8) as u8, value as u8, a]))
            } else if on(st, c, "RECTPAINT")? {
                Box::new(move |p| p[..3].copy_from_slice(&[(value >> 16) as u8, (value >> 8) as u8, value as u8]))
            } else if on(st, c, "RECTPAINT2")? {
                Box::new(move |p| p[3] = value.clamp(0, 255) as u8)
            } else if on(st, c, "RECTPAINT3")? {
                Box::new(move |p| p.copy_from_slice(&[(value >> 16) as u8, (value >> 8) as u8, value as u8, (value >> 24) as u8]))
            } else {
                let keys: Vec<&str> = c.args.iter().map(|a| a.0).collect();
                eprintln!("CGACT: not done: {keys:?}");
                return Ok(Flow::Next);
            };
            if id2.is_some() {
                img = img.crop(area);
                img.each(img.rect(), f);
            } else {
                img.each(area, f);
            }
        }
        let copy_attrs = on(st, c, "COPY")?;
        let to = self.layers.ensure(&dest);
        if copy_attrs && id2.is_some() {
            let (tex, seq) = (to.tex, to.seq);
            *to = attrs;
            to.tex = tex;
            to.seq = seq;
        }
        to.img = img;
        to.x += moved.0;
        to.y += moved.1;
        to.touch();
        Ok(Flow::Next)
    }

    fn cginfo(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let id = text(st, c, "ID")?.unwrap_or_default();
        let l = self.layers.get(&id);
        let mut v = Value::Int(0);
        if on(st, c, "EXIST")? {
            v = Value::Int(l.is_some() as i64);
        } else if let Some(l) = l {
            // the point to look at; ONMOUSE without SET/SET2 looks under the mouse
            let mx = st.get_sys(sysvar::MOUSE_X, false, &[]).int();
            let my = st.get_sys(sysvar::MOUSE_Y, false, &[]).int();
            let (sx, sy) = (int(st, c, "SET")?.unwrap_or(mx), int(st, c, "SET2")?.unwrap_or(my));
            for k in ["E", "A", "T", "TA", "X", "Y", "Z", "SX", "SY", "MODE", "COLOR", "ONMOUSE", "ONMOUSE2", "ONMOUSE3", "TID", "LINT", "LINT2"] {
                if !on(st, c, k)? {
                    continue;
                }
                v = match k {
                    "E" => Value::Int(l.e as i64),
                    "A" => Value::Int(l.a),
                    "T" => Value::Int(l.t),
                    "TA" => Value::Int(l.ta),
                    "X" => Value::Int(l.x),
                    "Y" => Value::Int(l.y),
                    "Z" => Value::Int(l.z),
                    "SX" => Value::Int(l.w() as i64),
                    "SY" => Value::Int(l.h() as i64),
                    "MODE" => Value::Int(l.mode),
                    "TID" => Value::text(&l.tid),
                    "LINT" => Value::Int(l.lint[0]),
                    "LINT2" => Value::Int(l.lint[1]),
                    "COLOR" => Value::Int(l.img.at(sx, sy).map_or(-1, |p| argb(p) & 0xffffff)),
                    _ => {
                        let (lx, ly) = self.screen_pos(&id);
                        let p = if l.set {
                            let inside = sx >= lx && sy >= ly && sx < lx + l.w() as i64 && sy < ly + l.h() as i64;
                            inside.then_some([0, 0, 0, 255])
                        } else {
                            l.img.at(sx - lx, sy - ly)
                        };
                        Value::Int(match (k, p) {
                            (_, None) => -1,
                            ("ONMOUSE", Some(p)) => argb(p) & 0xffffff,
                            ("ONMOUSE2", Some(p)) => p[3] as i64,
                            (_, Some(p)) => argb(p),
                        })
                    }
                };
                break;
            }
        } else if on(st, c, "ONMOUSE")? || on(st, c, "ONMOUSE2")? || on(st, c, "ONMOUSE3")? {
            v = Value::Int(-1);
        }
        store(st, c, v)?;
        Ok(Flow::Next)
    }

    fn font_cmd(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let f = &mut self.font;
        if let Some(n) = text(st, c, "NAME")? {
            f.name = n;
        }
        set(st, c, "SX", &mut f.sx)?;
        set(st, c, "SY", &mut f.sy)?;
        if let Some(v) = int(st, c, "COLOR")? {
            f.color = v as u32 & 0xffffff;
            f.color2 = None;
        }
        if let Some(v) = int(st, c, "COLOR2")? {
            // -1: no gradient
            f.color2 = (v >= 0 && v as u32 & 0xffffff != f.color).then_some(v as u32 & 0xffffff);
        }
        if let Some(v) = int(st, c, "BOLD")? {
            f.bold = v != 0;
        }
        Ok(Flow::Next)
    }

    fn fontinfo(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let f = &self.font;
        let v = if on(st, c, "NUM")? {
            Value::Int(1)
        } else if on(st, c, "NAME")? {
            Value::text(if int(st, c, "NO")?.is_some() { "ＭＳ ゴシック" } else { &f.name })
        } else if on(st, c, "SX")? {
            Value::Int(f.sx)
        } else if on(st, c, "SY")? {
            Value::Int(f.sy)
        } else if on(st, c, "COLOR")? {
            Value::Int(f.color as i64)
        } else if on(st, c, "COLOR2")? {
            Value::Int(f.color2.unwrap_or(f.color) as i64)
        } else if on(st, c, "BOLD")? {
            Value::Int(f.bold as i64)
        } else {
            Value::Int(0)
        };
        store(st, c, v)?;
        Ok(Flow::Next)
    }

    fn sound(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let id = text(st, c, "ID")?.unwrap_or_default();
        let fade = int(st, c, "FADE")?.unwrap_or(0) as f64;
        let mut s = self.sounds.remove(&id).unwrap_or(Sound { volume: 256, e: true, ..Default::default() });
        if let Some(f) = text(st, c, "FILE")? {
            s.file = f;
        }
        set(st, c, "LOOP", &mut s.looped)?;
        let old_volume = s.volume;
        set(st, c, "VOLUME", &mut s.volume)?;
        if let Some(e) = int(st, c, "E")? {
            s.e = e != 0;
        }
        let volume = |s: &Sound| if s.e { s.volume.clamp(0, 256) as f32 / 256.0 } else { 0.0 };
        match int(st, c, "PLAY")? {
            Some(0) => {
                if s.playing {
                    self.events.push(Event::Stop { id: id.clone(), fade_ms: fade });
                }
                s.playing = false;
            }
            Some(_) => {
                let found = self.archives.find(self.files.as_mut(), &s.file, &SOUND_EXTS);
                match found.and_then(|f| self.archives.read(self.files.as_mut(), &f)) {
                    Some(data) => {
                        self.events.push(Event::Play {
                            id: id.clone(),
                            data,
                            looped: s.looped == 0,
                            volume: volume(&s),
                            fade_ms: fade,
                            start_ms: int(st, c, "START")?.unwrap_or(0) as f64,
                        });
                        s.playing = true;
                        s.paused = false;
                    }
                    None => eprintln!("SOUND: no file {}", s.file),
                }
            }
            None => {
                if s.playing && (s.volume != old_volume || c.arg("E").is_some()) {
                    self.events.push(Event::Volume { id: id.clone(), volume: volume(&s), fade_ms: fade });
                }
            }
        }
        if let Some(p) = int(st, c, "PAUSE")? {
            s.paused = p != 0;
            self.events.push(Event::Pause { id: id.clone(), paused: s.paused });
        }
        self.sounds.insert(id, s);
        Ok(Flow::Next)
    }

    fn soundinfo(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let id = text(st, c, "ID")?.unwrap_or_default();
        let s = self.sounds.get(&id);
        let v = if on(st, c, "EXIST")? {
            Value::Int(s.is_some() as i64)
        } else if on(st, c, "FILE")? {
            Value::text(s.map_or("", |s| &s.file))
        } else if on(st, c, "PLAY")? {
            Value::Int(s.is_some_and(|s| s.playing) as i64)
        } else if on(st, c, "VOLUME")? {
            Value::Int(s.map_or(0, |s| s.volume))
        } else if on(st, c, "PAUSE")? {
            Value::Int(s.is_some_and(|s| s.paused) as i64)
        } else if on(st, c, "E")? {
            Value::Int(s.is_some_and(|s| s.e) as i64)
        } else if on(st, c, "LOOP")? {
            Value::Int(s.map_or(0, |s| s.looped))
        } else {
            Value::Int(0)
        };
        store(st, c, v)?;
        Ok(Flow::Next)
    }

    fn save_name(name: &str) -> String {
        let name = name.replace('\\', "/");
        let name = name.trim_start_matches("save/").to_string();
        if name.contains('.') { name } else { format!("{name}.sd") }
    }

    fn save(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let dno = int(st, c, "DNO")?;
        if let Some(dno) = dno {
            let data = if let Some(e) = c.arg("SET") {
                let p = st.place(e)?;
                let var = st.vars.get(p.var.id as usize).and_then(Option::as_ref);
                match var {
                    Some(var) if p.whole => save_var(var),
                    _ => {
                        let v = crate::expr::Vars::get(st, &p);
                        save_var(&Var::new(kind(&v), Vec::new(), v))
                    }
                }
            } else if let Some(id) = text(st, c, "ID")? {
                let img = self.layers.get(&id).map(|l| l.img.clone()).unwrap_or_default();
                let mut b = vec![4u8];
                b.extend(img.w.to_le_bytes());
                b.extend(img.h.to_le_bytes());
                b.extend(&img.px);
                b
            } else {
                return Ok(Flow::Next);
            };
            self.reserved.retain(|(d, _)| *d != dno as u32);
            self.reserved.push((dno as u32, data));
        }
        if let Some(file) = text(st, c, "FILE")? {
            let name = Self::save_name(&file);
            let mut entries = read_save(&self.files.read_save(&name).unwrap_or_default());
            for (d, data) in self.reserved.drain(..) {
                entries.insert(d, data);
            }
            let mut out = Vec::new();
            let mut keys: Vec<&u32> = entries.keys().collect();
            keys.sort();
            for k in keys {
                out.extend(k.to_le_bytes());
                out.extend((entries[k].len() as u32).to_le_bytes());
                out.extend(&entries[k]);
            }
            self.files.write_save(&name, &out);
        }
        Ok(Flow::Next)
    }

    fn load(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let file = text(st, c, "FILE")?.unwrap_or_default();
        let dno = int(st, c, "DNO")?.unwrap_or(0) as u32;
        let Some(bytes) = self.files.read_save(&Self::save_name(&file)) else { return Ok(Flow::Next) };
        let entries = read_save(&bytes);
        let Some(data) = entries.get(&dno) else { return Ok(Flow::Next) };
        if data.first() == Some(&4) {
            if let Some(id) = text(st, c, "ID")? {
                let w = u32::from_le_bytes(data[1..5].try_into().unwrap());
                let h = u32::from_le_bytes(data[5..9].try_into().unwrap());
                let l = self.layers.ensure(&id);
                l.img = Image { w, h, px: data[9..].to_vec() };
                l.touch();
            }
            return Ok(Flow::Next);
        }
        let Some(var) = load_var(data) else { return Ok(Flow::Next) };
        if let Some(e) = c.arg("LET") {
            let p = st.place(e)?;
            if p.whole {
                let v = st.vars.get(p.var.id as usize).and_then(Option::as_ref);
                // keep the variable's type
                let kind = v.map_or(var.kind, |v| v.kind);
                let vals = var.vals.into_iter().map(|x| kind.coerce(x)).collect();
                if st.vars.len() <= p.var.id as usize {
                    st.vars.resize(p.var.id as usize + 1, None);
                }
                st.vars[p.var.id as usize] = Some(Var { kind, dims: var.dims, vals });
            } else if let Some(v) = var.vals.into_iter().next() {
                st.set(&p, v);
            }
        }
        Ok(Flow::Next)
    }

    fn text_out(&mut self, st: &mut State, t: &[u8]) {
        let mut text = st.get_sys(sysvar::TEXT, true, &[]).bytes();
        let mut flag = st.get_sys(sysvar::TEXT_FLAG, true, &[]).bytes();
        for ch in sjis::chars(t) {
            text.extend_from_slice(ch);
            flag.push(if ch.len() == 2 { b'2' } else { b'1' });
        }
        st.set_sys(sysvar::TEXT, true, &[], Value::Str(text));
        st.set_sys(sysvar::TEXT_FLAG, true, &[], Value::Str(flag));
    }
}

fn argb(p: [u8; 4]) -> i64 {
    ((p[3] as i64) << 24) | ((p[0] as i64) << 16) | ((p[1] as i64) << 8) | p[2] as i64
}

fn kind(v: &Value) -> Kind {
    match v {
        Value::Int(_) => Kind::Int,
        Value::Flt(_) => Kind::Flt,
        Value::Str(_) => Kind::Str,
    }
}

/// A variable in a save file: type, dimensions and values.
fn save_var(v: &Var) -> Vec<u8> {
    let mut b = vec![match v.kind {
        Kind::Int => 1u8,
        Kind::Flt => 2,
        Kind::Str => 3,
    }];
    b.push(v.dims.len() as u8);
    for d in &v.dims {
        b.extend((*d as u32).to_le_bytes());
    }
    b.extend((v.vals.len() as u32).to_le_bytes());
    for x in &v.vals {
        match (v.kind, x) {
            (Kind::Int, x) => b.extend(x.int().to_le_bytes()),
            (Kind::Flt, x) => b.extend(x.flt().to_le_bytes()),
            (Kind::Str, x) => {
                let s = x.bytes();
                b.extend((s.len() as u32).to_le_bytes());
                b.extend(s);
            }
        }
    }
    b
}

fn load_var(b: &[u8]) -> Option<Var> {
    let mut at = 0;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let kind = match take(1)?[0] {
        1 => Kind::Int,
        2 => Kind::Flt,
        3 => Kind::Str,
        _ => return None,
    };
    let nd = take(1)?[0] as usize;
    let mut dims = Vec::new();
    for _ in 0..nd {
        dims.push(u32::from_le_bytes(take(4)?.try_into().ok()?) as usize);
    }
    let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
    let mut vals = Vec::with_capacity(n);
    for _ in 0..n {
        vals.push(match kind {
            Kind::Int => Value::Int(i64::from_le_bytes(take(8)?.try_into().ok()?)),
            Kind::Flt => Value::Flt(f64::from_le_bytes(take(8)?.try_into().ok()?)),
            Kind::Str => {
                let l = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
                Value::Str(take(l)?.to_vec())
            }
        });
    }
    Some(Var { kind, dims, vals })
}

/// A save file: data numbers and their bytes.
fn read_save(b: &[u8]) -> HashMap<u32, Vec<u8>> {
    let mut out = HashMap::new();
    let mut at = 0;
    while at + 8 <= b.len() {
        let dno = u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let n = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let Some(d) = b.get(at + 8..at + 8 + n) else { break };
        out.insert(dno, d.to_vec());
        at += 8 + n;
    }
    out
}

/// 222 names a layer or sound by two keywords, ID and NO (ID2 and NO2, MID and MNO, TID and
/// TNO): the name with `.n` after it when n is not 0, as a string expression.
fn numbered_names(st: &State, c: &Call) -> Result<Vec<(usize, Vec<u8>)>, String> {
    let mut out = Vec::new();
    for (i, (k, _, e)) in c.args.iter().enumerate() {
        let no = match k.to_ascii_uppercase().as_str() {
            "ID" => "NO",
            "ID2" => "NO2",
            "MID" => "MNO",
            "TID" => "TNO",
            _ => continue,
        };
        let n = int(st, c, no)?.unwrap_or(0);
        if n == 0 {
            continue;
        }
        let mut name = st.eval(e)?.bytes();
        name.extend(format!(".{n}").bytes());
        let mut q = vec![b'"'];
        q.extend(name);
        q.push(b'"');
        let mut x = vec![b'M'];
        x.extend((q.len() as u16).to_le_bytes());
        x.extend(q);
        out.push((i, x));
    }
    Ok(out)
}

impl Host for World {
    fn command(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        if self.numbered {
            let names = numbered_names(st, c)?;
            if !names.is_empty() {
                let args = c
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, &(k, a, e))| (k, a, names.iter().find(|n| n.0 == i).map_or(e, |n| &n.1[..])))
                    .collect();
                return self.dispatch(st, &Call { name: c.name, args, pos: c.pos });
            }
        }
        self.dispatch(st, c)
    }

    fn text(&mut self, st: &mut State, text: &[u8]) -> Result<Flow, String> {
        self.text_out(st, text);
        Ok(Flow::Next)
    }
}

impl World {
    fn dispatch(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        if self.log.iter().any(|n| n == c.name || n == "*") {
            let args: Vec<String> = c.args.iter().map(|(k, _, e)| format!("{k}={}", st.eval(e).map_or("?".into(), |v| v.str()))).collect();
            eprintln!("[{}] {}[{}]", st.task().id, c.name, args.join(" "));
        }
        match c.name {
            "CG" => self.cg(st, c),
            "CGACT" => self.cgact(st, c),
            "CGINFO" => self.cginfo(st, c),
            "CGEND" => {
                if let Some(id) = text(st, c, "ID")? {
                    self.layers.remove(&id);
                }
                Ok(Flow::Next)
            }
            "FONT" => self.font_cmd(st, c),
            "FONTINFO" => self.fontinfo(st, c),
            "SOUND" => self.sound(st, c),
            "SOUNDINFO" => self.soundinfo(st, c),
            "SOUNDEND" => {
                let id = text(st, c, "ID")?.unwrap_or_default();
                if self.sounds.remove(&id).is_some_and(|s| s.playing) {
                    self.events.push(Event::Stop { id, fade_ms: 0.0 });
                }
                Ok(Flow::Next)
            }
            "SAVE" => self.save(st, c),
            "SAVEEND" => {
                self.reserved.clear();
                Ok(Flow::Next)
            }
            "LOAD" => self.load(st, c),
            "FILEINFO" => {
                // (555) CGFILE= / SOUNDFILE=: a picture / sound named without its extension
                for (k, exts) in [("CGFILE", &IMAGE_EXTS[..]), ("SOUNDFILE", &SOUND_EXTS[..])] {
                    if let Some(name) = text(st, c, k)? {
                        let found = self.archives.find(self.files.as_mut(), &name, exts).is_some();
                        store(st, c, Value::Int(found as i64))?;
                        return Ok(Flow::Next);
                    }
                }
                let file = text(st, c, "FILE")?.unwrap_or_default();
                let exists = if file.to_lowercase().ends_with(".sd") || !file.contains('.') {
                    self.files.read_save(&Self::save_name(&file)).is_some()
                        || self.archives.exists(self.files.as_mut(), &file)
                } else {
                    self.archives.exists(self.files.as_mut(), &file)
                };
                store(st, c, Value::Int(exists as i64))?;
                Ok(Flow::Next)
            }
            "FILEACT" => {
                let file = text(st, c, "FILE")?.unwrap_or_default();
                if on(st, c, "DELETE")? {
                    self.files.remove_save(&Self::save_name(&file));
                } else if on(st, c, "COPY")? {
                    let to = text(st, c, "FILE2")?.unwrap_or_default();
                    if let Some(b) = self.files.read_save(&Self::save_name(&file)) {
                        self.files.write_save(&Self::save_name(&to), &b);
                    }
                }
                Ok(Flow::Next)
            }
            "WINDOW" => {
                if let Some(t) = text(st, c, "CAPTION")? {
                    if !t.is_empty() {
                        self.title = t.clone();
                        self.events.push(Event::Title(t));
                    }
                }
                Ok(Flow::Next)
            }
            "WINDOWINFO" => {
                let v = if on(st, c, "SX")? {
                    Value::Int(self.layers.w as i64)
                } else if on(st, c, "SY")? {
                    Value::Int(self.layers.h as i64)
                } else if on(st, c, "CAPTION")? {
                    Value::text(&self.title)
                } else if on(st, c, "EXIST")? || on(st, c, "E")? || on(st, c, "ACTIVE")? {
                    Value::Int((int(st, c, "NO")?.unwrap_or(0) == 0) as i64)
                } else {
                    Value::Int(0)
                };
                store(st, c, v)?;
                Ok(Flow::Next)
            }
            "MATH3D" => {
                math3d(st, c)?;
                Ok(Flow::Next)
            }
            "MATH" => {
                let a = c.arg("SET").map(|e| st.eval(e)).transpose()?.unwrap_or_default();
                let b = c.arg("SET2").map(|e| st.eval(e)).transpose()?.unwrap_or_default();
                let (x, y) = (a.flt(), b.flt());
                let ops: [(&str, fn(f64, f64) -> f64); 17] = [
                    ("SIN", |x, _| x.sin()),
                    ("ASIN", |x, _| x.asin()),
                    ("SINH", |x, _| x.sinh()),
                    ("COS", |x, _| x.cos()),
                    ("ACOS", |x, _| x.acos()),
                    ("COSH", |x, _| x.cosh()),
                    ("TAN", |x, _| x.tan()),
                    ("ATAN", |x, _| x.atan()),
                    ("ATAN2", |x, y| x.atan2(y)),
                    ("TANH", |x, _| x.tanh()),
                    ("SQRT", |x, _| x.sqrt()),
                    ("ABS", |x, _| x.abs()),
                    ("EXP", |x, _| x.exp()),
                    ("LOG", |x, _| x.ln()),
                    ("LOG10", |x, _| x.log10()),
                    ("POW", |x, y| x.powf(y)),
                    ("RND", |x, _| (random() * x.max(0.0)).floor()),
                ];
                for (k, f) in ops {
                    if on(st, c, k)? {
                        let r = f(x, y);
                        let v = if k == "RND" || (k == "ABS" && matches!(a, Value::Int(_))) {
                            Value::Int(r as i64)
                        } else {
                            Value::Flt(r)
                        };
                        store(st, c, v)?;
                        break;
                    }
                }
                Ok(Flow::Next)
            }
            "DIALOG" => {
                // INPUT=1: a box to type a line into (the player's name; Lilith asks again while
                // it is empty). There is none here: the default text (DEFSTR) is taken.
                if on(st, c, "INPUT")? {
                    if let Some(e) = c.arg("LETSTR") {
                        let def = c.arg("DEFSTR").map(|d| st.eval(d)).transpose()?.map_or_else(Vec::new, |v| v.bytes());
                        let e = e.to_vec();
                        st.assign(&e, Value::Str(def))?;
                    }
                    store(st, c, Value::Int(1))?;
                    return Ok(Flow::Next);
                }
                let s = text(st, c, "STR")?.unwrap_or_default();
                let caption = text(st, c, "CAPTION")?.unwrap_or_default();
                let buttons = if on(st, c, "YESNOCANCEL")? {
                    3
                } else if on(st, c, "YESNO")? || on(st, c, "OKCANCEL")? {
                    2
                } else {
                    1
                };
                let r = self.files.dialog(&s, &caption, buttons);
                store(st, c, Value::Int(r))?;
                Ok(Flow::Next)
            }
            "TEXT" => {
                // FILE= LET=v[$S]: the file's lines, each with its line break, from $S(0) on
                if let (Some(file), Some(e)) = (text(st, c, "FILE")?, c.arg("LET")) {
                    let b = self.archives.read(self.files.as_mut(), &file).unwrap_or_default();
                    let lines = b.split_inclusive(|&x| x == b'\n').map(<[u8]>::to_vec).collect();
                    let var = st.place(e)?.var;
                    st.fill(var, lines);
                }
                if on(st, c, "CLEAR")? {
                    st.set_sys(sysvar::TEXT, true, &[], Value::Str(Vec::new()));
                    st.set_sys(sysvar::TEXT_FLAG, true, &[], Value::Str(Vec::new()));
                }
                Ok(Flow::Next)
            }
            "ERROR" | "FPS" | "INPUT" | "MOUSE" | "SYSTEM" | "TRAY" | "MENU" | "PREP" | "CLIPACT"
            | "MOVIE" | "MOVIEEND" | "YMV" | "YMVEND" | "FLASH" | "FLASHEND" => Ok(Flow::Next),
            "PRODUCT" => self.product(st, c),
            "MOVIEINFO" | "YMVINFO" | "FLASHINFO" | "MENUINFO" | "CLIPINFO" | "REGINFO" => {
                store(st, c, Value::Int(0))?;
                Ok(Flow::Next)
            }
            other => {
                eprintln!("not done: {other}");
                Ok(Flow::Next)
            }
        }
    }
}

impl World {
    /// PRODUCT: see [`product`].
    fn product(&mut self, st: &mut State, c: &Call) -> Result<Flow, String> {
        let at = |k: &str| int(st, c, k).map(|v| v.unwrap_or(0));
        match int(st, c, "COM")?.unwrap_or(0) {
            21 => {
                let dir = text(st, c, "PSTR")?.unwrap_or_default();
                let names = self.archives.list(self.files.as_mut(), &dir);
                let files = VarId { id: sysvar::FILES, str: true };
                st.declare(files, Kind::Str, vec![names.len().max(1)], Value::Str(Vec::new()));
                for (i, n) in names.iter().enumerate() {
                    st.set_sys(sysvar::FILES, true, &[i as i64], Value::text(n));
                }
                st.set_sys(sysvar::FILES_COUNT, false, &[], Value::Int(names.len() as i64));
            }
            22 => {
                let line = c.arg("PSTR").map(|e| st.eval(e)).transpose()?.map_or_else(Vec::new, |v| v.bytes());
                let (strs, kinds, flag) = (at("PINT")?, at("PINT2")?, at("PINT3")?);
                let (toks, open) = product::tokens(&line, st.get_at(flag).int() != 0);
                for (i, (t, k)) in toks.iter().enumerate() {
                    st.set_at(strs + i as i64, Value::Str(t.clone()));
                    st.set_at(kinds + i as i64, Value::Int(*k));
                }
                if let Some(open) = open {
                    st.set_at(flag, Value::Int(open as i64));
                }
                store(st, c, Value::Int(toks.len() as i64))?;
            }
            23 => {
                let (strs, lens) = (at("PINT")?, at("PINT2")?);
                let mut n = 0;
                loop {
                    let len = st.get_at(strs + n).bytes().len() as i64;
                    st.set_at(lens + n, Value::Int(len));
                    if len == 0 {
                        break;
                    }
                    n += 1;
                }
                store(st, c, Value::Int(n))?;
            }
            29 => store(st, c, Value::Int(0))?,
            _ => {}
        }
        Ok(Flow::Next)
    }
}

/// MATH3D[INT=@I() FLT=@F()]: where ERIS puts a sprite on the screen (its es.SP.XYZCALC), as
/// the engine computes it. Each 100 of depth between the camera and a layer halves the layer;
/// at DEFZ it is drawn at its own size. In I: 1 the sprite, 2 the background, 4 the parent
/// sprite (0 for none), 5 and 6 the screen size, 7 not moved by the camera. In F: 3-5 the
/// camera, 6-8 the background, 9-10 its centre, 11-12 its offset, 13-15 the sprite, 16-17 its
/// zoom (%), 18-19 its centre, 20-21 its scale (%), 22-24 the parent, 25-26 the parent's centre,
/// 27 DEFZ, 28-30 a camera shift. Out: F31-32 the top left, F33-34 the zoom (%).
fn math3d(st: &mut State, c: &Call) -> Result<(), String> {
    let (Some(ie), Some(fe)) = (c.arg("INT"), c.arg("FLT")) else { return Ok(()) };
    let (ip, fp) = (st.place(ie)?, st.place(fe)?);
    let i = |k: i64| expr::Vars::get(st, &Place { var: ip.var, index: vec![k], whole: false }).int();
    let f = |k: i64| {
        let v = expr::Vars::get(st, &Place { var: fp.var, index: vec![k], whole: false }).flt();
        // the scales come in percent
        if k == 20 || k == 21 { v / 100.0 } else { v }
    };
    let (sw, sh) = (i(5) as f64, i(6) as f64);
    let (cx, cy, cz) = (f(3) + f(28), f(4) + f(29), f(5) + f(30));
    let (zoom_x, zoom_y) = (f(16), f(17));
    let (x, y, zx, zy) = if i(7) != 0 {
        (f(13) - f(18) * zoom_x / 100.0, f(14) - f(19) * zoom_y / 100.0, zoom_x, zoom_y)
    } else {
        let depth = |z: f64| 2f64.powf((f(27) + cz - z) / 100.0);
        let bg = depth(f(8));
        let mut x = bg * (f(6) - f(9) - f(20) * cx) + f(11) * f(20) + sw / 2.0;
        let mut y = bg * (f(7) - f(10) - f(21) * cy) + f(12) * f(21) + sh / 2.0;
        let mut s = depth(f(15));
        if i(1) == i(2) {
            // the background keeps its centre where it is
            x += s * f(18);
            y += s * f(19);
        } else {
            x += bg * f(9);
            y += bg * f(10);
            if i(4) == 0 {
                x += s * f(13) * f(20);
                y += s * f(14) * f(21);
            } else {
                // a child is sized by its parent's depth
                s = depth(f(24));
                x += s * (f(20) * (f(13) + f(22)) - f(25));
                y += s * (f(21) * (f(14) + f(23)) - f(26));
            }
        }
        (x - s * f(18) * zoom_x / 100.0, y - s * f(19) * zoom_y / 100.0, s * zoom_x, s * zoom_y)
    };
    if std::env::var_os("YURIS_M3D").is_some() {
        eprintln!("M3D I={:?} F={:?} -> {x} {y} {zx} {zy}", (1..=7).map(i).collect::<Vec<_>>(), (3..=30).map(f).collect::<Vec<_>>());
    }
    for (k, v) in [(31, x), (32, y), (33, zx), (34, zy)] {
        st.set(&Place { var: fp.var, index: vec![k], whole: false }, Value::Flt(v));
    }
    Ok(())
}

/// A random number in 0..1 (xorshift; the scripts only want dice).
fn random() -> f64 {
    use std::cell::Cell;
    thread_local!(static S: Cell<u64> = const { Cell::new(0x9e37_79b9_7f4a_7c15) });
    S.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}
