//! Runs a System-NNN game: the opening and title from the settings (`init/opening.xtx`,
//! `init/title.xtx`) and their back script, then the story from the first script of
//! `spt/project.xtx`.
//!
//! The scripts are made of films (a film table in the header; calls push the return position),
//! and a film of commands. A command sets up the layers (a picture, where it goes, an effect),
//! then runs: frames (`22220004`), a change from the screen before (`22220005`, after
//! `33330011` kept that screen) and messages that wait for a click; the command ends with
//! `5556000e`. Layers keep their pictures from one command to the next. The variables are
//! numbered; variable 30 holds the mode the back script draws for (3 the opening, 4 the title).
//! Saves are our own (ten slots from the right-click menu).

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::pack::Index;
use crate::spt::Script;
use crate::xtx::{self, Config};

/// A sound to start or stop.
pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub channel: i32,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
/// the most records run in one frame before the engine gives up on the script
const STEPS: usize = 100_000;
const BGM: i32 = 0;
const VOICE: i32 = 1;
const SE: i32 = 100;
/// the variable that holds the mode for the back script, and the modes
const MODE_VAR: u32 = 30;
const OPENING: i32 = 3;
const TITLE: i32 = 4;
/// where Before Dawn Daybreak's done requests are kept (variable REQUEST_DONE + number)
const REQUEST_DONE: u32 = 100_000;
/// where EXTRAVAGANZA's opened and played chart slots are kept
const SLOT_OPEN: u32 = 200_000;
const SLOT_DONE: u32 = 300_000;

#[derive(Clone)]
struct Effect {
    kind: i32,
    p: Vec<i32>,
    /// when it started (ms); 0 when it is over
    start: u64,
}

#[derive(Clone)]
struct Layer {
    pic: String,
    image: Rc<Image>,
    /// where the picture starts and where it is (x, y, w, h)
    a: [i32; 4],
    b: [i32; 4],
    effect: Option<Effect>,
}

#[derive(Clone, PartialEq)]
enum Wait {
    None,
    Until(u64),
    Click,
    /// the choices of a select and where they are
    Select(Vec<(String, [i32; 4])>),
    /// nothing more to run (the title after its back script)
    Idle,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Opening,
    Title,
    Game,
}

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Main,
    Save,
    Load,
}

#[derive(Clone, Default)]
struct Msg {
    name: String,
    lines: Vec<String>,
}

/// One running script: where, and the calls it is in.
#[derive(Clone)]
struct Frame {
    script: String,
    pc: usize,
    stack: Vec<usize>,
}

struct Fade {
    from: Vec<Draw>,
    start: u64,
    ms: u64,
}

struct Button {
    image: Rc<Image>,
    rect: [i32; 4],
    /// where the lit picture is in the image
    lit: (i32, i32),
}

pub struct Engine {
    files: Box<dyn Files>,
    fxf: bool,
    configs: HashMap<String, Config>,
    packs: HashMap<String, Option<Rc<Index>>>,
    scripts: HashMap<String, Rc<Script>>,
    bgm_list: Vec<String>,
    se_list: Vec<String>,
    first_script: String,
    pub screen: (u32, u32),
    frame_ms: u64,
    mode: Mode,
    run: Frame,
    /// the scripts that called the running one
    calls: Vec<Frame>,
    vars: HashMap<u32, i32>,
    layers: BTreeMap<i32, Layer>,
    kept: Option<Vec<Draw>>,
    fade: Option<Fade>,
    in_command: bool,
    msg: Option<Msg>,
    /// where the shown message's record is (a save starts there)
    msg_pc: usize,
    voice: Option<String>,
    wait: Wait,
    buttons: Vec<Button>,
    /// the variable a select sets and the value of each choice
    choice: Option<(u32, Vec<i32>)>,
    /// Before Dawn Daybreak's requests (bdd/iraiList.xtx): number and description
    requests: Vec<(i32, String)>,
    /// EXTRAVAGANZA's chart (chart/chart*.xtx or .fxf): slot, the slots it leads to, its text
    chart: Vec<(i32, Vec<i32>, String)>,
    /// the variable the chosen slot goes to (スロット番号)
    slot_var: u32,
    title_start: u64,
    title_wait: u64,
    window: Option<Rc<Image>>,
    name_window: Option<Rc<Image>>,
    bgm: Option<i32>,
    sounds: Vec<Sound>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    pictures: HashMap<String, Rc<Image>>,
    now: u64,
    frames: u64,
    pointer: (i32, i32),
    menu: Option<Menu>,
    ended: bool,
    pub log: bool,
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(w, h);
    i.fill(rgba);
    Rc::new(i)
}

