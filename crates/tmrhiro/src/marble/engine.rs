//! Runs a Marble engine game: the text scripts of `mg_data.mbl`, line by line.
//!
//! A line is a command (`*name args` or `@name args`, the same commands), a variable
//! (`!S016=5`), a label (`:NAME`), the speaker (`>name`), a page end (`\`), the start of a
//! branch (`{`, with the option's text after a `*select`) or the end of the branches (`}`), and
//! otherwise text. A `*select`, `*clickmap` or `*cjmp` is followed by its branches; a branch
//! runs until the next `{` or `}`, then the script goes on after the `}`.
//!
//! `*gra NAME,mode` shows a picture: names starting with A are backgrounds, S and three digits
//! (0..9, A..G) a colour. The mode picks the transition: 0 only prepares the picture, 4 shows it
//! at once, 5 wipes through the `*wipeset` mask, the others fade (`!S016` sets the speed of 3
//! and 17). These follow the game's executable.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use super::{mbl, prs, wady};
use crate::engine::Sound;

const MS_PER_FRAME: u64 = 16;

/// (fade step out of 256, ms a step, ms a step of mode 17) for `!S016` 0..=16
const SPEEDS: [(u64, u64, u64); 17] = [
    (4, 8, 20),
    (32, 4, 1),
    (32, 8, 2),
    (32, 12, 3),
    (16, 8, 5),
    (16, 12, 8),
    (8, 8, 12),
    (8, 12, 16),
    (4, 8, 20),
    (4, 10, 30),
    (4, 12, 40),
    (2, 8, 60),
    (2, 12, 70),
    (1, 8, 80),
    (1, 12, 100),
    (1, 16, 120),
    (1, 20, 140),
];

struct Script {
    name: String,
    lines: Vec<String>,
    labels: HashMap<String, usize>,
    /// a branch start (`{`) → the `}` that ends its branches
    group_end: HashMap<usize, usize>,
    /// a `*select`/`*clickmap`/`*cjmp` → its branch starts
    branches: HashMap<usize, Vec<usize>>,
}

impl Script {
    fn parse(name: &str, text: &str) -> Script {
        let lines: Vec<String> = text
            .split(['\n', '\0'])
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        let mut labels = HashMap::new();
        let mut group_end = HashMap::new();
        let mut branches = HashMap::new();
        let mut open: Vec<(usize, Vec<usize>)> = Vec::new();
        let mut opener: Option<usize> = None;
        for (i, l) in lines.iter().enumerate() {
            if let Some(label) = l.strip_prefix(':') {
                labels.entry(label.trim().to_uppercase()).or_insert(i);
            } else if l.starts_with('{') {
                match opener.take() {
                    Some(o) => open.push((o, vec![i])),
                    None => {
                        if let Some(g) = open.last_mut() {
                            g.1.push(i);
                        }
                    }
                }
            } else if l.starts_with('}') {
                if let Some((o, b)) = open.pop() {
                    for &s in &b {
                        group_end.insert(s, i);
                    }
                    branches.insert(o, b);
                }
            } else if let Some(cmd) = l.strip_prefix(['*', '@']) {
                let word = cmd.split([' ', ',']).next().unwrap_or("").to_lowercase();
                if matches!(
                    word.as_str(),
                    "select" | "clickmap" | "cjmp" | "seltree" | "switch"
                ) {
                    opener = Some(i);
                } else if word != "mark" {
                    opener = None;
                }
            }
        }
        Script {
            name: name.to_uppercase(),
            lines,
            labels,
            group_end,
            branches,
        }
    }
}

#[derive(Clone, Default)]
struct Scene {
    bg: Option<Rc<Image>>,
    sprites: Vec<(Rc<Image>, f32, f32)>,
    ovr: Option<Rc<Image>>,
}

struct Trans {
    old: Rc<Image>,
    start: u64,
    ms: u64,
    mask: Option<Rc<Image>>,
}

#[derive(PartialEq)]
enum Wait {
    None,
    Click,
    Until(u64),
    Trans,
    Choice,
    Map,
}

struct Choice {
    options: Vec<String>,
    starts: Vec<usize>,
}

