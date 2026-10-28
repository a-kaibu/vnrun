//! The story: .cst scenes played the way the games' sscript.kcs plays them (which this engine
//! does not run): pictures in layers bg/cg/fg/eg on planes (config.int/vram.xml: plane N shows
//! at 640,360 its point 640,360; cgN/fgN/egN sit on plane N, bgN on plane 0), the message
//! window of config.int/meswnd01.xml, voices, music and choices.
//!
//! Pictures and planes change on the spot; `rdraw` / `rwipe` crossfade from what the screen
//! showed in the last frame (the scenes change pictures and rdraw in the same frame).

use std::collections::HashMap;
use std::rc::Rc;

use nexas::gfx::{Draw, Image, Place};

use crate::cst::{Kind as Line, Scene};
use crate::engine::{Engine, Kind, Motion, Move, Obj, split_args, split_if};
use crate::save::SaveData;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Wait {
    None,
    Until(u64),
    /// motions, crossfades and text
    All,
    /// the message window's fade
    Window,
    Click,
    Select,
    /// a screen script the story started (`fex sel`), by run, and the flag for its exit code
    Fes(usize, i64),
}

/// The number and string variables.
pub(crate) type Vars = (HashMap<i64, i64>, HashMap<i64, String>);

pub struct Adv {
    pub owner: u32,
    /// the screen script that started the story (its `_EXITCODE_` says when the story ends)
    pub(crate) parent: usize,
    /// the scene entered last (not by `call`) and the variables then: where a load starts
    entry: Option<(String, Vars)>,
    /// the page showing: scene and the line it starts at
    mark: Option<(String, usize)>,
    /// the pages read: speaker, text
    pub(crate) backlog: Vec<(String, String)>,
    scene: Rc<Scene>,
    name: String,
    /// the scenes to go back to after a `call`ed one: scene, name, line
    calls: Vec<(Rc<Scene>, String, usize)>,
    pc: usize,
    wait: Wait,
    text: String,
    shown: f32,
    speaker: String,
    /// the next text starts a new page
    fresh: bool,
    window: f32,
    window_move: Option<(f32, f32, u64, u64)>,
    /// the window settings file and the theme in it (`view 1 meswnd_w`)
    theme: (String, String),
    win: Option<Rc<Theme>>,
    /// speaker -> number of its name picture (1000 + n in the theme's name pictures)
    name_ids: HashMap<String, i64>,
    drawdef: u64,
    wipedef: u64,
    /// the story's pictures as the last frame showed them
    pub prev: Vec<Draw>,
    trans: Option<(Vec<Draw>, u64, u64)>,
    choices: Vec<(String, String)>,
    hover: Option<usize>,
    planes: HashMap<i64, usize>,
    layers: HashMap<String, usize>,
    text_img: Option<(String, usize, Rc<Image>)>,
    name_img: Option<(String, Rc<Image>)>,
    choice_imgs: Vec<Rc<Image>>,
}

const TYPE_SPEED: f32 = 1.5;
const FONT: u32 = 26;
const PITCH: i32 = 34;

fn is_layer(s: &str) -> bool {
    matches!(s, "bg" | "cg" | "fg" | "eg")
}

/// `[0,1]`, `[0-3]` or `2` -> numbers.
fn indices(s: &str) -> Vec<i64> {
    let Some(inner) = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return s.parse().map(|v| vec![v]).unwrap_or_default();
    };
    let mut out = Vec::new();
    for part in inner.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                    out.extend(a..=b);
                }
            }
            None => out.extend(part.trim().parse::<i64>().ok()),
        }
    }
    out
}

/// Message text without its codes: `\n` breaks the line, `\fll` sizes, `[base/ruby]` ruby.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.next() {
                Some('n') => out.push('\n'),
                Some('f') => {
                    while it
                        .peek()
                        .is_some_and(|c| matches!(c, 'l' | 's' | 'n' | 'm'))
                    {
                        it.next();
                    }
                }
                _ => {}
            },
            '[' => {
                let mut base = String::new();
                let mut in_ruby = false;
                for d in it.by_ref() {
                    match d {
                        ']' => break,
                        '/' => in_ruby = true,
                        _ if !in_ruby => base.push(d),
                        _ => {}
                    }
                }
                out.push_str(&base);
            }
            _ => out.push(c),
        }
    }
    out
}

impl Adv {
    pub fn waits_fes(&self) -> bool {
        matches!(self.wait, Wait::Fes(..))
    }

    /// A page or a choice is showing, waiting for the player.
    pub fn reading(&self) -> bool {
        matches!(self.wait, Wait::Click | Wait::Select)
    }
}

impl Engine {
    /// `execkcs`: plays the story from the scene in string variable 200 (the games' sscript
    /// start file), or the first scene.
    pub fn start_story(&mut self, parent: usize) {
        // the save screen's choice (the games put it in $512, our menu answers 1000 + slot)
        let boot = self.flags.get(&512).copied().unwrap_or(0);
        if boot >= 1000 {
            self.flags.insert(512, -1);
            let k = (boot - 1000) as usize;
            match self
                .g
                .files
                .read_save(&crate::save::slot_name(k))
                .and_then(|b| SaveData::decode(&b))
            {
                Some(d) => {
                    self.load_save(parent, d);
                    return;
                }
                None => eprintln!("save slot {k}: nothing to load"),
            }
        }
        self.begin_story(parent);
    }

