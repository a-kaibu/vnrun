//! Runs the scripts a frame at a time and keeps what the screen shows: picture layers (a
//! background and standing pictures, by layer number), the message window with its face and
//! name plate, choices, and the built-in title screen (its picture, button parts and button
//! table come from system.arc and sys.arc).
//!
//! Pictures change on the stage first; `<UpDate kind,ms>` shows the stage, fading from what was
//! on screen before. Sound numbers: 0-1 music, 2-3 effects, 4.. voices (`<Se n>` is 2+n,
//! `<Voice n>` is 3+n).
//!
//! Saving is ours (crate::save): a right click on a page opens the save screen, and at the
//! title the load screen.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::arc::Index;
use crate::save::{SLOTS, SaveData, slot_name};
use crate::script::{Item, Script, cond};

pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub channel: i32,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

const FRAME_MS: f64 = 1000.0 / 60.0;
const STEPS: usize = 20_000;
/// layers `<ChrInit>` clears (the standing pictures; 0 is the background)
const CHARS: std::ops::RangeInclusive<i32> = 1..=7;

#[derive(Clone)]
struct Layer {
    name: String,
    img: Rc<Image>,
    x: i32,
    y: i32,
}

#[derive(Clone)]
struct Win {
    x: i32,
    y: i32,
    home: (i32, i32),
    rgb: [u8; 3],
    size: f32,
}

impl Default for Win {
    fn default() -> Win {
        Win {
            x: 0,
            y: 496,
            home: (264, 80),
            rgb: [255; 3],
            size: 24.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Until(f64),
    Click,
    /// a timed wait a click ends early
    ClickOr(f64),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Script,
    Title,
    Choice,
    Mask,
}

/// our save/load screen
struct Menu {
    save: bool,
    slots: Vec<Option<SaveData>>,
}

struct Button {
    label: String,
    dst: [i32; 4],
    normal: (i32, i32),
    over: (i32, i32),
}

struct Choice {
    file: String,
    label: String,
    text: String,
}

struct MaskChoice {
    id: i32,
    file: String,
    label: String,
    rgb: [u8; 3],
}

type TextKey = (String, u32, [u8; 3]);

pub struct Engine {
    /// the messages read so far (the wheel turned up)
    backlog: vncommon::backlog::Backlog,
    files: Box<dyn Files>,
    index: Index,
    scripts: HashMap<String, Rc<Script>>,
    cur: Rc<Script>,
    pc: usize,
    calls: Vec<(Rc<Script>, usize)>,
    vars: HashMap<String, i32>,
    images: HashMap<String, Rc<Image>>,
    stage: BTreeMap<i32, Layer>,
    view: BTreeMap<i32, Layer>,
    /// the screen before the last `<UpDate>`, fading out: picture, start, length
    fade: Option<(Rc<Image>, f64, f64)>,
    wins: HashMap<i32, Win>,
    win: i32,
    frame_on: bool,
    lines: Vec<String>,
    line: String,
    name: Option<String>,
    face: Option<Rc<Image>>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<TextKey, Rc<Image>>,
    now: f64,
    wait: Wait,
    mode: Mode,
    pointer: (i32, i32),
    clicked: bool,
    sounds: Vec<Sound>,
    loops: HashMap<i32, bool>,
    loaded: HashMap<i32, String>,
    choices: Vec<Choice>,
    mask: Option<Rc<Image>>,
    masks: Vec<MaskChoice>,
    overs: HashMap<i32, Rc<Image>>,
    title_bg: Option<Rc<Image>>,
    title_parts: Option<Rc<Image>>,
    buttons: Vec<Button>,
    /// name -> part of the name plate picture
    plates: HashMap<String, [i32; 4]>,
    ended: bool,
    pub screen: (u32, u32),
    pub log: bool,
    pub unknown: BTreeMap<String, u32>,
    pub text_log: Vec<String>,
    face_name: String,
    bgm: String,
    /// the page showing (taken at each `<KW>`), what a save writes
    page: Option<SaveData>,
    menu: Option<Menu>,
    cancelled: bool,
    /// start, length, amplitude x, y
    shake: Option<(f64, f64, i32, i32)>,
    /// layers shown as negatives
    nega: HashSet<i32>,
}

const MENU_TABS: [[i32; 4]; 3] = [[60, 36, 180, 48], [260, 36, 180, 48], [784, 36, 180, 48]];

fn inside(r: [i32; 4], (x, y): (i32, i32)) -> bool {
    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
}

fn slot_rect(k: usize) -> [i32; 4] {
    let (col, row) = ((k % 2) as i32, (k / 2) as i32);
    [60 + col * 464, 120 + row * 96, 440, 80]
}

fn num(s: Option<&String>) -> i32 {
    s.and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn place(alpha: f32, w: u32, h: u32) -> Place {
    Place {
        x: 0.0,
        y: 0.0,
        sx: 1.0,
        sy: 1.0,
        cx: 0.0,
        cy: 0.0,
        rot: 0.0,
        alpha,
        add: false,
        flip: (false, false),
        clip: [0.0, 0.0, w as f32, h as f32],
    }
}

/// The rows of a CSV setting (`.lib`: each byte is 0x82 more than the text's).
fn csv_rows(b: &[u8], masked: bool) -> Vec<Vec<String>> {
    let d: Vec<u8> = if masked {
        b.iter().map(|x| x.wrapping_sub(0x82)).collect()
    } else {
        b.to_vec()
    };
    crate::arc::sjis(&d)
        .lines()
        .map(|l| l.split(',').map(|c| c.trim().to_string()).collect())
        .collect()
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut index = Index::default();
        let mut arcs: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| n.to_lowercase().ends_with(".arc"))
            .collect();
        // patches last, so that they replace
        arcs.sort_by_key(|n| (n.to_lowercase().starts_with("patch"), n.to_lowercase()));
        for a in &arcs {
            if let Err(e) = index.add(files.as_mut(), a) {
                eprintln!("ateliergh: {e}");
            }
        }
        if index.files.is_empty() {
            return Err("no .arc archives".into());
        }
        let mut e = Engine {
            backlog: Default::default(),
            files,
            index,
            scripts: HashMap::new(),
            cur: Rc::new(Script::default()),
            pc: 0,
            calls: Vec::new(),
            vars: HashMap::new(),
            images: HashMap::new(),
            stage: BTreeMap::new(),
            view: BTreeMap::new(),
            fade: None,
            wins: HashMap::new(),
            win: 0,
            frame_on: false,
            lines: Vec::new(),
            line: String::new(),
            name: None,
            face: None,
            font: None,
            text_cache: HashMap::new(),
            now: 0.0,
            wait: Wait::None,
            mode: Mode::Script,
            pointer: (-1, -1),
            clicked: false,
            sounds: Vec::new(),
            loops: HashMap::new(),
            loaded: HashMap::new(),
            choices: Vec::new(),
            mask: None,
            masks: Vec::new(),
            overs: HashMap::new(),
            title_bg: None,
            title_parts: None,
            buttons: Vec::new(),
            plates: HashMap::new(),
            ended: false,
            screen: (1024, 768),
            log: false,
            unknown: BTreeMap::new(),
            text_log: Vec::new(),
            face_name: String::new(),
            bgm: String::new(),
            page: None,
            menu: None,
            cancelled: false,
            shake: None,
            nega: HashSet::new(),
        };
        e.load_ui();
        if e.index.files.contains_key("demo.txt") {
            e.goto("demo.txt", None)?;
        } else {
            e.mode = Mode::Title;
        }
        Ok(e)
    }

    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        self.index.read(self.files.as_mut(), name)
    }