pub struct Engine {
    files: Box<dyn Files>,
    /// (kind, name) → (archive file, entry); kind is the archive's name without "mg_" and digits
    index: HashMap<(String, String), (String, mbl::Entry)>,
    cfg: HashMap<String, String>,
    sprset: HashMap<String, Vec<(String, i32, i32)>>,
    maps: HashMap<String, Vec<[i32; 4]>>,
    scripts: HashMap<String, Rc<Script>>,
    script: Rc<Script>,
    pc: usize,
    stack: Vec<(Rc<Script>, usize)>,
    vars: HashMap<String, i32>,
    names: [String; 10],
    pub screen: (u32, u32),
    now: u64,
    next: Scene,
    shown: Scene,
    trans: Option<Trans>,
    wipe: Option<String>,
    shake: Option<(i32, i32, u64, u64)>,
    voice_end: u64,
    window: bool,
    novel: bool,
    speaker: String,
    page: Vec<String>,
    wait: Wait,
    choice: Option<Choice>,
    map: Option<(Vec<[i32; 4]>, Vec<usize>)>,
    parts: Option<Rc<Image>>,
    pointer: (i32, i32),
    sounds: Vec<Sound>,
    font: Option<Rc<fontdue::Font>>,
    images: HashMap<String, Rc<Image>>,
    ended: bool,
    pub log: bool,
}