impl Engine {
    pub fn new(files: Box<dyn Files>) -> Result<Engine, String> {
        let mut e = Engine {
            files,
            fxf: false,
            configs: HashMap::new(),
            packs: HashMap::new(),
            scripts: HashMap::new(),
            bgm_list: Vec::new(),
            se_list: Vec::new(),
            first_script: "sys".into(),
            screen: (800, 600),
            frame_ms: 50,
            mode: Mode::Opening,
            run: Frame {
                script: String::new(),
                pc: 0,
                stack: Vec::new(),
            },
            calls: Vec::new(),
            vars: HashMap::new(),
            layers: BTreeMap::new(),
            kept: None,
            fade: None,
            in_command: false,
            msg: None,
            msg_pc: 0,
            voice: None,
            wait: Wait::None,
            buttons: Vec::new(),
            choice: None,
            requests: Vec::new(),
            chart: Vec::new(),
            slot_var: 310,
            title_start: 0,
            title_wait: 0,
            window: None,
            name_window: None,
            bgm: None,
            sounds: Vec::new(),
            font: None,
            text_cache: HashMap::new(),
            pictures: HashMap::new(),
            now: 0,
            frames: 0,
            pointer: (-1, -1),
            menu: None,
            ended: false,
            log: false,
        };
        e.fxf = e.files.size("init/game.xtx").is_none() && e.files.size("init2/game.fxf").is_some();
        let project = e
            .spt_text("project")
            .ok_or("System-NNN: no spt/project.xtx")?;
        if let Some(first) = xtx::list(&project).into_iter().find(|s| !s.is_empty()) {
            e.first_script = first;
        }
        e.bgm_list = e
            .spt_text("bgmlist")
            .map(|t| xtx::list(&t))
            .unwrap_or_default();
        e.se_list = e
            .spt_text("selist")
            .map(|t| xtx::list(&t))
            .unwrap_or_default();
        if let Some(t) = e.spt_text("exesetup") {
            let c = Config::parse(&t);
            if let (Some(w), Some(h)) =
                (c.i("mainScreenWindowSizeX1"), c.i("mainScreenWindowSizeY1"))
                && w > 0
                && h > 0
            {
                e.screen = (w as u32, h as u32);
            }
        }
        if let Some(b) = e.file("bdd/irailist.xtx") {
            e.requests = requests(&xtx::text(&b, false));
        }
        for i in 1..=9 {
            // .xtx, or (later titles) .fxf XORed with 0xFF
            if let Some(b) = e.file(&format!("chart/chart{i}.xtx")) {
                e.chart.extend(chart(&xtx::text(&b, false)));
            } else if let Some(b) = e.file(&format!("chart/chart{i}.fxf")) {
                e.chart.extend(chart(&xtx::text(&b, true)));
            }
        }
        if let Some(t) = e.spt_text("var") {
            let names: Vec<String> = t
                .lines()
                .flat_map(|l| l.split(',').take(5).map(|s| s.trim().to_string()))
                .collect();
            if let Some(i) = names.iter().position(|n| n == "スロット番号") {
                e.slot_var = i as u32;
            }
        }
        let t = e.config("game").i_or("frameTime", 50);
        e.frame_ms = t.clamp(5, 200) as u64;
        e.load_script(&e.first_script.clone())?;
        e.start_opening();
        Ok(e)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())?;
        self.font = Some(Rc::new(f));
        self.text_cache.clear();
        Ok(())
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("[{} {}] {s}", self.run.script, self.run.pc);
        }
    }

    // ---- files ----

    fn file(&mut self, path: &str) -> Option<Vec<u8>> {
        let p = path.replace('\\', "/").to_lowercase();
        self.files.read(&p)
    }

    fn spt_text(&mut self, name: &str) -> Option<String> {
        if let Some(b) = self.file(&format!("spt/{name}.xtx")) {
            return Some(xtx::text(&b, false));
        }
        self.file(&format!("spt/{name}.fxf"))
            .map(|b| xtx::text(&b, true))
    }

    fn config(&mut self, name: &str) -> Config {
        if let Some(c) = self.configs.get(name) {
            return c.clone();
        }
        let c = match self.file(&format!("init/{name}.xtx")) {
            Some(b) => Config::parse(&xtx::text(&b, false)),
            None => self
                .file(&format!("init2/{name}.fxf"))
                .map(|b| Config::parse(&xtx::text(&b, true)))
                .unwrap_or_default(),
        };
        self.configs.insert(name.to_string(), c.clone());
        c
    }

    fn load_script(&mut self, name: &str) -> Result<Rc<Script>, String> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Ok(s.clone());
        }
        let b = self
            .file(&format!("spt/{key}.spt"))
            .ok_or_else(|| format!("System-NNN: no spt/{name}.spt"))?;
        let s = Rc::new(Script::parse(&key, &b)?);
        self.scripts.insert(key, s.clone());
        Ok(s)
    }

    fn pack(&mut self, path: &str, vtb: bool) -> Option<Rc<Index>> {
        if let Some(i) = self.packs.get(path) {
            return i.clone();
        }
        let base = path.rsplit_once('.').map_or(path, |p| p.0).to_string();
        let index = self.files.size(path).and_then(|size| {
            let t = self.file(&format!("{base}.{}", if vtb { "vtb" } else { "gtb" }))?;
            if vtb {
                Index::vtb(&t, size)
            } else {
                Index::gtb(&t, size)
            }
        });
        let index = index.map(Rc::new);
        self.packs.insert(path.to_string(), index.clone());
        index
    }

    fn pack_entry(&mut self, path: &str, vtb: bool, name: &str) -> Option<Vec<u8>> {
        let i = self.pack(path, vtb)?;
        let &(off, len) = i.entries.get(&name.to_lowercase())?;
        self.files.read_range(path, off, len)
    }

    /// A picture by name: `xx\name` or `name` (the pack is the folder, or the first two
    /// letters of the name); `system` pictures look in the sys pack first.
    fn picture(&mut self, name: &str, system: bool) -> Option<Rc<Image>> {
        let name = name.replace('/', "\\").to_lowercase();
        if let Some(i) = self.pictures.get(&name) {
            return Some(i.clone());
        }
        let (dir, base) = match name.rsplit_once('\\') {
            Some((d, b)) => (Some(d.to_string()), b.to_string()),
            None => (None, name.clone()),
        };
        let mut packs: Vec<String> = Vec::new();
        if let Some(d) = &dir {
            packs.push(d.clone());
        }
        let prefix: String = base.chars().take(2).collect();
        if system {
            packs.push("sys".into());
        }
        packs.push(prefix);
        packs.push("sys".into());
        let mut found = None;
        'outer: for p in packs {
            // loose pictures: dwq/<pack>/<name>.dwq
            if let Some(b) = self.file(&format!("dwq/{p}/{base}.dwq")) {
                found = Some(b);
                break;
            }
            for n in 0..10 {
                let path = format!("dwq/{p}{n}.gpk");
                if self.pack(&path, false).is_none() {
                    if n > 0 {
                        break;
                    }
                    continue;
                }
                if let Some(b) = self.pack_entry(&path, false, &base) {
                    found = Some(b);
                    break 'outer;
                }
            }
        }
        // loose files: dwq/<name>.dwq (some games keep pictures unpacked)
        let b = match found {
            Some(b) => b,
            None => self
                .file(&format!("dwq/{}.dwq", name))
                .or_else(|| self.file(&format!("dwq/{base}.dwq")))?,
        };
        match crate::dwq::decode(&b) {
            Ok(img) => {
                let img = Rc::new(img);
                if self.pictures.len() > 64 {
                    self.pictures.clear();
                }
                self.pictures.insert(name, img.clone());
                Some(img)
            }
            Err(err) => {
                self.trace(&format!("picture {name}: {err}"));
                None
            }
        }
    }

    // ---- modes ----

    fn start_opening(&mut self) {
        let c = self.config("opening");
        match c.s("backScriptName").map(str::to_string) {
            Some(back) if c.i_or("backScriptFlag", 1) != 0 && self.load_script(&back).is_ok() => {
                self.mode = Mode::Opening;
                self.vars.insert(MODE_VAR, OPENING);
                self.enter(&back);
            }
            _ => self.start_title(),
        }
    }

    fn start_title(&mut self) {
        self.mode = Mode::Title;
        self.reset_screen();
        self.calls.clear();
        self.buttons.clear();
        let c = self.config("title");
        for i in 1..=12 {
            let Some(f) = c.s(&format!("menu1Button{i}FileName")).map(str::to_string) else {
                continue;
            };
            let Some(image) = self.picture(&f, true) else {
                continue;
            };
            let w = c
                .i(&format!("menu1Button{i}SizeX"))
                .or(c.i("menu1Button1SizeX"))
                .unwrap_or(image.w as i32);
            let h = c
                .i(&format!("menu1Button{i}SizeY"))
                .or(c.i("menu1Button1SizeY"))
                .unwrap_or(image.h as i32);
            let x = c.i_or(&format!("menu1Button{i}PrintX"), 0);
            let y = c.i_or(&format!("menu1Button{i}PrintY"), 0);
            let lit = (
                c.i(&format!("menu1Button{i}PicSrcX2")).unwrap_or(0),
                c.i(&format!("menu1Button{i}PicSrcY2")).unwrap_or(h),
            );
            self.buttons.push(Button {
                image,
                rect: [x, y, w, h],
                lit,
            });
        }
        self.title_start = self.now;
        self.title_wait = (c.i_or("menuStartWaitTime", 0).max(0) as u64 * self.frame_ms).min(8000);
        if let Some(n) = c.i("bgmNumber") {
            self.play_bgm(n);
        }
        match c.s("backScriptName").map(str::to_string) {
            Some(back) if c.i_or("backScriptFlag", 0) != 0 && self.load_script(&back).is_ok() => {
                self.vars.insert(MODE_VAR, TITLE);
                self.enter(&back);
            }
            _ => {
                if let Some(f) = c
                    .s("filenameBG")
                    .or(c.s("filename"))
                    .or(c.s("bgFileName"))
                    .map(str::to_string)
                    && let Some(image) = self.picture(&f, true)
                {
                    let (w, h) = (image.w as i32, image.h as i32);
                    self.layers.insert(
                        0,
                        Layer {
                            pic: f,
                            image,
                            a: [0, 0, w, h],
                            b: [0, 0, w, h],
                            effect: Some(Effect {
                                kind: 0,
                                p: Vec::new(),
                                start: 0,
                            }),
                        },
                    );
                }
                self.wait = Wait::Idle;
            }
        }
    }

    fn start_game(&mut self) {
        self.mode = Mode::Game;
        self.reset_screen();
        self.stop_all(300);
        self.calls.clear();
        self.vars.clear();
        let first = self.first_script.clone();
        self.enter(&first);
    }

    fn reset_screen(&mut self) {
        self.layers.clear();
        self.kept = None;
        self.fade = None;
        self.msg = None;
        self.in_command = false;
        self.wait = Wait::None;
    }

    fn back_to_title(&mut self) {
        self.stop_all(300);
        self.start_title();
    }

    /// Starts a script from its beginning.
    fn enter(&mut self, script: &str) {
        let Ok(s) = self.load_script(script) else {
            self.trace(&format!("no script {script}"));
            self.script_ended();
            return;
        };
        self.run = Frame {
            script: s.name.clone(),
            pc: 0,
            stack: Vec::new(),
        };
        self.wait = Wait::None;
    }

    /// The running script came to its end (or returned from its top).
    fn script_ended(&mut self) {
        if let Some(f) = self.calls.pop() {
            self.run = f;
            return;
        }
        match self.mode {
            Mode::Opening => self.start_title(),
            Mode::Title => self.wait = Wait::Idle,
            Mode::Game => self.back_to_title(),
        }
    }

    // ---- the frame ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.frames += 1;
        self.now = (self.frames as f64 * FRAME_MS) as u64;
        if self.menu.is_some() {
            return Ok(());
        }
        if let Some(f) = &self.fade
            && self.now >= f.start + f.ms
        {
            self.fade = None;
        }
        if let Wait::Until(t) = self.wait
            && self.now >= t
        {
            self.wait = Wait::None;
        }
        if self.wait == Wait::None {
            self.run()?;
        }
        Ok(())
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..STEPS {
            if self.wait != Wait::None || self.ended {
                return Ok(());
            }
            let s = self.load_script(&self.run.script.clone())?;
            let pc = self.run.pc;
            let len = s.word(pc) as usize;
            if pc >= s.w.len() || len == 0 || pc + len > s.w.len() {
                self.trace("end of script");
                self.script_ended();
                continue;
            }
            let (typ, code) = (s.word(pc + 1), s.word(pc + 2));
            let arg = |i: usize| s.word(pc + 3 + i) as i32;
            match typ {
                0x6666_0001 => {
                    if code == 0x5555_0002 {
                        self.script_ended();
                    } else {
                        self.run.pc += len;
                    }
                }
                0x6666_0002 => {
                    if self.control(&s, pc, len, code)? {
                        return Ok(());
                    }
                }
                0x6666_0003 => {
                    let vals = self.eval_list(&s, pc + 3);
                    self.run.pc += len;
                    self.select(&s, code, &vals);
                }
                0x6666_0004 => {
                    let vals = self.eval_list(&s, pc + 3);
                    self.trace(&format!("function {code} {vals:?}"));
                    self.run.pc += len;
                    self.function(code, &vals);
                }
                0x6666_0005 => {
                    self.eval(&s, pc + 3);
                    self.run.pc += len;
                }
                0x6666_0006 => {
                    self.begin_command();
                    self.run.pc += len;
                    match code {
                        0x2222_0001..=0x2222_0003 => {
                            self.message(&s, arg(0) as u32);
                            self.msg_pc = pc;
                        }
                        0x2222_0004 => {
                            let n = arg(1).max(0) as u64;
                            if n > 0 {
                                self.wait = Wait::Until(self.now + n * self.frame_ms);
                            }
                        }
                        0x2222_0005 => {
                            let n = arg(2).max(0) as u64;
                            let ms = n * self.frame_ms;
                            if let Some(from) = self.kept.take()
                                && ms > 0
                            {
                                self.fade = Some(Fade {
                                    from,
                                    start: self.now,
                                    ms,
                                });
                            }
                            if ms > 0 {
                                self.wait = Wait::Until(self.now + ms);
                            }
                        }
                        _ => self.trace(&format!("command {code:x}")),
                    }
                }
                0x6666_0007 => {
                    self.begin_command();
                    let a: Vec<i32> = s.w[pc + 3..pc + len].iter().map(|&v| v as i32).collect();
                    self.run.pc += len;
                    self.setup(&s, code, &a);
                }
                _ => {
                    self.trace(&format!("record {typ:x} {code:x}"));
                    self.run.pc += len;
                }
            }
        }
        Err(format!(
            "System-NNN: {} ran {STEPS} records without waiting (at {})",
            self.run.script, self.run.pc
        ))
    }

    fn begin_command(&mut self) {
        if !self.in_command {
            self.in_command = true;
            self.msg = None;
        }
    }

    fn end_command(&mut self) {
        self.in_command = false;
        self.kept = None;
    }

    /// A control record; true when the frame ends here.
    fn control(&mut self, s: &Script, pc: usize, len: usize, code: u32) -> Result<bool, String> {
        let arg = |i: usize| s.word(pc + 3 + i);
        // skips the record at the pc, or the block when it is a `{`
        let skip_body = |p: usize| -> usize {
            if s.word(p + 1) == 0x6666_0002 && s.word(p + 2) == 0xa000_007b {
                p + s.word(p + 3).max(1) as usize
            } else {
                p + s.word(p).max(1) as usize
            }
        };
        match code {
            0x5556_0002 => {
                let mut p = pc;
                loop {
                    let (v, _) = self.eval(s, p + 3);
                    p += s.word(p).max(1) as usize;
                    if v > 0 {
                        break;
                    }
                    p = skip_body(p);
                    match (s.word(p + 1), s.word(p + 2)) {
                        (0x6666_0002, 0x5556_0004) => {
                            p += s.word(p).max(1) as usize;
                            break;
                        }
                        (0x6666_0002, 0x5556_0003) => continue,
                        _ => break,
                    }
                }
                self.run.pc = p;
            }
            0x5556_0011 => {
                let (v, _) = self.eval(s, pc + 3);
                self.run.pc = if v > 0 { pc + len } else { skip_body(pc + len) };
            }
            0x5556_0003 | 0x5556_0004 | 0x5556_0008 => {
                self.run.pc = skip_body(pc + len);
            }
            0x5556_0007 => match self.run.stack.pop() {
                Some(p) => self.run.pc = p,
                None => {
                    self.trace("return at the top");
                    self.script_ended();
                }
            },
            0x5556_0009 => {
                self.trace(&format!("call film {}", arg(0)));
                self.run.stack.push(pc + len);
                if self.run.stack.len() > 250 {
                    return Err("System-NNN: calls too deep".into());
                }
                match s.film(arg(0)) {
                    Some(t) => self.run.pc = t,
                    None => return Err(format!("System-NNN: no film {}", arg(0))),
                }
            }
            0x5556_0020 => self.run.pc = arg(0) as usize,
            0x5556_0005 if self.mode == Mode::Game => {
                // the top script starts over (sys.spt jumps to the story again)
                self.trace("start over");
                self.calls.clear();
                self.run.pc = 0;
                self.run.stack.clear();
                self.end_command();
                return Ok(true);
            }
            0x5556_0005 | 0x5556_0006 => {
                self.trace(&format!("end {code:x}"));
                self.run.pc = pc + len;
                self.script_ended();
                return Ok(true);
            }
            0x5556_000b => {
                self.run.pc = pc + len;
                self.trace("to the title");
                self.calls.clear();
                match self.mode {
                    Mode::Game => self.back_to_title(),
                    _ => self.script_ended(),
                }
                return Ok(true);
            }
            0x5556_000c | 0x5556_000e | 0x5556_0013 => {
                self.run.pc = pc + len;
                self.end_command();
                return Ok(true);
            }
            0x5556_000a | 0x5556_0015 | 0x5556_0016 => {
                // a script runs from its beginning (the header record is passed over)
                let name = s.string(arg(0));
                self.run.pc = pc + len;
                self.trace(&format!("script {name}"));
                if self.load_script(&name).is_err() {
                    self.trace(&format!("no script {name}"));
                    return Ok(false);
                }
                if code != 0x5556_0015 {
                    self.calls.push(self.run.clone());
                }
                let calls = std::mem::take(&mut self.calls);
                self.enter(&name);
                self.calls = calls;
                return Ok(true);
            }
            _ => self.run.pc = pc + len,
        }
        Ok(false)
    }

    // ---- expressions ----

    fn var(&self, i: u32) -> i32 {
        self.vars.get(&i).copied().unwrap_or(0)
    }

    /// The value of a token: a variable, a small or big constant.
    fn token(&self, s: &Script, t: u32, big: &[i32]) -> i32 {
        match t & 0xffff_0000 {
            0x8000_0000 => {
                if t & 0x8000 != 0 {
                    (t & 0x7fff) as i32
                } else {
                    self.var(t & 0xffff)
                }
            }
            0x9000_0000 => (t & 0xffff) as u16 as i16 as i32,
            0x9100_0000 => s.big(t & 0xffff),
            0x9200_0000 => big.get((t & 0xff) as usize).copied().unwrap_or(0),
            _ => 0,
        }
    }

    /// Evaluates the expression at `at`: its value and how many words it took.
    fn eval(&mut self, s: &Script, at: usize) -> (i32, usize) {
        let first = s.word(at);
        if first != 0xa000_0000 {
            let v = match first & 0xf000_0000 {
                0x8000_0000 => self.var(first & 0xffff),
                0x9000_0000 if first & 0x0f00_0000 == 0 => (first & 0xffff) as u16 as i16 as i32,
                0x9000_0000 if first & 0x0f00_0000 == 0x0100_0000 => s.big(first & 0xffff),
                0xc000_0000 | 0xd000_0000 => first as i32,
                _ => 0,
            };
            return (v, 1);
        }
        let n = s.word(at + 1) as usize;
        let mut st: Vec<u32> = Vec::new();
        let mut big: Vec<i32> = vec![0; 256];
        let mut nbig = 0usize;
        let push_val = |st: &mut Vec<u32>, big: &mut Vec<i32>, nbig: &mut usize, v: i32| {
            if (-0x7fff..=0x7fff).contains(&v) {
                st.push(0x9000_0000 | (v as u32 & 0xffff));
            } else {
                let k = *nbig % 256;
                big[k] = v;
                *nbig += 1;
                st.push(0x9200_0000 | k as u32);
            }
        };
        for i in 0..n {
            let t = s.word(at + 2 + i);
            let hi = t & 0xf000_0000;
            if hi == 0x8000_0000 || hi == 0x9000_0000 {
                st.push(t);
                continue;
            }
            if hi != 0xa000_0000 {
                break;
            }
            let fam = t & 0xffff_0000;
            let op = (t & 0xffff) as u8;
            let is_var = |x: u32| (x & 0xffff_0000) == 0x8000_0000;
            if fam == 0xa000_0000 && op == b'!' {
                let Some(x) = st.pop() else { break };
                let v = (self.token(s, x, &big) == 0) as i32;
                if is_var(x) {
                    self.vars.insert(x & 0xffff, v);
                    st.push(x);
                } else {
                    push_val(&mut st, &mut big, &mut nbig, v);
                }
                continue;
            }
            if st.len() < 2 {
                break;
            }
            let y = st.pop().unwrap();
            let x = st.pop().unwrap();
            let (a, b) = (self.token(s, x, &big), self.token(s, y, &big));
            let arith = |op: u8, a: i32, b: i32| -> Option<i32> {
                Some(match op {
                    b'%' => a.wrapping_rem(if b == 0 { 1 } else { b }),
                    b'&' => a & b,
                    b'*' => a.wrapping_mul(b),
                    b'+' => a.wrapping_add(b),
                    b'-' => a.wrapping_sub(b),
                    b'/' => a.wrapping_div(if b == 0 { 1 } else { b }),
                    b'^' => a ^ b,
                    b'|' => a | b,
                    _ => return None,
                })
            };
            match fam {
                0xa000_0000 if op == b'=' => {
                    if is_var(x) {
                        self.vars.insert(x & 0xffff, b);
                    }
                    st.push(x);
                }
                0xa000_0000 => {
                    let v = arith(op, a, b).unwrap_or(0);
                    push_val(&mut st, &mut big, &mut nbig, v);
                }
                0xa200_0000 if op == b'=' => {
                    push_val(&mut st, &mut big, &mut nbig, (a == b) as i32);
                }
                0xa200_0000 => {
                    let v = if op == b'!' {
                        (b == 0) as i32
                    } else {
                        arith(op, a, b).unwrap_or(a)
                    };
                    if is_var(x) {
                        self.vars.insert(x & 0xffff, v);
                    }
                    st.push(x);
                }
                0xa300_0000 => {
                    let v = match op {
                        b'&' => a != 0 && b != 0,
                        b'=' => a == b,
                        b'^' => (a != 0) != (b != 0),
                        _ => a != 0 || b != 0,
                    };
                    push_val(&mut st, &mut big, &mut nbig, v as i32);
                }
                0xa400_0000 => {
                    let v = if op == b'<' { a < b } else { a > b };
                    push_val(&mut st, &mut big, &mut nbig, v as i32);
                }
                _ => {
                    let v = match op {
                        b'!' => a != b,
                        b'<' => a <= b,
                        b'>' => a >= b,
                        _ => a == b,
                    };
                    push_val(&mut st, &mut big, &mut nbig, v as i32);
                }
            }
        }
        let v = st.last().map_or(0, |&x| self.token(s, x, &big));
        (v, n + 2)
    }

    // ---- commands ----

    fn setup(&mut self, s: &Script, code: u32, a: &[i32]) {
        let at = |i: usize| a.get(i).copied().unwrap_or(0);
        match code {
            0x3333_0015 | 0x3333_001f => {
                let layer = at(0);
                let name = s.string(at(1) as u32);
                self.load_layer(layer, &name);
            }
            0x3333_0016 => {
                let layer = at(1);
                if let Some(l) = self.layers.get_mut(&layer) {
                    l.a = [at(2), at(3), at(4), at(5)];
                    l.b = [at(6), at(7), at(8), at(9)];
                }
            }
            0x3333_0013 => {
                let layer = at(1);
                let kind = at(2);
                let p: Vec<i32> = a.get(3..).unwrap_or(&[]).to_vec();
                self.set_effect(layer, kind, p);
            }
            0x3333_0014 => {
                if let Some(l) = self.layers.get_mut(&at(1)) {
                    l.effect = None;
                }
            }
            0x3333_0012 => {
                for l in self.layers.values_mut() {
                    l.effect = None;
                }
            }
            0x3333_0011 => {
                self.kept = Some(self.scene());
            }
            0x3333_0007 => {
                let n = at(1);
                if n > 0 {
                    self.play_bgm(n);
                } else {
                    let fade = (at(2).max(0) as u64 * self.frame_ms) as u32;
                    self.stop(BGM, fade);
                    self.bgm = None;
                }
            }
            0x3333_0008 => {
                let v = at(1);
                self.voice = (v >= 0).then(|| s.string(v as u32));
            }
            0x3333_0009 => {
                let (n, ch) = (at(1), at(2).max(0));
                if n < 0 {
                    self.stop(SE + ch, 0);
                } else if let Some(name) = self.se_list.get(n as usize).cloned()
                    && !name.is_empty()
                    && name != "?"
                {
                    let data = self
                        .file(&format!("vaw/se/{name}.vaw"))
                        .or_else(|| self.file(&format!("vaw/{name}.vaw")))
                        .or_else(|| self.file(&format!("wgq/{name}.wgq")));
                    if let Some(d) = data.and_then(|d| crate::vaw::playable(&d)) {
                        self.sounds.push(Sound {
                            data: d,
                            volume: 1.0,
                            channel: SE + ch,
                            stop: false,
                            fade_ms: 0,
                            looped: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn load_layer(&mut self, layer: i32, name: &str) {
        let Some(image) = self.picture(name, false) else {
            self.trace(&format!("no picture {name}"));
            self.layers.remove(&layer);
            return;
        };
        let (w, h) = (image.w as i32, image.h as i32);
        let effect = self.layers.get(&layer).and_then(|l| l.effect.clone());
        self.layers.insert(
            layer,
            Layer {
                pic: name.to_string(),
                image,
                a: [0, 0, w, h],
                b: [0, 0, w, h],
                effect,
            },
        );
    }

    fn set_effect(&mut self, layer: i32, kind: i32, p: Vec<i32>) {
        let now = self.now.max(1);
        if kind == -1 {
            if let Some(l) = self.layers.get_mut(&layer) {
                l.effect = None;
            }
            return;
        }
        if kind == 54 && !self.layers.contains_key(&layer) {
            // a fill needs no picture
            let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
            self.layers.insert(
                layer,
                Layer {
                    pic: String::new(),
                    image: solid(1, 1, [0, 0, 0, 255]),
                    a: [0, 0, w, h],
                    b: [0, 0, w, h],
                    effect: None,
                },
            );
        }
        let Some(l) = self.layers.get_mut(&layer) else {
            return;
        };
        if kind == 1 {
            // Mae: as it was
            match &mut l.effect {
                Some(e) => e.start = 0,
                None => {
                    l.effect = Some(Effect {
                        kind: 0,
                        p: Vec::new(),
                        start: 0,
                    })
                }
            }
            return;
        }
        l.effect = Some(Effect {
            kind,
            p,
            start: now,
        });
    }

    /// The game's own functions (66660004). Before Dawn Daybreak's requests: 4 counts the
    /// requests left (variable 338), 3 marks the chosen one (339) done.
    /// A count and that many expressions.
    fn eval_list(&mut self, s: &Script, at: usize) -> Vec<i32> {
        let n = (s.word(at) as usize).min(64);
        let mut at = at + 1;
        let mut vals = Vec::new();
        for _ in 0..n {
            let (v, used) = self.eval(s, at);
            vals.push(v);
            at += used;
        }
        vals
    }

    fn function(&mut self, code: u32, a: &[i32]) {
        if !self.chart.is_empty() {
            // EXTRAVAGANZA's chart: 7 n opens slot n, 3 n k opens it (k 1) or closes it (k 0),
            // 2 n k marks it played
            let n = a.first().copied().unwrap_or(0);
            match code {
                7 => {
                    self.vars.insert(SLOT_OPEN + n as u32, 1);
                }
                3 => {
                    let k = a.get(1).copied().unwrap_or(1);
                    self.vars.insert(SLOT_OPEN + n as u32, (k != 0) as i32);
                }
                2 => {
                    self.vars.insert(SLOT_DONE + n as u32, 1);
                }
                _ => {}
            }
            return;
        }
        if self.requests.is_empty() {
            return;
        }
        match code {
            4 => {
                let left = self
                    .requests
                    .iter()
                    .filter(|r| self.var(REQUEST_DONE + r.0 as u32) == 0)
                    .count();
                self.vars.insert(338, left as i32);
            }
            3 => {
                let n = self.var(339);
                if n > 0 {
                    self.vars.insert(REQUEST_DONE + n as u32, 1);
                }
            }
            _ => {}
        }
    }

    fn select(&mut self, s: &Script, code: u32, vals: &[i32]) {
        let (w, _) = (self.screen.0 as i32, self.screen.1 as i32);
        let mut labels = Vec::new();
        let mut values = Vec::new();
        let mut target = None;
        if vals.is_empty() && !self.chart.is_empty() {
            // the chart: slots opened or reached from a played one, not played yet
            let mut reach: Vec<i32> = Vec::new();
            for (id, links, _) in &self.chart {
                if self.var(SLOT_DONE + *id as u32) != 0 {
                    reach.extend(links);
                }
            }
            for (id, _, text) in self.chart.clone() {
                // reached slots are open unless a function closed them
                let open = match self.vars.get(&(SLOT_OPEN + id as u32)) {
                    Some(&v) => v != 0,
                    None => reach.contains(&id),
                };
                if open && self.var(SLOT_DONE + id as u32) == 0 {
                    labels.push(text);
                    values.push(id);
                }
            }
            target = Some(self.slot_var);
        } else if vals.is_empty() && code == 1 && !self.requests.is_empty() {
            // the request board: one choice for each request left
            for (n, desc) in self.requests.clone() {
                if self.var(REQUEST_DONE + n as u32) == 0 {
                    labels.push(format!("依頼{n}  {desc}"));
                    values.push(n);
                }
            }
            target = Some(339);
        } else if vals.len() >= 2 {
            for (i, &v) in vals.iter().enumerate() {
                let t = s.message(v.max(0) as u32);
                labels.push(t.lines().next().unwrap_or("").trim().to_string());
                values.push(i as i32);
            }
        }
        if labels.is_empty() {
            self.trace(&format!("select {code} {vals:?}: no choices"));
            return;
        }
        let step = (400 / labels.len() as i32).clamp(40, 56);
        let mut opts = Vec::new();
        for (i, t) in labels.into_iter().enumerate() {
            let y = 100 + i as i32 * step;
            opts.push((t, [w / 2 - 330, y, w / 2 + 330, y + step - 8]));
        }
        self.choice = target.map(|t| (t, values));
        self.wait = Wait::Select(opts);
    }

    fn message(&mut self, s: &Script, idx: u32) {
        let t = plain(&s.message(idx).replace('\r', ""));
        let mut lines: Vec<String> = t.lines().map(|l| l.to_string()).collect();
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let mut name = String::new();
        if lines.len() >= 2 {
            let first = lines[0].trim();
            let second = lines[1].trim_start();
            if !first.is_empty()
                && first.chars().count() <= 12
                && !first.contains('「')
                && second.starts_with(['「', '『', '（', '('])
            {
                name = first.to_string();
                lines.remove(0);
            }
        }
        self.msg = Some(Msg { name, lines });
        if let Some(v) = self.voice.take() {
            self.play_voice(&v);
        }
        self.wait = Wait::Click;
    }

    // ---- sounds ----

    fn play_bgm(&mut self, n: i32) {
        if self.bgm == Some(n) {
            return;
        }
        let Some(name) = self.bgm_list.get((n - 1).max(0) as usize).cloned() else {
            return;
        };
        if name.is_empty() || name == "?" {
            return;
        }
        let data = self
            .file(&format!("wgq/{name}.wgq"))
            .or_else(|| self.file(&format!("vaw/{name}.vaw")));
        let Some(d) = data.and_then(|d| crate::vaw::playable(&d)) else {
            self.trace(&format!("no bgm {name}"));
            return;
        };
        self.bgm = Some(n);
        self.sounds.push(Sound {
            data: d,
            volume: 0.8,
            channel: BGM,
            stop: false,
            fade_ms: 0,
            looped: true,
        });
    }

    fn play_voice(&mut self, name: &str) {
        let n = name.to_lowercase();
        let prefix: String = n.chars().take(2).collect();
        let data = self
            .pack_entry(&format!("cdvaw/{prefix}.vpk"), true, &n)
            .or_else(|| self.file(&format!("vaw/{n}.vaw")))
            .or_else(|| self.file(&format!("cdvaw/{n}.vaw")));
        if let Some(d) = data.and_then(|d| crate::vaw::playable(&d)) {
            self.sounds.push(Sound {
                data: d,
                volume: 1.0,
                channel: VOICE,
                stop: false,
                fade_ms: 0,
                looped: false,
            });
        } else {
            self.trace(&format!("no voice {name}"));
        }
    }

    fn stop(&mut self, ch: i32, fade_ms: u32) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel: ch,
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    fn stop_all(&mut self, fade_ms: u32) {
        self.bgm = None;
        self.stop(-1, fade_ms);
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    // ---- drawing ----

    fn scene(&self) -> Vec<Draw> {
        let mut out = Vec::new();
        let (sw, sh) = (self.screen.0 as f32, self.screen.1 as f32);
        for l in self.layers.values() {
            let Some(e) = &l.effect else { continue };
            let p = |i: usize, d: i32| e.p.get(i).copied().unwrap_or(d);
            let frames = |d: i32| (p(0, d).max(1) as u64 * self.frame_ms) as f32;
            let k = |d: i32| {
                if e.start == 0 {
                    1.0
                } else {
                    ((self.now.saturating_sub(e.start)) as f32 / frames(d)).clamp(0.0, 1.0)
                }
            };
            let mut r = l.b;
            let mut alpha = 1.0;
            let mut over: Option<([u8; 3], f32)> = None;
            match e.kind {
                54 => {
                    let c = [p(1, 0), p(2, 0), p(3, 0)].map(|v| v.clamp(0, 255) as u8);
                    let mut d = draw(&solid(1, 1, [c[0], c[1], c[2], 255]), 0.0, 0.0, 1.0);
                    d.dst = [0.0, 0.0, sw, sh];
                    out.push(d);
                    continue;
                }
                52 | 61 | 62 => {
                    let t = k(20);
                    for ((v, a), b) in r.iter_mut().zip(l.a).zip(l.b) {
                        *v = a + ((b - a) as f32 * t).round() as i32;
                    }
                }
                9 => {
                    let t = k(20);
                    let (from, to) = (p(1, 0) as f32, p(2, 100) as f32);
                    alpha = ((from + (to - from) * t) / 100.0).clamp(0.0, 1.0);
                }
                10 | 11 => {
                    let t = k(20);
                    let t = if e.kind == 10 { t } else { 1.0 - t };
                    let c = [p(1, 0), p(2, 0), p(3, 0)].map(|v| v.clamp(0, 255) as u8);
                    let full = r[2] >= self.screen.0 as i32 && r[3] >= self.screen.1 as i32;
                    if full && c != [0, 0, 0] {
                        over = Some((c, 1.0 - t));
                    } else if full {
                        over = Some(([0, 0, 0], 1.0 - t));
                    } else {
                        alpha = t;
                    }
                }
                _ => {}
            }
            if alpha <= 0.0 {
                continue;
            }
            let mut d = draw(&l.image, r[0] as f32, r[1] as f32, alpha);
            if r[2] > 0 && r[3] > 0 {
                d.dst = [
                    r[0] as f32,
                    r[1] as f32,
                    (r[0] + r[2]) as f32,
                    (r[1] + r[3]) as f32,
                ];
            }
            out.push(d);
            if let Some((c, a)) = over
                && a > 0.0
            {
                let mut d = draw(&solid(1, 1, [c[0], c[1], c[2], 255]), 0.0, 0.0, a);
                d.dst = [
                    r[0] as f32,
                    r[1] as f32,
                    (r[0] + r[2]) as f32,
                    (r[1] + r[3]) as f32,
                ];
                out.push(d);
            }
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(m) = self.msg.clone() else { return };
        let mw = self.config("messageWindow");
        let pm = self.config("printmessage");
        if self.window.is_none()
            && let Some(f) = mw.s("filenameWindow").map(str::to_string)
        {
            self.window = self.picture(&f, true);
        }
        let (sw, sh) = (self.screen.0 as i32, self.screen.1 as i32);
        match self.window.clone() {
            Some(w) => out.push(draw(
                &w,
                mw.i_or("printX", 0) as f32,
                mw.i_or("printY", sh - w.h as i32) as f32,
                1.0,
            )),
            None => {
                let mut d = draw(&solid(1, 1, [0, 0, 0, 160]), 0.0, 0.0, 1.0);
                d.dst = [0.0, (sh - 170) as f32, sw as f32, sh as f32];
                out.push(d);
            }
        }
        let size = pm.i_or("fontSize", 24).clamp(10, 64);
        let rgb = [
            pm.i_or("colorR", 255),
            pm.i_or("colorG", 255),
            pm.i_or("colorB", 255),
        ]
        .map(|v| v.clamp(0, 255) as u8);
        if !m.name.is_empty() {
            let nx = mw.i_or("namePrintX", 50);
            let ny = mw.i_or("namePrintY", sh - 210);
            let mut nw = mw.i_or("nameSizeX", 240);
            if mw.i_or("nameWindowFlag", 0) != 0 {
                if self.name_window.is_none()
                    && let Some(f) = mw.s("filenameNameWindow").map(str::to_string)
                {
                    self.name_window = self.picture(&f, true);
                }
                if let Some(w) = self.name_window.clone() {
                    nw = w.w as i32;
                    out.push(draw(&w, nx as f32, ny as f32, 1.0));
                }
            }
            let nsize = pm.i_or("nameFontSize", size).clamp(10, 64);
            let t = self.text(&m.name, rgb, nsize);
            let x = if pm.i_or("nameCenterFlag", 0) != 0 {
                nx + (nw - nsize * m.name.chars().count() as i32) / 2
            } else {
                pm.i_or("nameX", nx + 10)
            };
            out.push(draw(
                &t,
                (x - 2) as f32,
                (pm.i_or("nameY", ny + 8) - 2) as f32,
                1.0,
            ));
        }
        let (x, y) = (pm.i_or("printX", 40), pm.i_or("printY", sh - 150));
        let next = pm.i_or("nextY", size + 6).max(size);
        for (i, l) in m.lines.iter().enumerate() {
            let t = self.text(l, rgb, size);
            out.push(draw(
                &t,
                (x - 2) as f32,
                (y + i as i32 * next - 2) as f32,
                1.0,
            ));
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        if let Some(m) = self.menu {
            return self.menu_draws(m);
        }
        let mut out = self.scene();
        if let Some(f) = &self.fade {
            let k = ((self.now.saturating_sub(f.start)) as f32 / f.ms.max(1) as f32).min(1.0);
            let mut from = f.from.clone();
            for d in &mut from {
                d.place.alpha *= 1.0 - k;
            }
            out.extend(from);
        }
        self.window_draws(&mut out);
        if self.mode == Mode::Title && self.title_ready() {
            for b in &self.buttons {
                let [x, y, w, h] = b.rect;
                let lit = inside([x, y, x + w, y + h], self.pointer.0, self.pointer.1);
                let src = if lit {
                    [b.lit.0, b.lit.1, w, h]
                } else {
                    [0, 0, w, h]
                };
                out.push(part(&b.image, src, x as f32, y as f32));
            }
            if self.buttons.is_empty() {
                for (label, r) in self.own_title_items() {
                    self.button(label, r, &mut out);
                }
            }
        }
        if let Wait::Select(opts) = self.wait.clone() {
            for (t, r) in opts {
                self.button(&t, r, &mut out);
            }
        }
        out
    }

    fn title_ready(&self) -> bool {
        self.now >= self.title_start + self.title_wait || self.wait == Wait::Idle
    }

    fn own_title_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        ["はじめから", "つづきから"]
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let y = h / 2 + 40 + i as i32 * 60;
                (n, [w / 2 - 120, y, w / 2 + 120, y + 48])
            })
            .collect()
    }

    // ---- text ----

    fn text(&mut self, s: &str, rgb: [u8; 3], size: i32) -> Rc<Image> {
        let key = (s.to_string(), rgb, size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => {
                Rc::new(Font::outline(f.clone(), size).render(&nexas::sjis::encode(s), rgb, 2))
            }
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 300 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.pointer = (x, y);
        }
        if let Some(m) = self.menu {
            self.menu_click(m, x, y);
            return;
        }
        self.fade = None;
        if self.mode == Mode::Opening {
            self.stop_all(200);
            self.start_title();
            return;
        }
        if self.mode == Mode::Title {
            if !self.title_ready() {
                self.title_wait = 0;
                return;
            }
            let hit = if self.buttons.is_empty() {
                self.own_title_items()
                    .iter()
                    .position(|(_, r)| inside(*r, x, y))
            } else {
                self.buttons.iter().position(|b| {
                    let [bx, by, w, h] = b.rect;
                    inside([bx, by, bx + w, by + h], x, y)
                })
            };
            match hit {
                Some(0) => self.start_game(),
                Some(1) => self.menu = Some(Menu::Load),
                _ => {}
            }
            return;
        }
        match self.wait.clone() {
            Wait::Click => self.wait = Wait::None,
            Wait::Until(_) => {
                for l in self.layers.values_mut() {
                    if let Some(e) = &mut l.effect {
                        e.start = 0;
                    }
                }
                self.wait = Wait::None;
            }
            Wait::Select(opts) if x >= 0 => {
                if let Some(i) = opts.iter().position(|(_, r)| inside(*r, x, y)) {
                    self.trace(&format!("chose {i}"));
                    if let Some((var, values)) = self.choice.take() {
                        self.vars.insert(var, values.get(i).copied().unwrap_or(0));
                    }
                    self.wait = Wait::None;
                }
            }
            _ => {}
        }
    }

    /// A right click: our menu (save, load, title).
    pub fn cancel(&mut self) {
        self.menu = match self.menu {
            None if self.mode == Mode::Game
                && matches!(self.wait, Wait::Click | Wait::Select(_)) =>
            {
                Some(Menu::Main)
            }
            None => None,
            Some(Menu::Main) => None,
            Some(_) if self.mode == Mode::Title => None,
            Some(_) => Some(Menu::Main),
        };
    }

    pub fn wheel(&mut self, _up: bool) {}

    pub fn ended(&self) -> bool {
        self.ended
    }

    // ---- our menu and saves ----

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let names = ["セーブ", "ロード", "タイトルへ", "戻る"];
        let top = h / 2 - 2 * 60;
        names
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let y = top + i as i32 * 60;
                (n, [w / 2 - 150, y, w / 2 + 150, y + 48])
            })
            .collect()
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let rh = (h - 80) / SLOTS as i32;
        (0..SLOTS as i32)
            .map(|i| [60, 40 + i * rh, w - 60, 40 + i * rh + rh - 6])
            .collect()
    }

    fn button(&mut self, label: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let lit = inside(r, self.pointer.0, self.pointer.1);
        let mut d = draw(
            &solid(
                1,
                1,
                if lit {
                    [70, 90, 140, 230]
                } else {
                    [20, 24, 40, 220]
                },
            ),
            0.0,
            0.0,
            1.0,
        );
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let t = self.text(label, [255, 255, 255], 22);
        let mut d = draw(
            &t,
            (r[0] + 14) as f32,
            (r[1] + (r[3] - r[1] - 26) / 2) as f32,
            1.0,
        );
        d.place.clip = [r[0] as f32, r[1] as f32, (r[2] - 6) as f32, r[3] as f32];
        out.push(d);
    }

    fn menu_draws(&mut self, m: Menu) -> Vec<Draw> {
        let mut out = self.scene();
        let mut d = draw(&solid(1, 1, [0, 0, 0, 150]), 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
        out.push(d);
        match m {
            Menu::Main => {
                for (label, r) in self.menu_items() {
                    self.button(label, r, &mut out);
                }
            }
            Menu::Save | Menu::Load => {
                for (i, r) in self.slot_rects().into_iter().enumerate() {
                    let label = match self.read_slot(i) {
                        Some(s) => format!("{:02}  {}", i + 1, slot_title(&s)),
                        None => format!("{:02}  ----", i + 1),
                    };
                    self.button(&label, r, &mut out);
                }
            }
        }
        out
    }

    fn menu_click(&mut self, m: Menu, x: i32, y: i32) {
        match m {
            Menu::Main => {
                let hit = self
                    .menu_items()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                match hit {
                    Some(0) => self.menu = Some(Menu::Save),
                    Some(1) => self.menu = Some(Menu::Load),
                    Some(2) => {
                        self.menu = None;
                        self.back_to_title();
                    }
                    Some(_) => self.menu = None,
                    None => {}
                }
            }
            Menu::Save | Menu::Load => {
                if let Some(i) = self.slot_rects().into_iter().position(|r| inside(r, x, y)) {
                    if m == Menu::Save {
                        self.save(i);
                        self.menu = None;
                    } else if self.read_slot(i).is_some() {
                        self.menu = None;
                        self.load(i);
                    }
                }
            }
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<String> {
        let b = self.files.read_save(&format!("nnn{:02}.sav", i + 1))?;
        Some(String::from_utf8_lossy(&b).to_string())
    }

    fn save(&mut self, i: usize) {
        let mut t = String::new();
        let title = self
            .msg
            .as_ref()
            .map(|m| {
                let s: String = m.lines.concat();
                let s = s.trim_start_matches('\u{3000}');
                s.chars().take(30).collect::<String>()
            })
            .unwrap_or_default();
        t.push_str(&format!("title\t{}\n", esc(&title)));
        for f in &self.calls {
            t.push_str(&format!(
                "call\t{}\t{}\t{}\n",
                f.script,
                f.pc,
                join(&f.stack)
            ));
        }
        t.push_str(&format!(
            "at\t{}\t{}\t{}\n",
            self.run.script,
            self.msg_pc,
            join(&self.run.stack)
        ));
        let mut vars: Vec<_> = self.vars.iter().collect();
        vars.sort();
        for (k, v) in vars {
            t.push_str(&format!("var\t{k}\t{v}\n"));
        }
        for (id, l) in &self.layers {
            let (kind, p) = l
                .effect
                .as_ref()
                .map_or((-1, Vec::new()), |e| (e.kind, e.p.clone()));
            t.push_str(&format!(
                "layer\t{id}\t{}\t{}\t{}\t{kind}\t{}\n",
                esc(&l.pic),
                join(&l.a),
                join(&l.b),
                join(&p)
            ));
        }
        if let Some(n) = self.bgm {
            t.push_str(&format!("bgm\t{n}\n"));
        }
        self.files
            .write_save(&format!("nnn{:02}.sav", i + 1), t.as_bytes());
    }

    fn load(&mut self, i: usize) {
        let Some(t) = self.read_slot(i) else { return };
        self.stop_all(200);
        self.reset_screen();
        self.mode = Mode::Game;
        self.calls.clear();
        self.vars.clear();
        let mut at = None;
        for l in t.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            match f.first().copied() {
                Some("call") if f.len() >= 3 => self.calls.push(Frame {
                    script: f[1].to_string(),
                    pc: f[2].parse().unwrap_or(0),
                    stack: split(f.get(3).copied().unwrap_or("")),
                }),
                Some("at") if f.len() >= 3 => {
                    at = Some(Frame {
                        script: f[1].to_string(),
                        pc: f[2].parse().unwrap_or(0),
                        stack: split(f.get(3).copied().unwrap_or("")),
                    })
                }
                Some("var") if f.len() >= 3 => {
                    if let (Ok(k), Ok(v)) = (f[1].parse(), f[2].parse()) {
                        self.vars.insert(k, v);
                    }
                }
                Some("layer") if f.len() >= 6 => {
                    let id: i32 = f[1].parse().unwrap_or(0);
                    let pic = unesc(f[2]);
                    let a: Vec<i32> = split(f[3]);
                    let b: Vec<i32> = split(f[4]);
                    let kind: i32 = f[5].parse().unwrap_or(-1);
                    let p: Vec<i32> = split(f.get(6).copied().unwrap_or(""));
                    if pic.is_empty() {
                        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
                        self.layers.insert(
                            id,
                            Layer {
                                pic,
                                image: solid(1, 1, [0, 0, 0, 255]),
                                a: [0, 0, w, h],
                                b: [0, 0, w, h],
                                effect: None,
                            },
                        );
                    } else {
                        self.load_layer(id, &pic);
                    }
                    if let Some(l) = self.layers.get_mut(&id) {
                        if a.len() == 4 && b.len() == 4 {
                            l.a = [a[0], a[1], a[2], a[3]];
                            l.b = [b[0], b[1], b[2], b[3]];
                        }
                        l.effect = (kind >= 0).then_some(Effect { kind, p, start: 0 });
                    }
                }
                Some("bgm") if f.len() >= 2 => {
                    if let Ok(n) = f[1].parse() {
                        self.play_bgm(n);
                    }
                }
                _ => {}
            }
        }
        match at {
            Some(f) if self.load_script(&f.script).is_ok() => {
                self.run = f;
                // the message record runs again and shows the message
                self.in_command = true;
            }
            _ => self.back_to_title(),
        }
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    /// The text being shown (for tests and the native runner).
    pub fn shown_text(&self) -> String {
        self.msg
            .as_ref()
            .map(|m| format!("{}{}", m.name, m.lines.concat()))
            .unwrap_or_default()
    }

    /// Where the script is and what it waits for (for tracing).
    pub fn position(&self) -> String {
        let wait = match &self.wait {
            Wait::None => "running".to_string(),
            Wait::Until(t) => format!("until {t} (now {})", self.now),
            Wait::Click => "click".into(),
            Wait::Select(o) => format!("select {}", o.len()),
            Wait::Idle => "idle".into(),
        };
        format!("{} {} {wait}", self.run.script, self.run.pc)
    }

    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Opening => "opening",
            Mode::Title => "title",
            Mode::Game => "game",
        }
    }
}

/// The requests of a request list: a header line, then number, block, name, ... with the
/// description in fields 9 to 16 (`@` for none).
fn requests(t: &str) -> Vec<(i32, String)> {
    t.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let n: i32 = f.first()?.trim().parse().ok()?;
            let desc: String = f
                .iter()
                .skip(9)
                .take(8)
                .filter(|s| !s.trim().is_empty() && s.trim() != "@")
                .map(|s| s.trim())
                .collect();
            Some((n, desc))
        })
        .collect()
}

/// The slots of a chart file: `@id kind<tab>x y next... text`; the text after ` !` is the
/// slot's text once played.
fn chart(t: &str) -> Vec<(i32, Vec<i32>, String)> {
    let mut out = Vec::new();
    for l in t.lines() {
        let Some(l) = l.strip_prefix('@') else {
            continue;
        };
        let mut it = l.split_whitespace();
        let Some(id) = it.next().and_then(|x| x.parse().ok()) else {
            continue;
        };
        let nums: Vec<&str> = it.collect();
        // kind, x, y, then the slots it leads to, then the text
        let mut links = Vec::new();
        let mut k = 3;
        while let Some(n) = nums.get(k).and_then(|x| x.parse::<i32>().ok()) {
            links.push(n);
            k += 1;
        }
        let text = nums.get(k..).unwrap_or(&[]).join(" ");
        let text = plain(text.split(" !").next().unwrap_or_default());
        out.push((id, links, text));
    }
    out
}

/// Text without the colour codes (`#` and a letter such as 黄 or 白).
fn plain(t: &str) -> String {
    let mut out = String::new();
    let mut it = t.chars();
    while let Some(c) = it.next() {
        if c == '#' {
            it.next();
        } else {
            out.push(c);
        }
    }
    out
}

fn slot_title(t: &str) -> String {
    t.lines()
        .find_map(|l| l.strip_prefix("title\t"))
        .map(unesc)
        .unwrap_or_default()
}

fn join<T: ToString>(v: &[T]) -> String {
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn split<T: std::str::FromStr>(s: &str) -> Vec<T> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

/// Part of a picture (`src`: x, y, w, h) at (x, y).
fn part(image: &Rc<Image>, src: [i32; 4], x: f32, y: f32) -> Draw {
    let mut d = draw(image, x - src[0] as f32, y - src[1] as f32, 1.0);
    d.place.clip = [x, y, x + src[2] as f32, y + src[3] as f32];
    d
}

fn draw(image: &Rc<Image>, x: f32, y: f32, alpha: f32) -> Draw {
    let (w, h) = (image.w as f32, image.h as f32);
    Draw {
        image: image.clone(),
        src: [0, 0, image.w as i32, image.h as i32],
        dst: [x, y, x + w, y + h],
        place: Place {
            x,
            y,
            sx: 1.0,
            sy: 1.0,
            cx: 0.0,
            cy: 0.0,
            rot: 0.0,
            alpha,
            add: false,
            flip: (false, false),
            clip: [0.0, 0.0, 1e6, 1e6],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_fields() {
        assert_eq!(split::<i32>("1,-2,3"), vec![1, -2, 3]);
        assert_eq!(join(&[1, 2]), "1,2");
        assert_eq!(unesc(&esc("a\tb\\c")), "a\tb\\c");
        assert_eq!(slot_title("x\ntitle\tab\n"), "ab");
        let c = chart("1800\n@1002 1\t250 200 1004 #黄幼蟲#白の前半\n");
        assert_eq!(c, vec![(1002, vec![1004], "幼蟲の前半".to_string())]);
        let r = requests("head\n3,1,3,0,0,1,1,12,@,a,b,@,@,@,@,@,@,@,0\n");
        assert_eq!(r, vec![(3, "ab".to_string())]);
    }
}