    fn begin_story(&mut self, parent: usize) {
        let name = self
            .strs
            .get(&200)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "00_01".into());
        let adv_xml = self.g.text("adv.xml").unwrap_or_default();
        let wnd = crate::game::xml_path(&adv_xml, &["etc", "defwnd"])
            .map(str::to_string)
            .or_else(|| self.g.find("meswnd01", &["xml"]))
            .unwrap_or_else(|| "meswnd.xml".into());
        let owner = self.new_owner();
        self.adv = Some(Adv {
            owner,
            parent,
            entry: None,
            mark: None,
            backlog: Vec::new(),
            scene: Rc::new(Scene { lines: Vec::new() }),
            name: String::new(),
            calls: Vec::new(),
            pc: 0,
            wait: Wait::None,
            text: String::new(),
            shown: 0.0,
            speaker: String::new(),
            fresh: true,
            window: 0.0,
            window_move: None,
            theme: (wnd, "default".into()),
            win: None,
            name_ids: name_table(&self.g.text("nametable.csv").unwrap_or_default()),
            drawdef: 15,
            wipedef: 30,
            prev: Vec::new(),
            trans: None,
            choices: Vec::new(),
            hover: None,
            planes: HashMap::new(),
            layers: HashMap::new(),
            text_img: None,
            name_img: None,
            choice_imgs: Vec::new(),
        });
        if let Err(e) = self.load_scene(&name) {
            eprintln!("{e}");
            self.end_story();
        }
    }

    /// `endkcs`, or the story's end: back to the screen scripts.
    pub fn end_story(&mut self) {
        let Some(a) = self.adv.take() else { return };
        self.drop_owner(a.owner);
        self.stop_sound("voice", 0);
        for k in 0..8 {
            self.stop_sound(&format!("bgm{k}"), 500);
            self.stop_sound(&format!("se{k}"), 0);
        }
        if let Some(r) = self.runs.get_mut(a.parent) {
            r.exit_code = 0;
        }
    }

    fn load_scene(&mut self, name: &str) -> Result<(), String> {
        let data = self
            .g
            .read(&format!("{name}.cst"))
            .ok_or(format!("{name}.cst: not found"))?;
        let scene = Rc::new(Scene::parse(&data)?);
        if self.log > 0 {
            eprintln!("scene {name}");
        }
        let a = self.adv.as_mut().unwrap();
        a.scene = scene;
        a.name = name.to_string();
        a.pc = 0;
        a.wait = Wait::None;
        if a.calls.is_empty() {
            a.entry = Some((name.to_string(), (self.flags.clone(), self.strs.clone())));
        }
        Ok(())
    }

    /// The story as a save slot holds it, when a page is showing.
    pub(crate) fn save_data(&self) -> Option<SaveData> {
        let a = self.adv.as_ref()?;
        let (entry, (ef, es)) = a.entry.as_ref()?;
        let (scene, line) = a.mark.as_ref()?;
        Some(SaveData {
            entry: entry.clone(),
            entry_flags: crate::save::sorted_flags(ef),
            entry_strs: crate::save::sorted_strs(es),
            scene: scene.clone(),
            line: *line as u32,
            flags: crate::save::sorted_flags(&self.flags),
            strs: crate::save::sorted_strs(&self.strs),
            speaker: a.speaker.clone(),
            text: a.text.clone(),
        })
    }

    /// Plays the story from a save: its scene from the top with the variables it was entered
    /// with, skipped through to the saved page (`run_story` then sets the saved variables).
    pub(crate) fn load_save(&mut self, parent: usize, d: SaveData) {
        if let Some(a) = self.adv.take() {
            self.drop_owner(a.owner);
        }
        self.stop_sound("voice", 0);
        for k in 0..8 {
            self.stop_sound(&format!("bgm{k}"), 0);
            self.stop_sound(&format!("se{k}"), 0);
        }
        self.flags = d.entry_flags.into_iter().collect();
        self.strs = d.entry_strs.into_iter().collect();
        self.strs.insert(200, d.entry.clone());
        self.pending_vars = Some((d.flags.into_iter().collect(), d.strs.into_iter().collect()));
        self.skip_to = Some((d.scene, d.line as usize));
        if let Some(r) = self.runs.get_mut(parent) {
            r.exit_code = -1;
        }
        self.begin_story(parent);
    }

    fn busy(&self) -> bool {
        let Some(a) = &self.adv else { return false };
        a.trans.is_some()
            || a.shown < a.text.chars().count() as f32
            || self
                .objs
                .iter()
                .flatten()
                .any(|o| o.owner == a.owner && !o.motions.is_empty())
    }

    pub fn adv_input(&mut self, clicked: bool, _rclicked: bool) {
        let mouse = self.mouse;
        let a = self.adv.as_mut().unwrap();
        if a.wait == Wait::Select {
            a.hover = choice_at(a.choices.len(), self.width, self.height, mouse);
            if clicked && let Some(k) = a.hover {
                let target = a.choices[k].0.clone();
                a.choices.clear();
                a.choice_imgs.clear();
                a.fresh = true;
                if let Err(e) = self.load_scene(&target) {
                    eprintln!("{e}");
                    self.end_story();
                }
            }
            return;
        }
        if !clicked {
            return;
        }
        let len = a.text.chars().count() as f32;
        if a.shown < len {
            a.shown = len;
            return;
        }
        if a.wait == Wait::Click {
            a.wait = Wait::None;
            a.fresh = true;
            self.stop_sound("voice", 0);
        }
    }

    /// Runs the story for a frame.
    pub fn run_story(&mut self) -> Result<(), String> {
        let now = self.now;
        let busy = self.busy();
        let a = self.adv.as_mut().unwrap();
        // the text types on
        let len = a.text.chars().count() as f32;
        if a.shown < len {
            a.shown = (a.shown + TYPE_SPEED).min(len);
        }
        if let Some((from, to, t0, dur)) = a.window_move {
            let t = if dur == 0 {
                1.0
            } else {
                ((now - t0) as f32 / dur as f32).min(1.0)
            };
            a.window = from + (to - from) * t;
            if t >= 1.0 {
                a.window_move = None;
            }
        }
        if let Some((_, t0, dur)) = &a.trans
            && now >= t0 + dur
        {
            a.trans = None;
        }
        let skip = match &self.skip_to {
            Some((n, l)) if *n == a.name && a.pc >= *l => {
                self.skip_to = None;
                if let Some((f, t)) = self.pending_vars.take() {
                    (self.flags, self.strs) = (f, t);
                }
                false
            }
            Some(_) => true,
            None => false,
        };
        if skip {
            a.trans = None;
            a.shown = a.text.chars().count() as f32;
            if a.wait != Wait::Select {
                a.wait = Wait::None;
            }
            let owner = a.owner;
            for o in self.objs.iter_mut().flatten().filter(|o| o.owner == owner) {
                finish(o);
            }
        }
        let a = self.adv.as_mut().unwrap();
        match a.wait {
            Wait::Until(t) if now < t => return Ok(()),
            Wait::All if busy => return Ok(()),
            Wait::Window if a.window_move.is_some() => return Ok(()),
            Wait::Click | Wait::Select => return Ok(()),
            Wait::Fes(r, var) => {
                if self.runs.get(r).is_some_and(|run| !run.done) {
                    return Ok(());
                }
                if let Some(run) = self.runs.get(r) {
                    let v = run.exit_code;
                    self.flags.insert(var, v);
                    self.end_children(r - 1);
                }
                self.adv.as_mut().unwrap().wait = Wait::None;
            }
            _ => a.wait = Wait::None,
        }
        for _ in 0..5000 {
            let Some(a) = self.adv.as_mut() else {
                return Ok(());
            };
            if a.wait != Wait::None {
                return Ok(());
            }
            if self
                .skip_to
                .as_ref()
                .is_some_and(|(n, l)| *n == a.name && a.pc >= *l)
            {
                self.skip_to = None;
                if let Some((f, t)) = self.pending_vars.take() {
                    (self.flags, self.strs) = (f, t);
                }
                if self.log > 0 {
                    eprintln!("skipped to {} line {}", a.name, a.pc);
                }
            }
            let Some(line) = a.scene.lines.get(a.pc).cloned() else {
                if let Some((scene, name, pc)) = a.calls.pop() {
                    (a.scene, a.name, a.pc) = (scene, name, pc);
                    continue;
                }
                if self.log > 0 {
                    eprintln!("scene {} ends", a.name);
                }
                self.end_story();
                return Ok(());
            };
            a.pc += 1;
            match line.kind {
                Line::Input => {
                    if self.skip_to.is_some() {
                        a.fresh = true;
                        a.shown = a.text.chars().count() as f32;
                        continue;
                    }
                    a.wait = Wait::Click;
                    return Ok(());
                }
                Line::Page => a.fresh = true,
                Line::Name => {
                    self.new_page();
                    // `$str20`: the player's name
                    let n = self.story_strings(&line.text);
                    let a = self.adv.as_mut().unwrap();
                    a.speaker = n.rsplit('＠').next().unwrap_or("").to_string();
                }
                Line::Message => {
                    self.new_page();
                    let t = clean(&self.story_strings(&line.text));
                    if self.log > 1 {
                        eprintln!("  text {t:?}");
                    }
                    let a = self.adv.as_mut().unwrap();
                    a.text.push_str(&t);
                    if a.window <= 0.0 && a.window_move.is_none() {
                        a.window = 1.0;
                    }
                }
                Line::Command => self.story_command(&line.text)?,
                Line::Other(_) => {}
            }
        }
        Ok(())
    }

    fn new_page(&mut self) {
        let a = self.adv.as_mut().unwrap();
        if a.fresh {
            a.fresh = false;
            if !a.text.is_empty() {
                let page = (std::mem::take(&mut a.speaker), a.text.clone());
                a.backlog.push(page);
                if a.backlog.len() > 200 {
                    a.backlog.remove(0);
                }
            }
            a.mark = Some((a.name.clone(), a.pc.saturating_sub(1)));
            a.text.clear();
            a.speaker.clear();
            a.shown = 0.0;
        }
    }

    fn set_wait(&mut self, w: Wait) {
        if self.skip_to.is_some() {
            return;
        }
        if let Some(a) = self.adv.as_mut() {
            a.wait = w;
        }
    }

    // ---- objects of the story

    fn plane(&mut self, n: i64) -> usize {
        if let Some(&i) = self.adv.as_ref().unwrap().planes.get(&n) {
            return i;
        }
        let owner = self.adv.as_ref().unwrap().owner;
        let mut o = Obj::new(owner, Kind::Plane);
        reset_plane(&mut o, n, self.width, self.height);
        let i = self.add_obj(o);
        self.adv.as_mut().unwrap().planes.insert(n, i);
        i
    }

    fn layer(&mut self, kind: &str, n: i64) -> usize {
        let key = format!("{kind}{n}");
        if let Some(&i) = self.adv.as_ref().unwrap().layers.get(&key) {
            return i;
        }
        let p = self.plane(if kind == "bg" { 0 } else { n });
        let owner = self.adv.as_ref().unwrap().owner;
        let mut o = Obj::new(owner, Kind::Image);
        o.plane = Some(p);
        o.anchor = true;
        o.pri = match kind {
            "bg" => 0,
            "cg" => 100,
            "fg" => 200,
            _ => 300,
        } + n as i32;
        o.pos = default_pos(kind);
        let i = self.add_obj(o);
        self.adv.as_mut().unwrap().layers.insert(key, i);
        i
    }

    fn live_story(&mut self) -> Vec<Draw> {
        let owner = self.adv.as_ref().unwrap().owner;
        let mut out = Vec::new();
        self.draw_objs(Some((true, owner)), &mut out);
        out
    }

    /// The story's pictures as the screen shows them.
    pub fn story_draws(&mut self) -> Vec<Draw> {
        let a = self.adv.as_ref().unwrap();
        let trans = a
            .trans
            .as_ref()
            .map(|(old, t0, dur)| (old.clone(), *t0, *dur));
        let mut live = self.live_story();
        match trans {
            Some((mut old, t0, dur)) => {
                let t = ((self.now.saturating_sub(t0)) as f32 / dur.max(1) as f32).min(1.0);
                for d in live.iter_mut() {
                    d.place.alpha *= t;
                }
                old.extend(live);
                old
            }
            None => live,
        }
    }

    /// A value argument: a number, an expression of flags (#n), or `@`, `@+20` for the
    /// current value.
    fn value(&mut self, s: &str, cur: f32) -> f32 {
        if s == "@" {
            return cur;
        }
        let s = s.replace('@', &format!("({})", cur as i64));
        crate::expr::eval(&s, &mut Flags(self)).unwrap_or(0) as f32
    }

    fn story_command(&mut self, line: &str) -> Result<(), String> {
        if self.log > 1 {
            eprintln!("  {line}");
        }
        if let Some((cond, rest)) = split_if(line) {
            if crate::expr::eval(cond, &mut Flags(self)).unwrap_or(0) != 0 && !rest.is_empty() {
                return self.story_command(rest);
            }
            return Ok(());
        }
        let line = self.story_strings(line);
        let a = split_args(&line);
        let Some(cmd) = a.first().map(String::as_str) else {
            return Ok(());
        };
        let arg = |k: usize| a.get(k).map_or("", String::as_str);
        let now = self.now;
        if cmd.starts_with('#') {
            // #1 = #2 = 0
            let parts: Vec<&str> = line.split('=').map(str::trim).collect();
            if parts.len() >= 2 {
                let v = crate::expr::eval(parts[parts.len() - 1], &mut Flags(self)).unwrap_or(0);
                for p in &parts[..parts.len() - 1] {
                    if let Ok(k) = p.trim_start_matches('#').parse::<i64>() {
                        self.flags.insert(k, v);
                    }
                }
            }
            return Ok(());
        }
        match cmd {
            "wait" => {
                let w = match arg(1) {
                    "" => Wait::All,
                    "mpl" => Wait::Window,
                    n => Wait::Until(now + n.parse::<u64>().unwrap_or(0)),
                };
                self.set_wait(w);
            }
            "call" => {
                // a scene as a subroutine (pictures of an event CG)
                let a = self.adv.as_mut().unwrap();
                if a.calls.len() < 16 {
                    a.calls.push((a.scene.clone(), a.name.clone(), a.pc));
                }
                let n = arg(1).to_string();
                if let Err(e) = self.load_scene(&n) {
                    eprintln!("{e}");
                    let a = self.adv.as_mut().unwrap();
                    if let Some((scene, name, pc)) = a.calls.pop() {
                        (a.scene, a.name, a.pc) = (scene, name, pc);
                    }
                }
            }
            "next" => {
                let n = arg(1).to_string();
                if let Err(e) = self.load_scene(&n) {
                    eprintln!("{e}");
                    self.end_story();
                }
            }
            "fselect" => {
                let a = self.adv.as_mut().unwrap();
                a.choices.clear();
                while let Some(l) = a.scene.lines.get(a.pc) {
                    if l.kind != Line::Command {
                        break;
                    }
                    let w: Vec<&str> = l.text.splitn(3, ' ').collect();
                    if w.len() < 3 || w[0].parse::<i64>().is_err() {
                        break;
                    }
                    a.choices.push((w[1].to_string(), w[2].to_string()));
                    a.pc += 1;
                }
                a.hover = None;
                a.wait = Wait::Select;
            }
            "str" => {
                let k: i64 = arg(1).parse().unwrap_or(0);
                self.strs.insert(k, a[2.min(a.len())..].join(" "));
            }
            "drawdef" => self.adv.as_mut().unwrap().drawdef = arg(1).parse().unwrap_or(15),
            "wipedef" => self.adv.as_mut().unwrap().wipedef = arg(1).parse().unwrap_or(30),
            "rdraw" | "rdraws1" | "rwipe" => {
                let a = self.adv.as_mut().unwrap();
                let n = if cmd == "rwipe" { arg(2) } else { arg(1) };
                let def = if cmd == "rwipe" { a.wipedef } else { a.drawdef };
                let dur = n.parse().unwrap_or(def);
                let old = std::mem::take(&mut a.prev);
                a.trans = (dur > 0).then_some((old, now, dur));
            }
            "fex" if arg(1) == "sel" => {
                // a screen script (a map to pick from); the story waits for its exit
                if let Err(e) = self.exec_fes(arg(2)) {
                    eprintln!("{e}");
                    return Ok(());
                }
                let r = self.runs.len() - 1;
                self.runs[r].story = true;
                let var = arg(3).parse().unwrap_or(-1);
                self.adv.as_mut().unwrap().wait = Wait::Fes(r, var);
            }
            "frameon" | "frameoff" => {
                let a = self.adv.as_mut().unwrap();
                let to = if cmd == "frameon" { 1.0 } else { 0.0 };
                let dur = if arg(1) == "fade" {
                    arg(2).parse().unwrap_or(0)
                } else {
                    0
                };
                a.window_move = Some((a.window, to, now, dur));
            }
            "view" => {
                if let Some(t) = a.get(2) {
                    let adv = self.adv.as_mut().unwrap();
                    adv.theme.1 = t.clone();
                    adv.win = None;
                }
            }
            "pcm" if self.skip_to.is_none() => {
                let n = arg(1).to_string();
                self.play("voice", &n, 1.0, false);
            }
            "bgm" | "se" => self.story_sound(cmd, &a),
            "pl" => self.plane_command(&a),
            _ if is_layer(cmd) => self.layer_command(cmd, &a),
            _ => {}
        }
        Ok(())
    }

    fn story_strings(&self, s: &str) -> String {
        if !s.contains("$str") {
            return s.to_string();
        }
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("$str") {
            out.push_str(&rest[..i]);
            let tail = &rest[i + 4..];
            let n = tail.bytes().take_while(u8::is_ascii_digit).count();
            let k: i64 = tail[..n].parse().unwrap_or(-1);
            out.push_str(self.strs.get(&k).map_or("", String::as_str));
            rest = &tail[n..];
        }
        out.push_str(rest);
        out
    }

    fn story_sound(&mut self, cmd: &str, a: &[String]) {
        let arg = |k: usize| a.get(k).map_or("", String::as_str);
        let Ok(n) = arg(1).parse::<i64>() else {
            if a.len() == 1 {
                for k in 0..8 {
                    self.stop_sound(&format!("{cmd}{k}"), 0);
                }
            }
            return;
        };
        let ch = format!("{cmd}{n}");
        let frames_ms = |f: &str| (f.parse::<f32>().unwrap_or(0.0) * 1000.0 / 60.0) as u32;
        match arg(2) {
            "" => self.stop_sound(&ch, 0),
            "end" | "stop" => self.stop_sound(&ch, frames_ms(arg(3))),
            "fade" => {
                if arg(5).parse::<f32>().unwrap_or(1.0) <= 0.0 {
                    self.stop_sound(&ch, frames_ms(arg(3)));
                }
            }
            "vol" | "pan" => {}
            "loop" => {
                let f = arg(3).to_string();
                self.play(&ch, &f, 1.0, true);
            }
            f => {
                let f = f.to_string();
                self.play(&ch, &f, 1.0, cmd == "bgm");
            }
        }
    }

    fn plane_command(&mut self, a: &[String]) {
        let arg = |k: usize| a.get(k).map_or("", String::as_str);
        let (w, h) = (self.width, self.height);
        if a.len() == 1 || matches!(arg(1), "stop" | "delay") {
            let planes: Vec<(i64, usize)> = self
                .adv
                .as_ref()
                .unwrap()
                .planes
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect();
            for (n, i) in planes {
                if let Some(o) = self.obj(i) {
                    if a.len() == 1 {
                        reset_plane(o, n, w, h);
                    } else {
                        finish(o);
                    }
                }
            }
            return;
        }
        let ns = indices(arg(1));
        if a.len() == 2 {
            for n in ns {
                let i = self.plane(n);
                reset_plane(self.obj(i).unwrap(), n, w, h);
            }
            return;
        }
        for n in ns {
            let i = self.plane(n);
            let sub = arg(2);
            if let Some(hex) = sub.strip_prefix('$') {
                let c = u32::from_str_radix(hex, 16).unwrap_or(0);
                let o = self.obj(i).unwrap();
                o.kind = Kind::Color;
                o.color = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                o.size = (w as f32, h as f32);
                o.pos = (0.0, 0.0);
                o.base = (0.0, 0.0);
                continue;
            }
            match sub {
                "clone" | "bmode" => {
                    if sub == "bmode" {
                        self.obj(i).unwrap().add = arg(3) == "1";
                    }
                }
                "quake" | "shake" | "mcquake" => {
                    // takes its time; the shaking itself is not drawn
                    let dur: u64 = if sub == "quake" {
                        a[3..]
                            .iter()
                            .filter_map(|s| s.split(',').next()?.parse::<u64>().ok())
                            .sum()
                    } else {
                        arg(3).parse().unwrap_or(0)
                    };
                    let now = self.now;
                    self.obj(i).unwrap().motions.push(Motion {
                        m: Move::Idle,
                        t0: now,
                        dur,
                        ease: 0,
                    });
                }
                _ => self.move_command(i, sub, &a[3..]),
            }
        }
    }

    /// The motions and settings that planes and pictures share.
    fn move_command(&mut self, i: usize, sub: &str, v: &[String]) {
        let Some(o) = self.obj(i).cloned() else {
            return;
        };
        let now = self.now;
        let s = |k: usize| v.get(k).map_or("0", String::as_str).to_string();
        let val = |e: &mut Engine, k: usize, cur: f32| e.value(&s(k), cur);
        let mut m: Option<(Move, u64, u8)> = None;
        let mut o2 = o.clone();
        match sub {
            "disp" => o2.disp = val(self, 0, 0.0) != 0.0,
            "pri" => o2.pri = val(self, 0, o.pri as f32) as i32,
            "blend" => o2.alpha = val(self, 0, o.alpha),
            "pos" => o2.pos = (val(self, 0, o.pos.0), val(self, 1, o.pos.1)),
            "scale" => {
                o2.scale = (
                    val(self, 0, o.scale.0 * 100.0) / 100.0,
                    val(self, 1, o.scale.1 * 100.0) / 100.0,
                )
            }
            "rot" => o2.rot = val(self, 2, o.rot),
            "stop" => finish(&mut o2),
            "fade" => {
                let (d, from, to) = (
                    val(self, 0, 0.0),
                    val(self, 1, o.alpha),
                    val(self, 2, o.alpha),
                );
                o2.alpha = from;
                m = Some((Move::Fade(from, to), d as u64, 0));
            }
            "arc" => {
                let d = val(self, 0, 0.0);
                m = Some((
                    Move::Arc {
                        a0: val(self, 1, 0.0),
                        a1: val(self, 2, 0.0),
                        r: (val(self, 3, 0.0), val(self, 4, 0.0)),
                        from: o.pos,
                    },
                    d as u64,
                    0,
                ));
            }
            "mscale" => {
                let d = val(self, 0, 0.0);
                let to = (
                    val(self, 1, o.scale.0 * 100.0) / 100.0,
                    val(self, 2, o.scale.1 * 100.0) / 100.0,
                );
                m = Some((Move::Scale(o.scale, to), d as u64, 0));
            }
            "mrot" => {
                let d = val(self, 0, 0.0);
                m = Some((Move::Rot(o.rot, val(self, 3, o.rot)), d as u64, 0));
            }
            _ if sub == "move"
                || sub.starts_with("amove")
                || sub.starts_with("m") && sub.ends_with("move") =>
            {
                let d = val(self, 0, 0.0);
                let to = (val(self, 1, o.pos.0), val(self, 2, o.pos.1));
                let ease = sub
                    .strip_prefix("amove")
                    .and_then(|k| k.parse().ok())
                    .unwrap_or(0);
                m = Some((Move::Pos(o.pos, to), d as u64, ease));
            }
            _ if sub.ends_with("fade") => {
                let (d, from, to) = (
                    val(self, 0, 0.0),
                    val(self, 1, o.alpha),
                    val(self, 2, o.alpha),
                );
                o2.alpha = from;
                m = Some((Move::Fade(from, to), d as u64, 0));
            }
            _ => {
                if self.log > 0 {
                    eprintln!("? {sub} {v:?}");
                }
            }
        }
        if let Some((mv, dur, ease)) = m {
            o2.motions.push(Motion {
                m: mv,
                t0: now,
                dur,
                ease,
            });
        }
        if let Some(slot) = self.obj(i) {
            *slot = o2;
        }
    }

    fn layer_command(&mut self, kind: &str, a: &[String]) {
        let arg = |k: usize| a.get(k).map_or("", String::as_str);
        if a.len() == 1 {
            let keys: Vec<usize> = self
                .adv
                .as_ref()
                .unwrap()
                .layers
                .iter()
                .filter(|(k, _)| k.starts_with(kind))
                .map(|(_, v)| *v)
                .collect();
            for i in keys {
                let o = self.obj(i).unwrap();
                o.disp = false;
                o.parts.clear();
                o.motions.clear();
            }
            return;
        }
        let ns = indices(arg(1));
        if ns.is_empty() {
            return; // "bg tone" and the like
        }
        if a.len() == 2 {
            for n in ns {
                let i = self.layer(kind, n);
                let o = self.obj(i).unwrap();
                o.disp = false;
                o.parts.clear();
                o.motions.clear();
            }
            return;
        }
        let sub = arg(2);
        const SUBS: [&str; 14] = [
            "fade", "blend", "pos", "move", "arc", "scale", "mscale", "rot", "mrot", "stop",
            "disp", "pri", "tone", "blur",
        ];
        let is_sub = SUBS.contains(&sub)
            || sub.starts_with("amove")
            || matches!(sub, "attr" | "attpos" | "mono" | "nega" | "sepia");
        // motions this does not play (`bg 0 quake ...`, `cg 1 spline ...`): not pictures
        let unplayed = !is_sub
            && (matches!(
                sub,
                "quake" | "shake" | "mcquake" | "spline" | "m2spline" | "mspline" | "wipe"
            ) || sub.starts_with("mc"))
            && !self.g.has(&format!("{sub}.hg3"));
        if unplayed {
            return;
        }
        for n in ns {
            let i = self.layer(kind, n);
            if is_sub {
                self.move_command(i, sub, &a[3..]);
                continue;
            }
            // a picture: name or name,part,part,...
            let mut parts: Vec<&str> = sub.split(',').collect();
            let base = parts.remove(0).to_string();
            let mut files = Vec::new();
            if parts.is_empty() {
                files.push(base.clone());
            } else {
                for (k, p) in parts.iter().enumerate() {
                    if *p == "0" || p.is_empty() {
                        continue;
                    }
                    let f = format!("{base}_{}{p}", "0".repeat(k));
                    if self.g.has(&format!("{f}.hg3")) {
                        files.push(f);
                    }
                }
            }
            // a new picture goes to the default place unless placed (`@` keeps the old one)
            let cur = self.objs[i].as_ref().unwrap().pos;
            let def = default_pos(kind);
            let rest = a[3..].to_vec();
            let x = rest.first().map_or(def.0, |s| self.value(s, cur.0));
            let y = rest.get(1).map_or(def.1, |s| self.value(s, cur.1));
            let o = self.obj(i).unwrap();
            o.parts = files;
            o.file = base;
            o.disp = true;
            o.alpha = 255.0;
            o.pos = (x, y);
            o.motions.clear();
        }
    }

    // ---- the message window

    fn theme(&mut self) -> Rc<Theme> {
        let a = self.adv.as_ref().unwrap();
        if let Some(t) = &a.win {
            return t.clone();
        }
        let (file, theme) = a.theme.clone();
        let xml = self.g.text(&file).unwrap_or_default();
        let sec = crate::game::xml_tag(&xml, &theme)
            .or_else(|| crate::game::xml_tag(&xml, "default"))
            .unwrap_or("");
        let mes = crate::game::xml_tag(sec, "mes").unwrap_or("");
        let img = |t: &str| {
            crate::game::xml_tag(mes, t)
                .unwrap_or("")
                .trim()
                .trim_end_matches(".hg3")
                .to_string()
        };
        // one picture may hold the window (frame 1) and its places, its frame 0 a mask
        let (bg, frame) = match img("img01") {
            f if f.is_empty() => (String::new(), img("img00")),
            f => (img("img00"), f),
        };
        // the older meswnd.xml (スズノネセブン！, 2008): <IMAGE><file>, its frame 1 the window,
        // 10 the text and 50 the click mark (its 100s are the buttons, not a name plate)
        let old = mes.is_empty().then(|| {
            let image = crate::game::xml_tag(&xml, "IMAGE").unwrap_or("");
            crate::game::xml_tag(image, "file")
                .unwrap_or("")
                .trim()
                .to_string()
        });
        let t = Rc::new(match old {
            Some(file) if !file.is_empty() => Theme {
                bg: String::new(),
                frame: file,
                names: String::new(),
                plate: false,
            },
            _ => Theme {
                bg,
                frame,
                names: img("img03"),
                plate: true,
            },
        });
        self.adv.as_mut().unwrap().win = Some(t.clone());
        t
    }

    pub fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(a) = self.adv.as_ref() else { return };
        let alpha = a.window;
        let waiting = a.wait == Wait::Click && a.shown >= a.text.chars().count() as f32;
        let (w, h) = (self.width as f32, self.height as f32);
        let full = [0.0, 0.0, w, h];
        let place = |alpha: f32| Place {
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
            clip: full,
        };
        let put = |out: &mut Vec<Draw>, img: Rc<Image>, x: f32, y: f32, alpha: f32| {
            let (iw, ih) = (img.w, img.h);
            out.push(Draw {
                image: img,
                src: [0, 0, iw as i32, ih as i32],
                dst: [x, y, x + iw as f32, y + ih as f32],
                place: place(alpha),
            });
        };
        if alpha > 0.0 {
            let theme = self.theme();
            let (bg, frame) = (&theme.bg, &theme.frame);
            let canvas = self.g.frame(frame, 1).or_else(|| self.g.frame(bg, 0));
            let top = canvas.as_ref().map_or(h - 200.0, |f| h - f.canvas_h as f32);
            for (file, id) in [(bg, 0), (frame, 1)] {
                if let Some(f) = self.g.frame(file, id) {
                    put(out, f.image.clone(), f.x as f32, top + f.y as f32, alpha);
                }
            }
            let rect = |e: &mut Engine, id: i64, d: [f32; 4]| {
                e.g.frame(frame, id).map_or(d, |f| {
                    [
                        f.x as f32,
                        top + f.y as f32,
                        f.image.w as f32,
                        f.image.h as f32,
                    ]
                })
            };
            let mut tr = rect(self, 10, [w * 0.2, h - 150.0, w * 0.7, 110.0]);
            let mut nr = match self.g.frame(frame, 100).filter(|_| theme.plate) {
                Some(_) => rect(self, 100, [0.0; 4]),
                None => rect(self, 11, [w * 0.2, h - 190.0, 200.0, 40.0]),
            };
            // the older window: its frame 10 covers it all; the text and the name plate go
            // where the window picture (frame 1) has them
            if !theme.plate
                && let Some(f) = self.g.frame(frame, 1)
            {
                let (x, y) = (f.x as f32, top + f.y as f32);
                let iw = f.image.w as f32;
                tr = [x + 70.0, y + 50.0, iw - 140.0, f.image.h as f32 - 60.0];
                nr = [x + 58.0, y, 245.0, 36.0];
            }
            let a = self.adv.as_ref().unwrap();
            let (text, shown, speaker) = (a.text.clone(), a.shown as usize, a.speaker.clone());
            let cached = a
                .text_img
                .as_ref()
                .filter(|(t, s, _)| *t == text && *s == shown)
                .map(|c| c.2.clone());
            let img = match cached {
                Some(i) => i,
                None => {
                    let i = Rc::new(self.font.render(
                        &text,
                        shown,
                        tr[2] as u32 + 40,
                        tr[3] as u32 + 12,
                        FONT,
                        PITCH,
                        [255, 255, 255],
                    ));
                    self.adv.as_mut().unwrap().text_img = Some((text, shown, i.clone()));
                    i
                }
            };
            put(out, img, tr[0] - 2.0, tr[1] - 2.0, alpha);
            // the game's picture of the name, or the name in the font
            let named = self
                .adv
                .as_ref()
                .unwrap()
                .name_ids
                .get(&speaker)
                .and_then(|n| self.g.frame(&theme.names, 1000 + n));
            if let Some(f) = named {
                put(
                    out,
                    f.image.clone(),
                    nr[0] + f.x as f32,
                    nr[1] + f.y as f32,
                    alpha,
                );
            } else if !speaker.is_empty() {
                let a = self.adv.as_ref().unwrap();
                let cached = a
                    .name_img
                    .as_ref()
                    .filter(|(t, _)| *t == speaker)
                    .map(|c| c.1.clone());
                let img = match cached {
                    Some(i) => i,
                    None => {
                        let i = Rc::new(self.font.render(
                            &speaker,
                            usize::MAX,
                            nr[2] as u32,
                            nr[3] as u32,
                            FONT - 2,
                            PITCH,
                            [255, 255, 255],
                        ));
                        self.adv.as_mut().unwrap().name_img = Some((speaker.clone(), i.clone()));
                        i
                    }
                };
                // centred in the name plate
                let tw = speaker
                    .chars()
                    .map(|c| self.font.advance(c, FONT - 2))
                    .sum::<i32>() as f32;
                let x = nr[0] + ((nr[2] - tw) / 2.0).max(0.0) - 2.0;
                put(
                    out,
                    img,
                    x,
                    nr[1] + (nr[3] - (FONT as f32 + 4.0)) / 2.0,
                    alpha,
                );
            }
            if waiting && let Some(f) = self.g.frame("sys_meswnd_wait", -1) {
                let at = rect(self, 50, [w - 60.0, h - 40.0, 30.0, 30.0]);
                put(out, f.image.clone(), at[0], at[1], alpha);
            }
        }
        self.choice_draws(out);
    }

    fn choice_draws(&mut self, out: &mut Vec<Draw>) {
        let a = self.adv.as_ref().unwrap();
        if a.wait != Wait::Select || a.choices.is_empty() {
            return;
        }
        let n = a.choices.len();
        let hover = a.hover;
        if a.choice_imgs.len() != n {
            let texts: Vec<String> = a.choices.iter().map(|c| c.1.clone()).collect();
            let imgs = texts
                .iter()
                .map(|t| {
                    Rc::new(self.font.render(
                        t,
                        usize::MAX,
                        900,
                        40,
                        FONT - 2,
                        PITCH,
                        [255, 255, 255],
                    ))
                })
                .collect();
            self.adv.as_mut().unwrap().choice_imgs = imgs;
        }
        let (w, h) = (self.width, self.height);
        let imgs = self.adv.as_ref().unwrap().choice_imgs.clone();
        for (k, img) in imgs.into_iter().enumerate() {
            let r = choice_rect(k, n, w, h);
            let lit = hover == Some(k);
            let c = if lit { [70, 110, 190] } else { [20, 30, 60] };
            let mut bg = Image::new(1, 1);
            bg.fill([c[0], c[1], c[2], 255]);
            let place = Place {
                x: 0.0,
                y: 0.0,
                sx: 1.0,
                sy: 1.0,
                cx: 0.0,
                cy: 0.0,
                rot: 0.0,
                alpha: 0.8,
                add: false,
                flip: (false, false),
                clip: [0.0, 0.0, w as f32, h as f32],
            };
            out.push(Draw {
                image: Rc::new(bg),
                src: [0, 0, 1, 1],
                dst: r,
                place,
            });
            let (iw, ih) = (img.w as f32, img.h as f32);
            let (x, y) = (r[0] + 20.0, r[1] + (r[3] - r[1] - ih) / 2.0 + 4.0);
            out.push(Draw {
                src: [0, 0, img.w as i32, img.h as i32],
                image: img,
                dst: [x, y, x + iw, y + ih],
                place: Place {
                    alpha: 1.0,
                    ..place
                },
            });
        }
    }
}