fn num(s: &str) -> i32 {
    s.trim().parse().unwrap_or(0)
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut index = HashMap::new();
        for file in files.list() {
            let lower = file.to_lowercase();
            let Some(stem) = lower.strip_suffix(".mbl") else {
                continue;
            };
            let kind = stem
                .trim_start_matches("mg_")
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .to_string();
            let Some(size) = files.size(&file) else {
                continue;
            };
            let Some(head) = files.read_range(&file, 0, 8.min(size as usize)) else {
                continue;
            };
            let Some(n) = mbl::Index::index_size(&head) else {
                continue;
            };
            let Some(b) = files.read_range(&file, 0, n.min(size as usize)) else {
                continue;
            };
            let Ok(ix) = mbl::Index::parse(&b, size) else {
                continue;
            };
            for (name, e) in ix.entries {
                index
                    .entry((kind.clone(), name))
                    .or_insert((file.clone(), e));
            }
        }
        if !index.keys().any(|(k, _)| k == "data") {
            return Err("marble: no mg_data.mbl".into());
        }
        let empty = Rc::new(Script::parse("", ""));
        let mut e = Engine {
            files,
            index,
            cfg: HashMap::new(),
            sprset: HashMap::new(),
            maps: HashMap::new(),
            scripts: HashMap::new(),
            script: empty,
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            names: Default::default(),
            screen: (800, 600),
            now: 0,
            next: Scene::default(),
            shown: Scene::default(),
            trans: None,
            wipe: None,
            shake: None,
            voice_end: 0,
            window: false,
            novel: false,
            speaker: String::new(),
            page: Vec::new(),
            wait: Wait::None,
            choice: None,
            map: None,
            parts: None,
            pointer: (-1, -1),
            sounds: Vec::new(),
            font: None,
            images: HashMap::new(),
            ended: false,
            log: false,
        };
        if let Some(s) = e.script_text("_CONFIG") {
            for l in s.lines() {
                if let Some((k, v)) = l.split_once(':') {
                    e.cfg.insert(k.trim().to_uppercase(), v.trim().to_string());
                }
            }
        }
        e.names[0] = e.cfg.get("DEFAULT_NAME").cloned().unwrap_or_default();
        e.names[1] = e.cfg.get("DEFAULT_NAME2").cloned().unwrap_or_default();
        if let Some(s) = e.script_text("_SPRSET") {
            let mut cur: Option<String> = None;
            for l in s.lines().map(str::trim) {
                if let Some(name) = l.strip_prefix(':') {
                    cur = Some(name.to_uppercase());
                    e.sprset.entry(name.to_uppercase()).or_default();
                } else if let (Some(c), false) = (&cur, l.is_empty()) {
                    let v: Vec<&str> = l.split(',').collect();
                    let (x, y) = (
                        v.get(1).map_or(0, |s| num(s)),
                        v.get(2).map_or(0, |s| num(s)),
                    );
                    e.sprset
                        .get_mut(c)
                        .unwrap()
                        .push((v[0].trim().to_uppercase(), x, y));
                }
            }
        }
        if let Some(s) = e.script_text("MAP") {
            let mut cur: Option<String> = None;
            for l in s.lines().map(str::trim) {
                if let Some(name) = l.strip_prefix(':') {
                    cur = Some(name.to_uppercase());
                } else if let (Some(c), Some(r)) = (&cur, l.strip_prefix("*,")) {
                    let v: Vec<i32> = r.split(',').map(num).collect();
                    if v.len() >= 4 {
                        e.maps
                            .entry(c.clone())
                            .or_default()
                            .push([v[0], v[1], v[2], v[3]]);
                    }
                }
            }
        }
        e.chain("MAIN", None);
        Ok(e)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let font = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())?;
        self.font = Some(Rc::new(font));
        Ok(())
    }

    fn read(&mut self, kinds: &[&str], name: &str) -> Option<Vec<u8>> {
        let name = name.to_uppercase();
        let (file, e) = kinds
            .iter()
            .find_map(|k| self.index.get(&(k.to_string(), name.clone())))?
            .clone();
        self.files
            .read_range(&file, e.offset as u64, e.size as usize)
    }

    fn script_text(&mut self, name: &str) -> Option<String> {
        let b = self.read(&["data"], name)?;
        Some(nexas::sjis::decode(&mbl::script(&b)).replace('\0', "\n"))
    }

    fn load(&mut self, name: &str) -> Option<Rc<Script>> {
        let key = name.to_uppercase();
        if let Some(s) = self.scripts.get(&key) {
            return Some(s.clone());
        }
        let text = self.script_text(&key)?;
        let s = Rc::new(Script::parse(&key, &text));
        self.scripts.insert(key, s.clone());
        Some(s)
    }

    fn cfg(&self, key: &str, default: i32) -> i32 {
        self.cfg.get(key).map_or(default, |v| num(v))
    }

    /// A picture; system pictures (SYS_*) keep their mask in the lower half, the others in a
    /// picture named with M for the first letter.
    fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let name = name.to_uppercase();
        if let Some(i) = self.images.get(&name) {
            return Some(i.clone());
        }
        let mut img = prs::decode(&self.read(&["gra", "gjp"], &name)?).ok()?;
        if name.starts_with("SYS_") {
            img = lower_mask(&img, img.h / 2);
        } else if name.starts_with('B') {
            let mask = format!("M{}", name.strip_prefix('B').unwrap_or(&name));
            if let Some(m) = self
                .read(&["gra", "gjp"], &mask)
                .and_then(|b| prs::decode(&b).ok())
            {
                apply_mask(&mut img, &m, 0, 0);
            }
        }
        let img = Rc::new(img);
        if self.images.len() > 48 {
            self.images.clear();
        }
        self.images.insert(name, img.clone());
        Some(img)
    }

    fn var(&self, name: &str) -> i32 {
        if let Ok(n) = name.parse() {
            return n;
        }
        self.vars.get(&name.to_uppercase()).copied().unwrap_or(0)
    }

    fn eval(&self, expr: &str) -> i32 {
        let mut total = 0i32;
        let mut op = '+';
        let mut term = String::new();
        for c in expr.chars().chain(std::iter::once('\0')) {
            if matches!(c, '+' | '-' | '*' | '/' | '\0') && !term.is_empty() {
                let v = self.var(term.trim());
                total = match op {
                    '-' => total.wrapping_sub(v),
                    '*' => total.wrapping_mul(v),
                    '/' => total.checked_div(v).unwrap_or(0),
                    _ => total.wrapping_add(v),
                };
                term.clear();
                op = c;
            } else if c != '\0' {
                term.push(c);
            }
        }
        total
    }

    fn speed(&self) -> (u64, u64, u64) {
        SPEEDS[self.var("S016").clamp(0, 16) as usize]
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        match self.wait {
            Wait::Click => {
                self.wait = Wait::None;
                if !self.novel {
                    self.page.clear();
                }
                self.speaker.clear();
            }
            Wait::Trans => self.trans = None,
            Wait::Choice => {
                let n = self.choice.as_ref().map_or(0, |c| c.options.len());
                if let Some(k) = self.choice_at(n, x, y) {
                    let c = self.choice.take().unwrap();
                    self.pc = c.starts[k] + 1;
                    self.wait = Wait::None;
                    self.page.clear();
                }
            }
            Wait::Map => {
                let hit = self
                    .map
                    .as_ref()
                    .and_then(|(rects, _)| rects.iter().position(|r| inside(*r, x, y)));
                if let Some(k) = hit {
                    let (_, starts) = self.map.take().unwrap();
                    match starts.get(k) {
                        Some(&s) => self.pc = s + 1,
                        None => self.skip_group(),
                    }
                    self.parts = None;
                    self.wait = Wait::None;
                }
            }
            _ => {}
        }
    }

    pub fn cancel(&mut self) {
        if self.wait == Wait::Click {
            self.window = !self.window;
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up {
            let (x, y) = self.pointer;
            self.click(x, y);
        }
    }

    pub fn start_at(&mut self, name: &str) {
        let (file, label) = match name.split_once(',') {
            Some((f, l)) => (f, Some(l)),
            None => (name, None),
        };
        self.chain(file, label);
    }

    fn chain(&mut self, file: &str, label: Option<&str>) {
        match self.load(file) {
            Some(s) => {
                self.script = s;
                self.pc = label
                    .and_then(|l| self.script.labels.get(&l.to_uppercase()).copied())
                    .unwrap_or(0);
            }
            None => eprintln!("marble: no script {file}"),
        }
    }

    /// After the current branch: past the `}` of the group whose branch starts at `start`.
    fn skip_group(&mut self) {
        let line = self.pc.saturating_sub(1);
        if let Some(&end) = self.script.group_end.get(&line) {
            self.pc = end + 1;
        }
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.now += MS_PER_FRAME;
        if self
            .trans
            .as_ref()
            .is_some_and(|t| self.now >= t.start + t.ms)
        {
            self.trans = None;
        }
        if self.wait == Wait::Trans && self.trans.is_none() {
            self.wait = Wait::None;
        }
        if matches!(self.wait, Wait::Until(t) if self.now >= t) {
            self.wait = Wait::None;
        }
        if self
            .shake
            .is_some_and(|(_, _, start, ms)| self.now >= start + ms)
        {
            self.shake = None;
        }
        let mut budget = 2000;
        while self.wait == Wait::None && !self.ended && budget > 0 {
            budget -= 1;
            let Some(line) = self.script.lines.get(self.pc).cloned() else {
                self.end();
                continue;
            };
            self.pc += 1;
            if self.log {
                eprintln!("{}:{} {line}", self.script.name, self.pc);
            }
            self.line(&line);
        }
        Ok(())
    }

    /// The end of a script (or `*end`): back to the caller, or to the title.
    fn end(&mut self) {
        match self.stack.pop() {
            Some((s, pc)) => {
                self.script = s;
                self.pc = pc;
            }
            None => self.chain("MAIN", None),
        }
    }

    fn line(&mut self, line: &str) {
        let Some(first) = line.chars().next() else {
            return;
        };
        match first {
            ';' | ':' | '}' => {}
            '{' => {
                // the end of a branch: on after the group
                if let Some(&end) = self.script.group_end.get(&(self.pc - 1)) {
                    self.pc = end + 1;
                }
            }
            '!' => {
                if let Some((k, v)) = line[1..].split_once('=') {
                    let v = self.eval(v);
                    self.vars.insert(k.trim().to_uppercase(), v);
                }
            }
            '>' => self.speaker = self.subst(&line[1..]),
            '\\' | '=' | '+' | '~' => self.wait = Wait::Click,
            '*' | '@' => self.command(&line[1..]),
            _ => self.text(line),
        }
    }

    fn subst(&self, s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            let digit = chars.peek().and_then(|d| {
                d.to_digit(10)
                    .or_else(|| ('０'..='９').contains(d).then(|| *d as u32 - '０' as u32))
            });
            if let (true, Some(d)) = (c == '＄', digit) {
                chars.next();
                out.push_str(&self.names[d as usize]);
                continue;
            }
            out.push(c);
        }
        out
    }

    fn text(&mut self, line: &str) {
        let mut s = String::new();
        let mut ruby = false;
        let mut page_end = false;
        for c in self.subst(line).chars() {
            match c {
                '<' => ruby = true,
                '>' => ruby = false,
                '▼' => page_end = true,
                _ if ruby => {}
                _ => s.push(c),
            }
        }
        self.window = true;
        let (cols, rows) = if self.novel {
            (self.cfg("MOJI_MAX2_X", 52), self.cfg("MOJI_MAX2_Y", 12))
        } else {
            (self.cfg("MOJI_MAX_X", 42), self.cfg("MOJI_MAX_Y", 4))
        };
        for l in wrap(&s, cols.max(2) as usize) {
            if self.page.len() >= rows.max(1) as usize {
                self.page.clear();
            }
            self.page.push(l);
        }
        if page_end {
            self.wait = Wait::Click;
        }
    }

    fn command(&mut self, cmd: &str) {
        let (name, args) = match cmd.find([' ', '\t']) {
            Some(i) => (&cmd[..i], cmd[i + 1..].trim()),
            None => (cmd, ""),
        };
        let name = name.to_lowercase();
        let a: Vec<&str> = args.split(',').map(str::trim).collect();
        let n = |k: usize| a.get(k).map_or(0, |s| num(s));
        // a transition left out is -1, a short fade
        let mode = |k: usize| a.get(k).filter(|s| !s.is_empty()).map_or(-1, |s| num(s));
        match name.as_str() {
            "gra" | "grb" | "bg" => {
                let pic = a[0].to_uppercase();
                if pic.starts_with('S') && pic.len() == 4 {
                    let level = |c: char| (c.to_digit(17).unwrap_or(0) * 16).min(255) as u8;
                    let c: Vec<u8> = pic[1..].chars().map(level).collect();
                    let mut img = Image::new(self.screen.0, self.screen.1);
                    img.fill([c[0], c[1], c[2], 255]);
                    self.next.bg = Some(Rc::new(img));
                    self.next.sprites.clear();
                } else if pic.starts_with('B') {
                    if let Some(img) = self.image(&pic) {
                        self.next.sprites = vec![(img, 0.0, 0.0)];
                    }
                } else if let Some(img) = self.image(&pic) {
                    if img.w > self.screen.0 || self.shown.bg.is_none() {
                        self.screen = (self.screen.0.max(800), self.screen.1.max(600));
                    }
                    self.next.bg = Some(img);
                    self.next.sprites.clear();
                } else if self.log {
                    eprintln!("marble: no picture {pic}");
                }
                self.show(mode(1));
            }
            "spr" => {
                let set = a[0].trim_start_matches('#').to_uppercase();
                let parts = self
                    .sprset
                    .get(&set)
                    .cloned()
                    .unwrap_or_else(|| vec![(set.clone(), 0, 0)]);
                let mut sprites = Vec::new();
                for (pic, x, y) in parts {
                    if let Some(img) = self.image(&pic) {
                        sprites.push((img, x as f32, y as f32));
                    }
                }
                self.next.sprites = sprites;
                self.show(mode(1));
            }
            "clrb" => {
                self.next.sprites.clear();
                self.show(mode(0));
            }
            "ovr" => {
                let pic = a[0].to_uppercase();
                self.next.ovr = if pic == "OFF" {
                    None
                } else {
                    self.image(&pic).map(|i| Rc::new(side_mask(&i)))
                };
                self.show(mode(1));
            }
            "wipeset" => self.wipe = Some(a[0].to_uppercase()),
            "music" => self.music(n(0)),
            "se" | "seloop" => {
                let looped = name == "seloop";
                self.play(&["se", "wav"], a[0], false, looped);
            }
            "seoff" => self.sounds.push(Sound {
                data: Vec::new(),
                volume: 1.0,
                bgm: false,
                voice: false,
                effect_channel: Some(-1),
                stop: true,
                fade_ms: 0,
                loop_start: 0,
            }),
            "cv" => {
                if let Some(ms) = self.play(&["ogg", "wav"], a[0], true, false) {
                    self.voice_end = self.now + ms;
                }
            }
            "wait_v_end" => self.wait = Wait::Until(self.voice_end),
            "if" => {
                // "(cond) *command" or "(cond) label"
                let Some((cond, rest)) = args.strip_prefix('(').and_then(|r| r.split_once(')'))
                else {
                    return;
                };
                if self.test(cond) {
                    let rest = rest.trim();
                    if rest.starts_with(['*', '@', '!']) {
                        self.line(rest);
                    } else {
                        self.goto(rest);
                    }
                }
            }
            "switch" => {
                let line = self.pc - 1;
                let starts = self.script.branches.get(&line).cloned().unwrap_or_default();
                let hit = starts.iter().find(|&&s| {
                    let l = &self.script.lines[s];
                    l[1..]
                        .trim()
                        .strip_prefix('(')
                        .and_then(|r| r.split_once(')'))
                        .is_some_and(|(c, _)| self.test(c))
                });
                match (hit, starts.first()) {
                    (Some(&s), _) => self.pc = s + 1,
                    (None, Some(s)) => self.pc = self.script.group_end[s] + 1,
                    _ => {}
                }
            }
            "sleep" | "wait" | "delay" => self.wait = Wait::Until(self.now + n(0).max(0) as u64),
            "shake" => {
                self.shake = Some((n(0), n(1), self.now, (n(2).max(1) * 6) as u64));
                self.wait = Wait::Until(self.now + (n(2).max(1) * 6) as u64);
            }
            "go" => self.goto(a[0]),
            "chain" | "call" => {
                let (file, label) = match args.split_once(':') {
                    Some((f, l)) => (f.trim(), Some(l.trim())),
                    None => (args.split(' ').next().unwrap_or(""), None),
                };
                if self.load(file).is_none() {
                    eprintln!("marble: no script {file}, going on");
                    return;
                }
                if name == "call" {
                    self.stack.push((self.script.clone(), self.pc));
                } else {
                    self.stack.clear();
                }
                self.chain(file, label);
            }
            "end" => self.end(),
            "select" => {
                let line = self.pc - 1;
                let starts = self.script.branches.get(&line).cloned().unwrap_or_default();
                let options = starts
                    .iter()
                    .map(|&s| self.subst(self.script.lines[s][1..].trim()))
                    .collect();
                self.choice = Some(Choice { options, starts });
                self.wait = Wait::Choice;
            }
            "clickmap" => {
                let line = self.pc - 1;
                let starts = self.script.branches.get(&line).cloned().unwrap_or_default();
                let rects = self
                    .maps
                    .get(&a[0].to_uppercase())
                    .cloned()
                    .unwrap_or_default();
                self.window = false;
                self.map = Some((rects, starts));
                self.wait = Wait::Map;
            }
            "cjmp" => {
                let line = self.pc - 1;
                let v = self.var(a[0]);
                let starts = self.script.branches.get(&line).cloned().unwrap_or_default();
                match usize::try_from(v).ok().and_then(|v| starts.get(v)) {
                    Some(&s) => self.pc = s + 1,
                    None => {
                        if let Some(&s) = starts.first() {
                            self.pc = self.script.group_end[&s] + 1;
                        }
                    }
                }
            }
            "partsload" => self.parts = self.image(a[0]),
            "gameinit" | "gameclr" => {
                self.vars.retain(|k, _| k.starts_with('S'));
            }
            "win_close" => {
                self.window = false;
                self.page.clear();
            }
            "win_open" => self.window = true,
            "tw_set" => {
                self.novel = n(0) == 1;
                self.page.clear();
            }
            "exitgame" | "exit" => self.ended = true,
            "mark" | "break" | "title" | "mode" | "setname" | "load" | "save" | "fix"
            | "special" | "rec" | "vend" | "backto" | "string" => {}
            _ => {
                if self.log {
                    eprintln!("marble: unknown command {cmd}");
                }
            }
        }
    }

    /// Shows the prepared scene with transition `mode`.
    fn show(&mut self, mode: i32) {
        if mode == 0 {
            return;
        }
        let mode = if mode < 0 { 1 } else { mode };
        let (step, delay, t17) = self.speed();
        let ms = match mode {
            4 => 0,
            1 => 160,
            2 => 480,
            17 => self.var("S016").clamp(1, 16) as u64 * t17 * 8,
            _ => 256 / step * delay,
        };
        if ms == 0 {
            self.shown = self.next.clone();
            return;
        }
        let old = Rc::new(self.render_scene(&self.shown.clone()));
        let mask = if mode == 5 {
            let w = self.wipe.clone();
            w.and_then(|m| self.image(&m))
        } else {
            None
        };
        self.shown = self.next.clone();
        self.trans = Some(Trans {
            old,
            start: self.now,
            ms,
            mask,
        });
        self.wait = Wait::Trans;
    }

    fn music(&mut self, n: i32) {
        if n <= 0 {
            self.sounds.push(Sound {
                data: Vec::new(),
                volume: 1.0,
                bgm: true,
                voice: false,
                effect_channel: None,
                stop: true,
                fade_ms: (-n).max(0) as u32 * 100,
                loop_start: 0,
            });
            return;
        }
        let name = format!("M{n:02}");
        if let Some(b) = self.read(&["bgm"], &name) {
            let data = wady::to_wav(&b).unwrap_or(b);
            self.sounds.push(Sound {
                data,
                volume: 1.0,
                bgm: true,
                voice: false,
                effect_channel: None,
                stop: false,
                fade_ms: 0,
                loop_start: 0,
            });
        } else if self.log {
            eprintln!("marble: no music {name}");
        }
    }

    /// Plays a sound; returns how long it lasts (in ms) when that is known.
    fn play(&mut self, kinds: &[&str], name: &str, voice: bool, looped: bool) -> Option<u64> {
        let Some(b) = self.read(kinds, name) else {
            if self.log {
                eprintln!("marble: no sound {name}");
            }
            return None;
        };
        let data = wady::to_wav(&b).unwrap_or(b);
        let ms = length_ms(&data);
        self.sounds.push(Sound {
            data,
            volume: 1.0,
            bgm: looped,
            voice,
            effect_channel: (!voice && !looped).then_some(0),
            stop: false,
            fade_ms: 0,
            loop_start: 0,
        });
        ms
    }

    fn goto(&mut self, label: &str) {
        let label = label.trim().to_uppercase();
        match self.script.labels.get(&label) {
            Some(&l) => self.pc = l + 1,
            None => eprintln!("marble: no label {label} in {}", self.script.name),
        }
    }

    /// A condition: `A=1`, `A!=B`, `A<3`, ...
    fn test(&self, cond: &str) -> bool {
        for op in [">=", "<=", "!=", "<>", "==", "=", ">", "<"] {
            if let Some((l, r)) = cond.split_once(op) {
                let (l, r) = (self.eval(l), self.eval(r));
                return match op {
                    ">=" => l >= r,
                    "<=" => l <= r,
                    "!=" | "<>" => l != r,
                    ">" => l > r,
                    "<" => l < r,
                    _ => l == r,
                };
            }
        }
        self.eval(cond) != 0
    }

    fn scene_draws(&self, s: &Scene, out: &mut Vec<Draw>) {
        let (dx, dy) = match self.shake {
            Some((kind, amp, start, _)) => {
                let flip = if ((self.now - start) / 32).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                let a = amp as f32 * flip;
                match kind {
                    0 => (0.0, a),
                    1 => (a, 0.0),
                    _ => (a, a),
                }
            }
            None => (0.0, 0.0),
        };
        if let Some(bg) = &s.bg {
            out.push(draw(bg, dx, dy, 1.0));
        }
        for (img, x, y) in &s.sprites {
            out.push(draw(img, x + dx, y + dy, 1.0));
        }
        if let Some(o) = &s.ovr {
            out.push(draw(o, dx, dy, 1.0));
        }
    }

    fn render_scene(&self, s: &Scene) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        let mut d = Vec::new();
        self.scene_draws(s, &mut d);
        for d in d {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    fn text_font(&self, size: i32) -> Option<Font> {
        self.font.clone().map(|f| Font::outline(f, size))
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        let shown = self.shown.clone();
        self.scene_draws(&shown, &mut out);
        if let Some(t) = &self.trans {
            let p = ((self.now - t.start) as f32 / t.ms as f32).clamp(0.0, 1.0);
            match &t.mask {
                Some(m) => {
                    let img = Rc::new(wipe(&t.old, m, p));
                    out.push(draw(&img, 0.0, 0.0, 1.0));
                }
                None => out.push(draw(&t.old, 0.0, 0.0, 1.0 - p)),
            }
        }
        if self.window && self.trans.is_none() && self.wait != Wait::Map {
            self.window_draws(&mut out);
        }
        if let Some(c) = &self.choice {
            let options = c.options.clone();
            self.choice_draws(&options, &mut out);
        }
        if let (Some((rects, _)), Some(parts)) = (&self.map, &self.parts) {
            let (x, y) = self.pointer;
            if let Some(r) = rects.iter().find(|r| inside(**r, x, y)) {
                out.push(part(parts, *r));
            }
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let pitch_x = self.cfg("TX_PITCH_X", 13);
        let size = (pitch_x * 2 - 2).min(self.cfg("FONT_SIZE", 20) + 4).max(12);
        let (x0, y0, pitch_y) = if self.novel {
            let mut dark = Image::new(self.screen.0, self.screen.1);
            let a = self.cfg("TXW_ALPHA2", 120).clamp(0, 255) as u8;
            dark.fill([0, 0, 0, a]);
            out.push(draw(&Rc::new(dark), 0.0, 0.0, 1.0));
            (
                self.cfg("TX2_OFS_X", 32),
                self.cfg("TX2_OFS_Y", 32),
                self.cfg("TX2_PITCH_Y", 40),
            )
        } else {
            let (wx, wy) = (self.cfg("TXW_OFS_X", 76), self.cfg("TXW_OFS_Y", 407));
            if let Some(tw) = self.image("SYS_TW") {
                out.push(draw(&tw, wx as f32, wy as f32, 1.0));
            }
            (
                wx + self.cfg("TX_OFS_X", 10),
                wy + self.cfg("TX_OFS_Y", 20),
                self.cfg("TX_PITCH_Y", 36),
            )
        };
        let Some(font) = self.text_font(size) else {
            return;
        };
        if !self.novel && !self.speaker.is_empty() && !self.page.is_empty() {
            let img = Rc::new(font.render(&nexas::sjis::encode(&self.speaker), [255; 3], 2));
            let (nx, ny) = (self.cfg("NP_OFS_X", 84), self.cfg("NP_OFS_Y", 407));
            let nh = self.cfg("NP_SZ_Y", 36);
            out.push(draw(
                &img,
                nx as f32 + 8.0,
                (ny + (nh - size) / 2 - 2) as f32,
                1.0,
            ));
        }
        for (k, l) in self.page.iter().enumerate() {
            let img = Rc::new(font.render(&nexas::sjis::encode(l), [255; 3], 2));
            out.push(draw(
                &img,
                x0 as f32 - 2.0,
                (y0 + pitch_y * (k as i32 + 1) - 15) as f32 - 2.0,
                1.0,
            ));
        }
        let icon = (self.wait == Wait::Click)
            .then(|| self.image("SYS_CLICKW"))
            .flatten();
        if let Some(icon) = icon {
            let (w, h) = (
                self.cfg("CLICK_WAIT_SIZE_X", 40),
                self.cfg("CLICK_WAIT_SIZE_Y", 40),
            );
            let frames = (icon.w as i32 / w.max(1)).max(1);
            let f = (self.now / 50) as i32 % frames;
            let (x, y) = (
                self.cfg("CLICK_WAIT_OFS_X", 659),
                self.cfg("CLICK_WAIT_OFS_Y", 526),
            );
            let mut d = draw(&icon, (x - f * w) as f32, y as f32, 1.0);
            d.place.clip = [x as f32, y as f32, (x + w) as f32, (y + h) as f32];
            out.push(d);
        }
    }

    fn choice_rects(&self, n: usize) -> Vec<[i32; 4]> {
        let (w, h) = (464, 41);
        let gap = 56;
        let top = (self.screen.1 as i32 - 120 - gap * n as i32) / 2;
        (0..n)
            .map(|k| {
                let x = (self.screen.0 as i32 - w) / 2;
                let y = top + gap * k as i32;
                [x, y, x + w, y + h]
            })
            .collect()
    }

    fn choice_at(&self, n: usize, x: i32, y: i32) -> Option<usize> {
        self.choice_rects(n).iter().position(|r| inside(*r, x, y))
    }

    fn choice_draws(&mut self, options: &[String], out: &mut Vec<Draw>) {
        let rects = self.choice_rects(options.len());
        let (px, py) = self.pointer;
        let font = self.text_font(20);
        for (r, text) in rects.iter().zip(options) {
            let hover = inside(*r, px, py);
            if let Some(b) = self.image(if hover { "SYS_SW2" } else { "SYS_SW" }) {
                out.push(draw(&b, r[0] as f32, r[1] as f32, 1.0));
            }
            if let Some(f) = &font {
                let img = Rc::new(f.render(&nexas::sjis::encode(text), [255; 3], 2));
                let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
                let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
                out.push(draw(&img, x, y, 1.0));
            }
        }
    }

    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }
}