    /// The title screen (picture, button parts and table) and the name plates.
    fn load_ui(&mut self) {
        let names: Vec<String> = self.index.files.keys().cloned().collect();
        let title = names
            .iter()
            .filter(|n| n.ends_with("title.bmp"))
            .min_by_key(|n| (!n.contains("_tr_"), n.len()))
            .cloned();
        if let Some(t) = title {
            self.title_bg = self.image(&t);
            let parts = t.replace("title.bmp", "title_parts.bmp");
            self.title_parts = self.image(&parts);
            // the trial's table has its own row for the trial's extra mode
            let table = if t.contains("_tr_") && names.iter().any(|n| n == "system4-1.lib") {
                "system4-1.lib"
            } else {
                "system4.lib"
            };
            if let Some(b) = self.read(table) {
                for r in csv_rows(&b, true).into_iter().skip(2) {
                    let n = |i: usize| num(r.get(i));
                    if r.len() < 10 || n(4) <= 0 || n(5) <= 0 {
                        continue;
                    }
                    self.buttons.push(Button {
                        label: r[0].clone(),
                        dst: [n(2), n(3), n(4), n(5)],
                        normal: (n(6), n(7)),
                        over: (n(8), n(9)),
                    });
                }
            }
        }
        if let Some(b) = self.read("name.csv") {
            for r in csv_rows(&b, false).into_iter().skip(1) {
                if r.len() >= 8 {
                    let n = |i: usize| num(r.get(i));
                    self.plates.insert(r[0].clone(), [n(4), n(5), n(6), n(7)]);
                }
            }
        }
    }

    fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(i) = self.images.get(&key) {
            return Some(i.clone());
        }
        let b = self.read(&key)?;
        let img = match crate::bmp::decode(&b).or_else(|_| Image::decode(&b)) {
            Ok(i) => Rc::new(i),
            Err(err) => {
                if self.log {
                    eprintln!("{name}: {err}");
                }
                return None;
            }
        };
        if self.images.len() > 40 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        Some(img)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(Rc::new(f));
        Ok(())
    }

    fn script(&mut self, file: &str) -> Result<Rc<Script>, String> {
        let key = file.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Ok(s.clone());
        }
        let b = self.read(&key).ok_or(format!("{file}: no such script"))?;
        let s = Rc::new(Script::parse(file, &crate::arc::sjis(&b)));
        self.scripts.insert(key, s.clone());
        Ok(s)
    }

    /// Continues at a label of a script (its start without one).
    fn goto(&mut self, file: &str, label: Option<&str>) -> Result<(), String> {
        let s = if file.is_empty() {
            self.cur.clone()
        } else {
            self.script(file)?
        };
        let pc = match label {
            Some(l) => *s
                .labels
                .get(&l.to_lowercase())
                .ok_or(format!("{}: no label {l}", s.name))?,
            None => 0,
        };
        self.cur = s;
        self.pc = pc;
        self.mode = Mode::Script;
        Ok(())
    }

    /// `file,label` or `label` of the current script.
    fn target(&mut self, a: &[String], call: bool) -> Result<(), String> {
        if call {
            self.calls.push((self.cur.clone(), self.pc));
        }
        match a {
            [f, l, ..] if f.to_lowercase().ends_with(".txt") => self.goto(f, Some(l)),
            [f] if f.to_lowercase().ends_with(".txt") => self.goto(f, None),
            [l, ..] => self.goto("", Some(l)),
            [] => Ok(()),
        }
    }

    fn var(&self, name: &str) -> i32 {
        self.vars.get(name).copied().unwrap_or(0)
    }

    fn back_to_title(&mut self) {
        self.mode = Mode::Title;
        self.calls.clear();
        self.stage.clear();
        self.view.clear();
        self.fade = None;
        self.clear_text();
        self.face = None;
        self.face_name.clear();
        self.frame_on = false;
        self.wait = Wait::None;
        self.page = None;
        self.shake = None;
        self.nega.clear();
    }

    // ---- saves ----

    fn save_data(&self) -> SaveData {
        let mut vars: Vec<(String, i32)> = self.vars.iter().map(|(k, v)| (k.clone(), *v)).collect();
        vars.sort();
        let mut wins: Vec<[i32; 9]> = self
            .wins
            .iter()
            .map(|(k, w)| {
                [
                    *k,
                    w.x,
                    w.y,
                    w.home.0,
                    w.home.1,
                    w.rgb[0] as i32,
                    w.rgb[1] as i32,
                    w.rgb[2] as i32,
                    w.size as i32,
                ]
            })
            .collect();
        wins.sort();
        let mut lines = self.lines.clone();
        if !self.line.is_empty() {
            lines.push(self.line.clone());
        }
        let mut loops: Vec<(i32, bool)> = self.loops.iter().map(|(k, v)| (*k, *v)).collect();
        loops.sort();
        SaveData {
            script: self.cur.name.clone(),
            pc: self.pc,
            calls: self
                .calls
                .iter()
                .map(|(s, pc)| (s.name.clone(), *pc))
                .collect(),
            vars,
            layers: self
                .view
                .iter()
                .map(|(k, l)| (*k, l.name.clone(), l.x, l.y))
                .collect(),
            face: self.face_name.clone(),
            frame_on: self.frame_on,
            wins,
            name: self.name.clone().unwrap_or_default(),
            lines,
            bgm: self.bgm.clone(),
            loops,
        }
    }

    fn open_menu(&mut self, save: bool) {
        let slots = (0..SLOTS)
            .map(|k| {
                self.files
                    .read_save(&slot_name(k))
                    .and_then(|b| SaveData::decode(&b))
            })
            .collect();
        self.menu = Some(Menu { save, slots });
    }

    fn load(&mut self, d: &SaveData) -> Result<(), String> {
        self.back_to_title();
        self.cur = self.script(&d.script)?;
        self.pc = d.pc;
        for (name, pc) in &d.calls {
            let s = self.script(name)?;
            self.calls.push((s, *pc));
        }
        self.vars = d.vars.iter().cloned().collect();
        for (k, name, x, y) in &d.layers {
            if let Some(img) = self.image(name) {
                let l = Layer {
                    name: name.clone(),
                    img,
                    x: *x,
                    y: *y,
                };
                self.stage.insert(*k, l);
            }
        }
        self.view = self.stage.clone();
        if !d.face.is_empty() {
            self.face = self.image(&d.face);
            self.face_name = d.face.clone();
        }
        self.frame_on = d.frame_on;
        for w in &d.wins {
            self.wins.insert(
                w[0],
                Win {
                    x: w[1],
                    y: w[2],
                    home: (w[3], w[4]),
                    rgb: [w[5] as u8, w[6] as u8, w[7] as u8],
                    size: w[8].max(8) as f32,
                },
            );
        }
        self.name = (!d.name.is_empty()).then(|| d.name.clone());
        self.lines = d.lines.clone();
        self.loops = d.loops.iter().cloned().collect();
        self.stop(-1, 0);
        if !d.bgm.is_empty() {
            self.bgm = d.bgm.clone();
            let f = d.bgm.clone();
            self.sound(0, &f);
        }
        self.mode = Mode::Script;
        self.wait = Wait::Click;
        self.page = Some(d.clone());
        Ok(())
    }

    /// The save/load screen's turn; true while it is open.
    fn menu_frame(&mut self, click: bool, cancel: bool) -> Result<bool, String> {
        let Some(m) = &self.menu else {
            return Ok(false);
        };
        if cancel {
            self.menu = None;
            return Ok(true);
        }
        if !click {
            return Ok(true);
        }
        let save = m.save;
        let p = self.pointer;
        if inside(MENU_TABS[0], p) && self.page.is_some() {
            self.open_menu(true);
        } else if inside(MENU_TABS[1], p) {
            self.open_menu(false);
        } else if inside(MENU_TABS[2], p) {
            self.menu = None;
        } else if let Some(k) = (0..SLOTS).find(|&k| inside(slot_rect(k), p)) {
            if save {
                if let Some(d) = self.page.clone() {
                    self.files.write_save(&slot_name(k), &d.encode());
                    self.open_menu(true);
                }
            } else if let Some(Some(d)) = self.menu.as_ref().map(|m| m.slots[k].clone()) {
                self.menu = None;
                self.load(&d)?;
            }
        }
        Ok(true)
    }

    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(save) = self.menu.as_ref().map(|m| m.save) else {
            return;
        };
        let dim = self.solid(self.screen.0, self.screen.1, [0, 0, 0, 200]);
        self.whole(out, &dim, 0, 0, 1.0);
        let tabs = [("セーブ", save), ("ロード", !save), ("閉じる", false)];
        for (i, (label, on)) in tabs.iter().enumerate() {
            let r = MENU_TABS[i];
            let hover = inside(r, self.pointer);
            let bg = self.solid(
                r[2] as u32,
                r[3] as u32,
                if *on {
                    [90, 60, 130, 230]
                } else if hover {
                    [70, 70, 90, 230]
                } else {
                    [40, 40, 50, 230]
                },
            );
            self.whole(out, &bg, r[0], r[1], 1.0);
            let w = label.chars().count() as i32 * 26;
            self.text_at(out, label, 26.0, [255; 3], r[0] + (r[2] - w) / 2, r[1] + 11);
        }
        for k in 0..SLOTS {
            let r = slot_rect(k);
            let hover = inside(r, self.pointer);
            let bg = self.solid(
                r[2] as u32,
                r[3] as u32,
                if hover {
                    [70, 60, 100, 230]
                } else {
                    [30, 30, 40, 230]
                },
            );
            self.whole(out, &bg, r[0], r[1], 1.0);
            let summary = self
                .menu
                .as_ref()
                .and_then(|m| m.slots[k].as_ref().map(|d| d.summary()));
            let title = format!("No.{:02}", k + 1);
            self.text_at(out, &title, 20.0, [255, 220, 160], r[0] + 12, r[1] + 10);
            let text = summary.unwrap_or_else(|| "―".into());
            self.text_at(out, &text, 20.0, [255; 3], r[0] + 12, r[1] + 44);
        }
    }

    fn clear_text(&mut self) {
        self.lines.clear();
        self.line.clear();
        self.name = None;
    }

    fn snapshot(&mut self) -> Rc<Image> {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.layer_draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        Rc::new(img)
    }

    fn sound(&mut self, ch: i32, file: &str) {
        let Some(data) = self.read(file) else {
            if self.log {
                eprintln!("no sound {file}");
            }
            return;
        };
        let looped = self.loops.get(&ch).copied().unwrap_or(ch < 2);
        self.sounds.push(Sound {
            data,
            volume: 1.0,
            channel: ch,
            stop: false,
            fade_ms: 0,
            looped,
        });
    }

    fn stop_music(&mut self, ch: i32, ms: i32) {
        if ch == 0 {
            self.bgm.clear();
        }
        self.stop(ch, ms);
    }

    fn stop(&mut self, ch: i32, ms: i32) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel: ch,
            stop: true,
            fade_ms: ms.max(0) as u32,
            looped: false,
        });
    }

    fn tag(&mut self, cmd: &str, a: &[String]) -> Result<(), String> {
        let n = |i: usize| num(a.get(i));
        let s = |i: usize| a.get(i).map(String::as_str).unwrap_or("");
        match cmd.to_ascii_lowercase().as_str() {
            "kw" => {
                if !self.line.is_empty() || !self.lines.is_empty() {
                    let mut all = self.lines.clone();
                    if !self.line.is_empty() {
                        all.push(self.line.clone());
                    }
                    let text = match &self.name {
                        Some(n) => format!("{n}{}", all.join("")),
                        None => all.join(""),
                    };
                    self.text_log.push(text);
                }
                self.wait = Wait::Click;
                self.page = Some(self.save_data());
            }
            "winclear" => self.clear_text(),
            "winrubi" => self.line.push_str(s(0)),
            "face" => {
                self.face = if s(0).eq_ignore_ascii_case("off") {
                    None
                } else {
                    self.image(s(0))
                };
                self.face_name = if self.face.is_some() {
                    s(0).to_string()
                } else {
                    String::new()
                };
            }
            "winframe" => self.frame_on = s(0).eq_ignore_ascii_case("on"),
            "wincurrent" => self.win = n(0),
            "winpos" => {
                let w = self.wins.entry(self.win).or_default();
                (w.x, w.y) = (n(0), n(1));
            }
            "winmsghome" => self.wins.entry(self.win).or_default().home = (n(0), n(1)),
            "windeffontcolor" => {
                self.wins.entry(self.win).or_default().rgb = [n(0) as u8, n(1) as u8, n(2) as u8]
            }
            "windeffontsize" => self.wins.entry(self.win).or_default().size = n(1).max(8) as f32,
            "imageload" => {
                if let Some(img) = self.image(s(1)) {
                    let (x, y) = self.stage.get(&n(0)).map_or((0, 0), |l| (l.x, l.y));
                    let name = s(1).to_string();
                    self.stage.insert(n(0), Layer { name, img, x, y });
                } else {
                    self.stage.remove(&n(0));
                }
            }
            "imagepos" => {
                if let Some(l) = self.stage.get_mut(&n(0)) {
                    (l.x, l.y) = (n(1), n(2));
                }
            }
            "imageinit" => {
                self.stage.remove(&n(0));
            }
            "chrinit" => self.stage.retain(|k, _| !CHARS.contains(k)),
            "update" => {
                let ms = a
                    .iter()
                    .rev()
                    .find_map(|x| x.parse::<i32>().ok())
                    .unwrap_or(0);
                let ms = if s(0).eq_ignore_ascii_case("normal") {
                    0
                } else {
                    ms
                };
                if ms > 0 {
                    let old = self.snapshot();
                    self.fade = Some((old, self.now, ms as f64));
                    self.wait = Wait::Until(self.now + ms as f64);
                } else {
                    self.fade = None;
                }
                self.view = self.stage.clone();
            }
            "tw" => {
                let until = self.now + n(0).max(0) as f64;
                self.wait = Wait::ClickOr(until);
            }
            "jump" => self.target(a, false)?,
            "jumpr" => self.target(a, true)?,
            "return" => match self.calls.pop() {
                Some((s, pc)) => {
                    self.cur = s;
                    self.pc = pc;
                }
                None => self.back_to_title(),
            },
            "if" => {
                if let Some((c, dest)) = a.split_last()
                    && cond(c, &|v| self.var(v))
                {
                    self.target(dest, false)?;
                }
            }
            "mov" => {
                let v = s(1).parse().unwrap_or_else(|_| self.var(s(1)));
                self.vars.insert(s(0).to_string(), v);
            }
            "sltdel" => self.choices.clear(),
            "sltadd" => self.choices.push(Choice {
                file: s(1).to_string(),
                label: s(2).to_string(),
                text: s(3).to_string(),
            }),
            "sltrun" => {
                if !self.choices.is_empty() {
                    self.mode = Mode::Choice;
                }
            }
            "sltmaskdel" => {
                self.masks.clear();
                self.overs.clear();
            }
            "sltmaskhitset" => self.mask = self.image(s(0)),
            "sltmaskadd" => self.masks.push(MaskChoice {
                id: n(0),
                file: s(1).to_string(),
                label: s(2).to_string(),
                rgb: [n(3) as u8, n(4) as u8, n(5) as u8],
            }),
            "sltmaskoverset" => {
                if let Some(i) = self.image(s(1)) {
                    self.overs.insert(n(0), i);
                }
            }
            "sltmaskrun" => {
                if self.mask.is_some() && !self.masks.is_empty() {
                    self.mode = Mode::Mask;
                }
            }
            "soundloop" => {
                self.loops.insert(n(0), s(1).eq_ignore_ascii_case("on"));
            }
            "bgm" => {
                if n(0) == 0 {
                    self.bgm = s(1).to_string();
                }
                self.sound(n(0), s(1))
            }
            "se" => self.sound(2 + n(0), s(1)),
            "voice" => self.sound(3 + n(0), s(1)),
            "soundload" => {
                self.loaded.insert(n(0), s(1).to_string());
            }
            "soundrun" => {
                if s(1).eq_ignore_ascii_case("play") {
                    if let Some(f) = self.loaded.get(&n(0)).cloned() {
                        self.sound(n(0), &f);
                    }
                } else {
                    self.stop_music(n(0), n(3));
                }
            }
            "soundfade" => self.stop_music(n(0), n(2)),
            "imageshake" => {
                let ms = n(1).max(1) as f64 * FRAME_MS;
                self.shake = Some((self.now, ms, n(2), n(3)));
                self.wait = Wait::Until(self.now + ms);
            }
            "imagenegaposi" => {
                if s(1).eq_ignore_ascii_case("on") {
                    self.nega.insert(n(0));
                } else {
                    self.nega.remove(&n(0));
                }
            }
            "titlereturn" => self.back_to_title(),
            "systemicon" | "titlecancel" | "imagerate" | "winimage" => {}
            _ => *self.unknown.entry(cmd.to_string()).or_default() += 1,
        }
        Ok(())
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(item) = self.cur.items.get(self.pc).cloned() else {
            // the end of a script: back to its caller, or the title
            match self.calls.pop() {
                Some((s, pc)) => {
                    self.cur = s;
                    self.pc = pc;
                }
                None => self.back_to_title(),
            }
            return Ok(());
        };
        self.pc += 1;
        if self.log {
            eprintln!("{} {:?}", self.cur.name, item);
        }
        match item {
            Item::Label(_) => {}
            Item::Text(t) => {
                if self.line.is_empty() && t.starts_with('【') && t.ends_with('】') {
                    self.name = Some(t.trim_matches(['【', '】']).to_string());
                    self.line.clear();
                    // the name line ends here
                    if let Some(Item::Break) = self.cur.items.get(self.pc) {
                        self.pc += 1;
                    }
                } else {
                    self.line.push_str(&t);
                }
            }
            Item::Break => {
                if !self.line.is_empty() {
                    self.lines.push(std::mem::take(&mut self.line));
                }
            }
            Item::Tag(cmd, args) => self.tag(&cmd, &args)?,
        }
        Ok(())
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.log_page();
        self.now += FRAME_MS;
        let click = std::mem::take(&mut self.clicked);
        let cancel = std::mem::take(&mut self.cancelled);
        if self.menu_frame(click, cancel)? {
            return Ok(());
        }
        if cancel {
            match self.mode {
                Mode::Title => self.open_menu(false),
                Mode::Script if self.wait == Wait::Click && self.page.is_some() => {
                    self.open_menu(true)
                }
                _ => {}
            }
            if self.menu.is_some() {
                return Ok(());
            }
        }
        if let Some((start, ms, _, _)) = self.shake
            && self.now >= start + ms
        {
            self.shake = None;
        }
        if let Some((_, start, ms)) = self.fade
            && self.now >= start + ms
        {
            self.fade = None;
        }
        match self.mode {
            Mode::Title => {
                if click && let Some(i) = self.button_at(self.pointer) {
                    let label = self.buttons[i].label.clone();
                    self.start_from_title(&label)?;
                }
                return Ok(());
            }
            Mode::Choice => {
                if click && let Some(i) = self.choice_at(self.pointer) {
                    let c = &self.choices[i];
                    let (f, l) = (c.file.clone(), c.label.clone());
                    self.choices.clear();
                    self.clear_text();
                    self.goto(&f, Some(&l))?;
                }
                return Ok(());
            }
            Mode::Mask => {
                if click && let Some(i) = self.mask_at(self.pointer) {
                    let c = &self.masks[i];
                    let (f, l) = (c.file.clone(), c.label.clone());
                    self.goto(&f, Some(&l))?;
                }
                return Ok(());
            }
            Mode::Script => {}
        }
        match self.wait {
            Wait::Click if click => self.wait = Wait::None,
            Wait::Until(t) if self.now >= t => self.wait = Wait::None,
            Wait::ClickOr(t) if click || self.now >= t => self.wait = Wait::None,
            _ => {}
        }
        let mut steps = 0;
        while self.wait == Wait::None && self.mode == Mode::Script && !self.ended {
            self.step()?;
            steps += 1;
            if steps > STEPS {
                return Err(format!("{}: no wait in {STEPS} steps", self.cur.name));
            }
        }
        Ok(())
    }

    fn start_from_title(&mut self, label: &str) -> Result<(), String> {
        let l = label.to_lowercase();
        if l.contains("start") {
            self.goto("main.txt", None)
        } else if l.contains("えろ") || l.contains("mode") {
            let sel = self
                .index
                .files
                .keys()
                .find(|n| n.ends_with("_sel.txt"))
                .cloned();
            match sel {
                Some(s) => self.goto(&s, None),
                None => Ok(()),
            }
        } else if l == "end" || l.contains("exit") || l.contains("quit") {
            self.ended = true;
            Ok(())
        } else if l.contains("load") {
            self.open_menu(false);
            Ok(())
        } else {
            // settings: not yet
            Ok(())
        }
    }

    fn button_at(&self, (x, y): (i32, i32)) -> Option<usize> {
        self.buttons.iter().position(|b| {
            x >= b.dst[0] && y >= b.dst[1] && x < b.dst[0] + b.dst[2] && y < b.dst[1] + b.dst[3]
        })
    }

    fn choice_rects(&self) -> Vec<[i32; 4]> {
        let (w, h, gap) = (640, 48, 16);
        let n = self.choices.len() as i32;
        let top = (self.screen.1 as i32 - 272 - n * (h + gap)) / 2 + 40;
        (0..n)
            .map(|i| [(self.screen.0 as i32 - w) / 2, top + i * (h + gap), w, h])
            .collect()
    }

    fn choice_at(&self, (x, y): (i32, i32)) -> Option<usize> {
        self.choice_rects()
            .iter()
            .position(|r| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
    }

    fn mask_at(&self, (x, y): (i32, i32)) -> Option<usize> {
        let m = self.mask.as_ref()?;
        if x < 0 || y < 0 || x >= m.w as i32 || y >= m.h as i32 {
            return None;
        }
        let o = ((y as u32 * m.w + x as u32) * 4) as usize;
        let c = [m.px[o], m.px[o + 1], m.px[o + 2]];
        self.masks.iter().position(|e| e.rgb == c)
    }

    // ---- drawing ----

    fn text_image(&mut self, s: &str, size: f32, rgb: [u8; 3]) -> Option<Rc<Image>> {
        let font = self.font.clone()?;
        let key = (s.to_string(), size as u32, rgb);
        if let Some(i) = self.text_cache.get(&key) {
            return Some(i.clone());
        }
        if self.text_cache.len() > 128 {
            self.text_cache.clear();
        }
        let img = Rc::new(crate::text::render(&font, s, size, rgb, [0x20, 0x20, 0x20]));
        self.text_cache.insert(key, img.clone());
        Some(img)
    }

    fn put(&self, out: &mut Vec<Draw>, img: &Rc<Image>, src: [i32; 4], x: i32, y: i32, a: f32) {
        let (w, h) = (src[2] - src[0], src[3] - src[1]);
        if w <= 0 || h <= 0 {
            return;
        }
        out.push(Draw {
            image: img.clone(),
            src,
            dst: [x as f32, y as f32, (x + w) as f32, (y + h) as f32],
            place: place(a, self.screen.0, self.screen.1),
        });
    }

    fn whole(&self, out: &mut Vec<Draw>, img: &Rc<Image>, x: i32, y: i32, a: f32) {
        self.put(out, img, [0, 0, img.w as i32, img.h as i32], x, y, a);
    }

    fn layer_draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        let (dx, dy) = match self.shake {
            Some((start, ms, ax, ay)) => {
                let t = ((self.now - start) / ms).clamp(0.0, 1.0);
                let k = (1.0 - t) * (t * ms / 40.0 * std::f64::consts::PI).sin();
                ((ax as f64 * k) as i32, (ay as f64 * k) as i32)
            }
            None => (0, 0),
        };
        let layers: Vec<(i32, Layer)> = self.view.iter().map(|(k, l)| (*k, l.clone())).collect();
        for (k, l) in layers {
            let img = if self.nega.contains(&k) {
                self.negative(&l.img)
            } else {
                l.img.clone()
            };
            self.whole(&mut out, &img, l.x + dx, l.y + dy, 1.0);
        }
        out
    }

    fn negative(&mut self, img: &Rc<Image>) -> Rc<Image> {
        let key = format!("#neg{}", img.id);
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let mut n = (**img).clone();
        for p in n.px.chunks_exact_mut(4) {
            for c in &mut p[..3] {
                *c = 255 - *c;
            }
        }
        let n = Rc::new(n);
        self.images.insert(key, n.clone());
        n
    }

    fn text_at(&mut self, out: &mut Vec<Draw>, s: &str, size: f32, rgb: [u8; 3], x: i32, y: i32) {
        if let Some(img) = self.text_image(s, size, rgb) {
            let p = crate::text::PAD;
            self.whole(out, &img, x - p, y - p, 1.0);
        }
    }

    fn solid(&mut self, w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
        let key = format!("#solid{w}x{h}:{rgba:?}");
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let mut img = Image::new(w, h);
        img.fill(rgba);
        let img = Rc::new(img);
        self.images.insert(key, img.clone());
        img
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = self.draws_inner();
        if self.backlog.is_open() {
            let log = std::mem::take(&mut self.backlog);
            let (w, h) = self.screen;
            let size = (h as i32 / 27).max(16);
            out.extend(log.draws(w, h, size, &mut |s, rgb| {
                let _ = rgb;
                self.text_image(s, size as f32, rgb)
                    .unwrap_or_else(|| Rc::new(Image::new(1, 1)))
            }));
            self.backlog = log;
        }
        out
    }

    /// Keeps the message waiting for a click in the backlog (vncommon::backlog).
    fn log_page(&mut self) {
        if !(self.mode == Mode::Script && matches!(self.wait, Wait::Click)) {
            return;
        }
        let (name, text) = (self.name.clone().unwrap_or_default(), {
            let mut all = self.lines.clone();
            if !self.line.is_empty() {
                all.push(self.line.clone());
            }
            all.join("")
        });
        self.backlog.push(name.trim(), &text);
    }

    fn draws_inner(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        if self.mode == Mode::Title {
            if let Some(bg) = self.title_bg.clone() {
                self.whole(&mut out, &bg, 0, 0, 1.0);
            }
            if let Some(parts) = self.title_parts.clone() {
                let hover = self.button_at(self.pointer);
                for (i, b) in self.buttons.iter().enumerate() {
                    let (sx, sy) = if hover == Some(i) { b.over } else { b.normal };
                    self.put(
                        &mut out,
                        &parts,
                        [sx, sy, sx + b.dst[2], sy + b.dst[3]],
                        b.dst[0],
                        b.dst[1],
                        1.0,
                    );
                }
            }
            self.menu_draws(&mut out);
            return out;
        }
        let layers = self.layer_draws();
        out.extend(layers);
        if let Some((old, start, ms)) = self.fade.clone() {
            let t = ((self.now - start) / ms).clamp(0.0, 1.0) as f32;
            self.whole(&mut out, &old, 0, 0, 1.0 - t);
        }
        if self.mode == Mode::Mask {
            let hover = self.mask_at(self.pointer).map(|i| self.masks[i].id);
            if let Some(img) = hover.and_then(|id| self.overs.get(&id).cloned()) {
                self.whole(&mut out, &img, 0, 0, 1.0);
            }
        }
        let win = self.wins.get(&0).cloned().unwrap_or_default();
        let shown = self.frame_on && self.mode == Mode::Script || self.mode == Mode::Choice;
        if shown {
            if let Some(w) = self.image("win_a.bmp") {
                self.whole(&mut out, &w, win.x, win.y, 1.0);
            }
            if let Some(f) = self.face.clone() {
                let y = self.screen.1 as i32 - f.h as i32;
                self.whole(&mut out, &f, win.x + 8, y, 1.0);
            }
            if let Some(name) = self.name.clone() {
                let plate = self.plates.get(&name).copied();
                match (plate, self.image("name.bmp")) {
                    (Some(r), Some(img)) => self.put(
                        &mut out,
                        &img,
                        [r[0], r[1], r[0] + r[2], r[1] + r[3]],
                        win.x + win.home.0 - 16,
                        win.y + win.home.1 - r[3] - 6,
                        1.0,
                    ),
                    _ => self.text_at(
                        &mut out,
                        &format!("【{name}】"),
                        win.size,
                        win.rgb,
                        win.x + win.home.0,
                        win.y + win.home.1 - win.size as i32 - 12,
                    ),
                }
            }
            let mut lines = self.lines.clone();
            if !self.line.is_empty() {
                lines.push(self.line.clone());
            }
            let lh = (win.size * 1.4).round() as i32;
            for (i, l) in lines.iter().enumerate() {
                self.text_at(
                    &mut out,
                    l,
                    win.size,
                    win.rgb,
                    win.x + win.home.0,
                    win.y + win.home.1 + i as i32 * lh,
                );
            }
        }
        if self.mode == Mode::Choice {
            let hover = self.choice_at(self.pointer);
            let rects = self.choice_rects();
            for (i, r) in rects.iter().enumerate() {
                let bg = if hover == Some(i) {
                    self.solid(r[2] as u32, r[3] as u32, [60, 40, 90, 220])
                } else {
                    self.solid(r[2] as u32, r[3] as u32, [0, 0, 0, 180])
                };
                self.whole(&mut out, &bg, r[0], r[1], 1.0);
                let text = self.choices[i].text.clone();
                let size = 26.0;
                let tw = text.chars().count() as i32 * size as i32;
                self.text_at(
                    &mut out,
                    &text,
                    size,
                    [255; 3],
                    r[0] + (r[2] - tw) / 2,
                    r[1] + (r[3] - size as i32) / 2,
                );
            }
        }
        self.menu_draws(&mut out);
        out
    }

    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        if x >= 0 {
            self.pointer = (x, y);
        }
        self.clicked = true;
    }

    /// A right click: our save screen on a page, the load screen at the title; again closes it.
    pub fn cancel(&mut self) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        self.cancelled = true;
    }

    /// 4 (enter) advances like a click.
    pub fn key(&mut self, k: i32) {
        if k == 4 {
            self.clicked = true;
        }
    }

    pub fn wheel(&mut self, up: bool) {
        // turned up: the backlog
        if self.backlog.is_open() || up {
            self.log_page();
            if self.backlog.is_open()
                || (self.mode == Mode::Script && matches!(self.wait, Wait::Click))
            {
                self.backlog.wheel(up);
            }
        }
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    pub fn ended(&self) -> bool {
        self.ended
    }
}