fn choice_rect(k: usize, n: usize, w: u32, h: u32) -> [f32; 4] {
    let (bw, bh, gap) = (w as f32 * 0.75, 56.0, 24.0);
    let total = n as f32 * bh + (n as f32 - 1.0) * gap;
    let top = (h as f32 * 0.42 - total / 2.0).max(10.0);
    let x = (w as f32 - bw) / 2.0;
    let y = top + k as f32 * (bh + gap);
    [x, y, x + bw, y + bh]
}

fn choice_at(n: usize, w: u32, h: u32, (mx, my): (i32, i32)) -> Option<usize> {
    (0..n).find(|&k| {
        let r = choice_rect(k, n, w, h);
        (mx as f32) >= r[0] && (mx as f32) < r[2] && (my as f32) >= r[1] && (my as f32) < r[3]
    })
}

/// The message window's pictures (`<mes>` of a theme in meswnd01.xml): img00 the window,
/// img01 the frame (its pictures 10, 50 and 100 mark the text, the click mark and the name),
/// img03 the names (1000 + their number in nametable.csv).
pub struct Theme {
    bg: String,
    frame: String,
    names: String,
    /// frame 100 of `frame` is the name plate
    plate: bool,
}

/// nametable.csv: `number<TAB>name<TAB>...`.
fn name_table(csv: &str) -> HashMap<String, i64> {
    csv.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let n = f.next()?.trim().parse().ok()?;
            Some((f.next()?.trim().to_string(), n))
        })
        .collect()
}

/// Where pictures of a layer go (config.int/vram.xml).
fn default_pos(kind: &str) -> (f32, f32) {
    if matches!(kind, "cg" | "fg") {
        (640.0, 280.0)
    } else {
        (0.0, 0.0)
    }
}

fn reset_plane(o: &mut Obj, n: i64, w: u32, h: u32) {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    o.kind = Kind::Plane;
    o.pos = (cx, cy);
    o.base = (cx, cy);
    o.scale = (1.0, 1.0);
    o.rot = 0.0;
    o.alpha = 255.0;
    o.add = false;
    o.disp = n < 6;
    o.pri = 5000 + 100 * n as i32;
    o.motions.clear();
}

fn finish(o: &mut Obj) {
    let ms = std::mem::take(&mut o.motions);
    for m in ms {
        crate::engine::apply(o, &m, 1.0);
    }
}

/// Flags (#n) for expressions of the story.
struct Flags<'a>(&'a Engine);

impl crate::expr::Env for Flags<'_> {
    fn get(&mut self, n: &str) -> i64 {
        let k = n
            .trim_start_matches(['#', '$'])
            .parse::<i64>()
            .unwrap_or(-1);
        self.0.flags.get(&k).copied().unwrap_or(0)
    }
}