/// How long a WAV or Ogg Vorbis sound lasts.
fn length_ms(b: &[u8]) -> Option<u64> {
    if b.starts_with(b"RIFF") && b.len() >= 44 {
        let rate = u32::from_le_bytes(b[28..32].try_into().unwrap()) as u64;
        return (rate > 0).then(|| (b.len() as u64 - 44) * 1000 / rate);
    }
    if !b.starts_with(b"OggS") || b.len() < 44 {
        return None;
    }
    // the sample rate is in the identification header, the length in the last page's granule
    let rate = u32::from_le_bytes(b[40..44].try_into().unwrap()) as u64;
    let last = b.windows(4).rposition(|w| w == b"OggS")?;
    let granule = u64::from_le_bytes(b.get(last + 6..last + 14)?.try_into().unwrap());
    (rate > 0).then(|| granule * 1000 / rate)
}

/// Lines of at most `cols` half-width columns.
fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut used = 0;
    for c in s.chars() {
        let w = if (c as u32) < 0x80 || ('｡'..='ﾟ').contains(&c) {
            1
        } else {
            2
        };
        // closing punctuation stays on the line
        let hang = matches!(c, '、' | '。' | '」' | '』' | '）' | '…' | '！' | '？');
        if used + w > cols && !(hang && used <= cols) {
            out.push(String::new());
            used = 0;
        }
        out.last_mut().unwrap().push(c);
        used += w;
    }
    out
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

/// `r` (x0, y0, x1, y1) of `image`, in place.
fn part(image: &Rc<Image>, r: [i32; 4]) -> Draw {
    let mut d = draw(image, 0.0, 0.0, 1.0);
    d.place.clip = r.map(|v| v as f32);
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
            clip: [-1e9, -1e9, 1e9, 1e9],
        },
    }
}

/// The opacity of `img` from the brightness of `mask` placed at (x, y) on it.
fn apply_mask(img: &mut Image, mask: &Image, x: u32, y: u32) {
    for py in 0..img.h.min(mask.h.saturating_sub(y)) {
        for px in 0..img.w.min(mask.w.saturating_sub(x)) {
            let m = mask.px[(((py + y) * mask.w + px + x) * 4) as usize];
            img.px[((py * img.w + px) * 4 + 3) as usize] = m;
        }
    }
}

/// The upper `h` rows of `img`, with the rows below as their opacity.
fn lower_mask(img: &Image, h: u32) -> Image {
    let mut out = Image::new(img.w, h);
    out.px.copy_from_slice(&img.px[..(img.w * h * 4) as usize]);
    apply_mask(&mut out, img, 0, h);
    out
}

/// The left half of `img`, with the right half as its opacity.
fn side_mask(img: &Image) -> Image {
    let w = img.w / 2;
    let mut out = Image::new(w, img.h);
    for y in 0..img.h {
        let from = ((y * img.w) * 4) as usize;
        let to = ((y * w) * 4) as usize;
        out.px[to..to + (w * 4) as usize].copy_from_slice(&img.px[from..from + (w * 4) as usize]);
    }
    apply_mask(&mut out, img, w, 0);
    out
}

/// The old screen during a wipe: where the mask is darker than `p` it is gone.
fn wipe(old: &Image, mask: &Image, p: f32) -> Image {
    let mut out = old.clone();
    let t = p * 272.0 - 16.0;
    for (k, px) in out.px.chunks_exact_mut(4).enumerate() {
        let (x, y) = (k as u32 % old.w, k as u32 / old.w);
        let m = if x < mask.w && y < mask.h {
            mask.px[((y * mask.w + x) * 4) as usize] as f32
        } else {
            255.0
        };
        px[3] = ((m - t) * 16.0).clamp(0.0, 255.0) as u8;
    }
    out
}
