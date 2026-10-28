//! The game: the title (ui/720p/title/title.ui), the talk scene (ui/720p/talk/talk.ui) and the
//! scenario. The macros of scenario/define.ks (BG, FG, cpg, switch, ...) are run here directly;
//! their Squirrel bodies are not.
//!
//! The screen is the editor's 1280x720 design drawn at 1920x1080 (the pictures are 1080p).

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::assets::{Assets, Host};
use crate::eval::{self, Env};
use crate::gfx::{Draw, Img};
use crate::json;
use crate::ks::{self, Arg, Op};
use crate::obj::{self, Value};
use crate::text::{self, Font, Style};
use crate::ui::{Affine, Node};

pub const W: f32 = 1280.0;
pub const H: f32 = 720.0;
const SCALE: f32 = 1.5;
const FRAME_MS: f64 = 1000.0 / 60.0;
/// Characters a second while text is shown.
const CPS: f32 = 60.0;
const AUTO_MS: f64 = 2000.0;
const SLOTS: usize = 12;
const QUICK: &str = "quick.txt";
const SYSTEM: &str = "system.txt";

/// A sound to start or stop (the web side plays it).
pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub bgm: bool,
    pub voice: bool,
    /// Effects: channel 0; with `stop`, -1 stops them all.
    pub effect_channel: Option<i32>,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

/// A fade of one view's opacity.
struct Anim {
    id: String,
    from: f32,
    to: f32,
    start: f64,
    ms: f64,
    /// Removed once done.
    delete: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Until(f64),
    /// cpg: the text is being shown, then waits for a click.
    Page {
        nowait: bool,
        cn: bool,
        noswap: bool,
    },
    Choice,
    /// [s]: stops.
    Stop,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Title,
    Talk,
    /// The slots, saving or loading; over the title or the talk scene.
    Slots {
        save: bool,
        page: usize,
    },
    Log {
        top: usize,
    },
}

/// A clickable view of the last frame.
struct Hit {
    name: String,
    inv: Affine,
    size: [f32; 2],
}

/// What a save holds: the position at the start of the page, the stage then and the flags.
#[derive(Clone, Default, Debug)]
pub struct Snapshot {
    pub file: String,
    pub pc: usize,
    pub title: String,
    pub text: String,
    pub bg: Option<String>,
    /// position -> (.ui, method)
    pub fg: BTreeMap<String, (String, String)>,
    pub cutin: Option<(String, String)>,
    pub bgm: Option<String>,
    pub window: bool,
    pub f: BTreeMap<String, Value>,
}

type TextKey = (String, u32, u32, u32);

pub struct Engine {
    a: Assets,
    pub screen: (u32, u32),
    font: Font,
    imgs: HashMap<(String, u32), (Rc<Img>, u64)>,
    /// sprite sheet folder -> clip -> part of the sheet
    sheets: HashMap<String, HashMap<String, [f32; 4]>>,
    /// rendered labels by text, box and style, with the frame they were last drawn in
    texts: HashMap<TextKey, (Rc<Img>, u64)>,
    uis: HashMap<String, Option<Node>>,
    scripts: HashMap<String, Rc<ks::Script>>,
    frame: u64,
    now: f64,
    mode: Mode,
    title: Option<Node>,
    base: Node,
    file: String,
    pc: usize,
    stack: Vec<(String, usize)>,
    wait: Wait,
    anims: Vec<Anim>,
    /// Text gathered for the next cpg, and the page shown.
    pending: String,
    msg: String,
    shown: f32,
    name: String,
    window: bool,
    /// The choices: label, op after its [case]; and the op after [endswitch].
    choices: Vec<(String, usize)>,
    switch_end: usize,
    f: BTreeMap<String, Value>,
    sf: BTreeMap<String, Value>,
    /// f["FG_<pos>"]: the view of each position, with its .ui and method.
    fg: BTreeMap<String, (String, String, String)>,
    bg: Option<(String, String)>,
    cutin: Option<(String, String)>,
    bgm: Option<String>,
    voice: Option<String>,
    sounds: Vec<Sound>,
    page: Snapshot,
    label_title: String,
    hits: Vec<Hit>,
    hover: Option<String>,
    pointer: (f32, f32),
    skip: bool,
    auto: bool,
    auto_at: f64,
    /// The window hidden by a right click or its close button.
    hidden_by_user: bool,
    log: Vec<(String, String)>,
    ended: bool,
    chars: HashMap<String, String>,
    cgs: HashMap<i64, Vec<String>>,
    pub warnings: Vec<String>,
}

fn arg_str(op: &Op, k: &str) -> Option<String> {
    match op.arg(k)? {
        Arg::Str(s) => Some(s.clone()),
        Arg::Num(n) => Some(if n.fract() == 0.0 {
            format!("{}", *n as i64)
        } else {
            n.to_string()
        }),
        Arg::Bool(b) => Some(b.to_string()),
        Arg::Expr(e) => Some(e.clone()),
    }
}

impl Engine {
    pub fn new(host: Box<dyn Host>) -> Result<Engine, String> {
        let a = Assets::open(host)?;
        let mut e = Engine {
            a,
            screen: ((W * SCALE) as u32, (H * SCALE) as u32),
            font: Font::default(),
            imgs: HashMap::new(),
            sheets: HashMap::new(),
            texts: HashMap::new(),
            uis: HashMap::new(),
            scripts: HashMap::new(),
            frame: 0,
            now: 0.0,
            mode: Mode::Title,
            title: None,
            base: Node::default(),
            file: String::new(),
            pc: 0,
            stack: Vec::new(),
            wait: Wait::None,
            anims: Vec::new(),
            pending: String::new(),
            msg: String::new(),
            shown: 0.0,
            name: String::new(),
            window: true,
            choices: Vec::new(),
            switch_end: 0,
            f: BTreeMap::new(),
            sf: BTreeMap::new(),
            fg: BTreeMap::new(),
            bg: None,
            cutin: None,
            bgm: None,
            voice: None,
            sounds: Vec::new(),
            page: Snapshot::default(),
            label_title: String::new(),
            hits: Vec::new(),
            hover: None,
            pointer: (-1.0, -1.0),
            skip: false,
            auto: false,
            auto_at: 0.0,
            hidden_by_user: false,
            log: Vec::new(),
            ended: false,
            chars: HashMap::new(),
            cgs: HashMap::new(),
            warnings: Vec::new(),
        };
        e.load_conf();
        e.load_system();
        // The game's own message font, when it ships one fontdue reads.
        for name in [
            "SourceHanSans-Medium.ttc",
            "SourceHanSans-Medium.otf",
            "ZenKakuGothicAntique-Regular.ttf",
        ] {
            if e.font.ready() {
                break;
            }
            if let Some(p) = e.a.find(name, "", &["font"])
                && let Some(b) = e.a.read(&p)
            {
                let _ = e.font.set(&b);
            }
        }
        e.open_title();
        Ok(e)
    }

    /// A font for the text when the game's cannot be read.
    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        if self.font.ready() {
            return Ok(());
        }
        self.font.set(ttf)
    }

    pub fn title_name(&self) -> &str {
        &self.a.title
    }

    fn warn(&mut self, s: String) {
        if self.warnings.len() < 200 && !self.warnings.contains(&s) {
            eprintln!("sakanagl: {s}");
            self.warnings.push(s);
        }
    }

    // ---- configuration -------------------------------------------------------------------

    fn conf_tokens(&mut self, name: &str) -> Vec<obj::Tok> {
        let Some(p) = self.a.find(name, "", &["conf"]) else {
            return Vec::new();
        };
        let b = self.a.read(&p).unwrap_or_default();
        obj::tokens(&b)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| *t != obj::Tok::Newline)
            .collect()
    }

    fn load_conf(&mut self) {
        use obj::Tok;
        // character.conf: character = { "Makoto" = "慎"; ... }
        let t = self.conf_tokens("character.conf");
        for w in t.windows(3) {
            if let (Tok::Str(k), Tok::Punct(b'='), Tok::Str(v)) = (&w[0], &w[1], &w[2]) {
                self.chars.insert(k.clone(), v.clone());
            }
        }
        // cg.conf: cg[0] <- #CGData() { graphics[0] = "ev_02_0.ui"; ... }
        let t = self.conf_tokens("cg.conf");
        let mut cur = None;
        for (i, w) in t.windows(4).enumerate() {
            match (&w[0], &w[1], &w[2], &w[3]) {
                (Tok::Name(n), Tok::Punct(b'['), Tok::Int(id), Tok::Punct(b']')) if n == "cg" => {
                    cur = Some(*id)
                }
                (Tok::Name(n), Tok::Punct(b'['), Tok::Int(ix), Tok::Punct(b']'))
                    if n == "graphics" =>
                {
                    if let (Some(c), Some(Tok::Str(s))) = (cur, t.get(i + 5)) {
                        let l = self.cgs.entry(c).or_default();
                        let ix = *ix as usize;
                        if l.len() <= ix {
                            l.resize(ix + 1, String::new());
                        }
                        l[ix] = s.clone();
                    }
                }
                _ => {}
            }
        }
    }

    fn load_system(&mut self) {
        if let Some(b) = self.a.host.read_save(SYSTEM) {
            for line in String::from_utf8_lossy(&b).lines() {
                if let Some((k, v)) = line.split_once('=') {
                    self.sf.insert(k.to_string(), parse_value(v));
                }
            }
        }
    }

    fn save_system(&mut self) {
        let mut s = String::new();
        for (k, v) in &self.sf {
            if let Some(v) = show_value(v) {
                s.push_str(&format!("{k}={v}\n"));
            }
        }
        self.a.host.write_save(SYSTEM, s.as_bytes());
    }

    // ---- files ---------------------------------------------------------------------------

    /// A .ui by bare name or path, with its links resolved.
    fn ui(&mut self, name: &str) -> Option<Node> {
        let key = name.to_string();
        if let Some(n) = self.uis.get(&key) {
            return n.clone();
        }
        let path = self.a.find(
            name,
            "ui",
            &["img/720p", "ui/720p", "ui/common", "img", "ui"],
        );
        let mut node = path
            .and_then(|p| self.a.read(&p))
            .and_then(|b| obj::parse(&b).ok())
            .map(|v| Node::from_ui(&v));
        if let Some(n) = node.as_mut() {
            let mut load = |p: &str| -> Option<Node> {
                let path = self.a.resolve(p)?.to_string();
                let b = self.a.read(&path)?;
                Some(Node::from_ui(&obj::parse(&b).ok()?))
            };
            n.link(&mut load, 0);
        } else {
            self.warn(format!("no view {name}"));
        }
        self.uis.insert(key, node.clone());
        node
    }

    fn script(&mut self, name: &str) -> Option<Rc<ks::Script>> {
        let name = name.trim_start_matches('/');
        let file = name.rsplit('/').next().unwrap_or(name).to_string();
        if let Some(s) = self.scripts.get(&file) {
            return Some(s.clone());
        }
        let p = self.a.find(&file, "ks", &["scenario"])?;
        let b = self.a.read(&p)?;
        let s = Rc::new(ks::parse(&String::from_utf8_lossy(&b)));
        self.scripts.insert(file, s.clone());
        Some(s)
    }

    fn sound(&mut self, name: &str, dirs: &[&str]) -> Option<Vec<u8>> {
        let p = self
            .a
            .find(name, "ogg", dirs)
            .or_else(|| self.a.find(name, "wav", dirs))?;
        self.a.read(&p)
    }

    // ---- title ---------------------------------------------------------------------------

    fn open_title(&mut self) {
        self.mode = Mode::Title;
        self.title = self
            .ui("ui/720p/title/title.ui")
            .or_else(|| self.ui("title.ui"));
        if let Some(t) = self.title.as_mut() {
            // The opening animation's later pages and covers start hidden or are faded in by
            // title.scp; show the menu over the first page.
            for n in ["white", "titlepage1", "titlepage2", "ev_02", "ev_03"] {
                if let Some(v) = t.find_mut(n) {
                    v.visible = false;
                }
            }
        }
        if let Some(b) = self.sound("bg_sys", &["music"]) {
            self.play_bgm_data(b, "bg_sys");
        }
    }

    fn start_game(&mut self) {
        self.reset_talk();
        self.f.clear();
        if self.goto("start.ks", None).is_err() {
            let _ = self.goto("main.ks", Some("START"));
        }
        self.page = self.snapshot();
        self.mode = Mode::Talk;
    }

    fn reset_talk(&mut self) {
        self.base = self
            .ui("ui/720p/talk/talk.ui")
            .or_else(|| self.ui("talk.ui"))
            .unwrap_or_default();
        self.stack.clear();
        self.wait = Wait::None;
        self.anims.clear();
        self.pending.clear();
        self.msg.clear();
        self.shown = 0.0;
        self.name.clear();
        self.window = true;
        self.choices.clear();
        self.fg.clear();
        self.bg = None;
        self.cutin = None;
        self.skip = false;
        self.auto = false;
        self.set_window(true);
        self.stop_bgm(0);
        self.sounds.push(Sound::stop_effects());
    }

    // ---- the stage -----------------------------------------------------------------------

    fn container(&mut self, path: &str) -> Option<&mut Node> {
        self.base.path_mut(path)
    }

    /// Adds view `node` as `id` (`IMG.BG.name`: the container, then the name).
    fn create(&mut self, id: &str, mut node: Node, order: Option<usize>) {
        let (parent, name) = id.rsplit_once('.').unwrap_or(("", id));
        node.name = name.to_string();
        self.anims.retain(|a| a.id != id);
        let Some(c) = self.container(parent) else {
            self.warn(format!("no container {parent}"));
            return;
        };
        c.children.retain(|n| n.name != name);
        let at = order.map_or(c.children.len(), |o| o.min(c.children.len()));
        c.children.insert(at, node);
    }

    fn obj_mut(&mut self, id: &str) -> Option<&mut Node> {
        let (parent, name) = id.rsplit_once('.')?;
        self.base
            .path_mut(parent)?
            .children
            .iter_mut()
            .find(|n| n.name == name)
    }

    fn delete(&mut self, id: &str) {
        self.anims.retain(|a| a.id != id);
        if let Some((parent, name)) = id.rsplit_once('.')
            && let Some(c) = self.base.path_mut(parent)
        {
            c.children.retain(|n| n.name != name);
        }
    }

    /// Fades `id` from its opacity to `to` over `ms`; then removes it when `delete`.
    fn fade(&mut self, id: &str, to: f32, ms: f64, delay: f64, delete: bool) {
        let from = self.obj_mut(id).map_or(0.0, |n| n.alpha);
        self.anims.retain(|a| a.id != id);
        if ms <= 1.0 && delay <= 0.0 || self.skip {
            if delete {
                self.delete(id);
            } else if let Some(n) = self.obj_mut(id) {
                n.alpha = to;
            }
            return;
        }
        self.anims.push(Anim {
            id: id.to_string(),
            from,
            to,
            start: self.now + delay,
            ms,
            delete,
        });
    }

    fn run_anims(&mut self) {
        let now = self.now;
        let mut done = Vec::new();
        let anims = std::mem::take(&mut self.anims);
        for a in &anims {
            let t = ((now - a.start) / a.ms).clamp(0.0, 1.0) as f32;
            let v = a.from + (a.to - a.from) * t;
            if let Some(n) = self.obj_mut(&a.id) {
                n.alpha = v;
            }
            if t >= 1.0 {
                done.push((a.id.clone(), a.delete));
            }
        }
        self.anims = anims;
        self.anims.retain(|a| now < a.start + a.ms);
        for (id, delete) in done {
            if delete {
                self.delete(&id);
            }
        }
    }

    fn set_window(&mut self, on: bool) {
        self.window = on;
        for p in ["GROUP.MSGWIN", "GROUP.SEL"] {
            if let Some(n) = self.base.path_mut(p) {
                n.visible = on;
            }
        }
    }

    /// Old view `old` out and `id` (`ui`) in, as the BG and FG macros do.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        id: &str,
        ui: &str,
        old: Option<String>,
        time: f64,
        delay: f64,
        order: Option<usize>,
        pos: [f32; 2],
    ) {
        let (outdelay, outtime, intime, indelay) = if delay > 0.0 {
            let od = 0.5 * time * delay;
            (od, time - od, time - od, 0.0)
        } else {
            let id_ = 0.5 * time * delay.abs();
            (0.0, time - id_, time - id_, id_)
        };
        let same = old.as_deref() == Some(id);
        if let Some(o) = old.filter(|o| o != id) {
            if outtime > 0.0 || outdelay > 0.0 {
                self.fade(&o, 0.0, outtime, outdelay, true);
            } else {
                self.delete(&o);
            }
        }
        let Some(mut node) = self.ui(ui) else {
            return;
        };
        node.pos = pos;
        let fadein = !same && (intime > 0.0 || indelay > 0.0) && !self.skip;
        node.alpha = if fadein { 0.0 } else { 1.0 };
        if same {
            // the same view again: keep it where it is in the order
            if let Some(n) = self.obj_mut(id) {
                *n = Node {
                    name: n.name.clone(),
                    ..node
                };
                return;
            }
        }
        self.create(id, node, order);
        if fadein {
            self.fade(id, 1.0, intime, indelay, false);
        }
    }

    fn file_name(p: &str) -> String {
        let f = p.rsplit('/').next().unwrap_or(p);
        f.rsplit_once('.').map_or(f, |(a, _)| a).to_string()
    }

    fn show_bg(&mut self, ui: &str, time: f64, delay: f64) {
        let id = format!("IMG.BG.{}", Self::file_name(ui));
        let old = self.bg.as_ref().map(|b| b.0.clone());
        self.place(&id, ui, old, time, delay, Some(0), [0.0, 0.0]);
        self.bg = Some((id, ui.to_string()));
    }

    fn calc_xrate(pos: &str) -> f32 {
        let mut it = pos.split('/');
        let (Some(a), Some(b)) = (it.next(), it.next()) else {
            return 0.0;
        };
        let (n, t) = (
            a.trim().parse::<f32>().unwrap_or(0.0),
            b.trim().parse::<f32>().unwrap_or(0.0),
        );
        if n != 0.0 && t > 0.0 {
            let x = (n.abs() - 0.5) / t;
            if n < 0.0 { -x } else { x }
        } else {
            0.0
        }
    }

    fn show_fg(&mut self, ui: &str, method: &str, pos: &str, time: f64, delay: f64, layer: usize) {
        let id = format!("GROUP.IMG.FG.{}", Self::file_name(ui));
        let old = self.fg.get(pos).map(|f| f.0.clone());
        // another position showing the same view loses it
        let dup: Vec<String> = self
            .fg
            .iter()
            .filter(|(p, f)| f.0 == id && *p != pos)
            .map(|(p, _)| p.clone())
            .collect();
        for p in dup {
            self.fg.remove(&p);
        }
        let x = Self::calc_xrate(pos) * W;
        self.place(&id, ui, old, time, delay, Some(layer), [x, 0.0]);
        self.method(&id, method);
        self.fg
            .insert(pos.to_string(), (id, ui.to_string(), method.to_string()));
    }

    /// A view's script method, as far as it shows: `change_N` and the like show face N; the
    /// others make the main group visible (`invisible` hides the emotion).
    fn method(&mut self, id: &str, method: &str) {
        let Some(n) = self.obj_mut(id) else {
            return;
        };
        let face = method
            .rsplit_once('_')
            .and_then(|(_, d)| d.parse::<u32>().ok().map(|_| d.to_string()));
        if let Some(face) = face
            && let Some(faces) = n.find_mut("FACE")
            && faces.children.iter().any(|c| c.name == face)
        {
            for c in &mut faces.children {
                c.visible = c.name == face;
            }
        }
        let show = method != "invisible";
        for g in ["GROUP", "EM"] {
            if let Some(v) = n.find_mut(g) {
                v.visible = show || g == "GROUP";
            }
        }
    }

    fn free(&mut self, time: f64) {
        let ids: Vec<String> = self.fg.values().map(|f| f.0.clone()).collect();
        self.fg.clear();
        for id in ids {
            self.fade(&id, 0.0, time, 0.0, true);
        }
    }

    fn transition(&mut self, show: bool, color: u32, time: f64) {
        let id = "IMG.FADE.TRANS";
        if show {
            let node = Node {
                size: [W, H],
                fill: Some(color | 0xff),
                alpha: 0.0,
                ..Node::default()
            };
            self.create(id, node, None);
            self.fade(id, 1.0, time, 0.0, false);
        } else {
            if let Some(n) = self.obj_mut(id) {
                n.fill = Some(color | 0xff);
            }
            self.fade(id, 0.0, time, 0.0, true);
        }
    }

    // ---- sound ---------------------------------------------------------------------------

    fn play_bgm_data(&mut self, data: Vec<u8>, name: &str) {
        self.bgm = Some(name.to_string());
        self.sounds.push(Sound {
            data,
            volume: 0.6,
            bgm: true,
            voice: false,
            effect_channel: None,
            stop: false,
            fade_ms: 0,
            looped: true,
        });
    }

    fn play_bgm(&mut self, name: &str) {
        if self.bgm.as_deref() == Some(name) {
            return;
        }
        match self.sound(name, &["music"]) {
            Some(d) => self.play_bgm_data(d, name),
            None => self.warn(format!("no bgm {name}")),
        }
    }

    fn stop_bgm(&mut self, fade: u32) {
        self.bgm = None;
        self.sounds.push(Sound {
            bgm: true,
            stop: true,
            fade_ms: fade,
            ..Sound::none()
        });
    }

    fn play_se(&mut self, name: &str, looped: bool) {
        if self.skip && !looped {
            return;
        }
        match self.sound(name, &["snd"]) {
            Some(data) => self.sounds.push(Sound {
                data,
                volume: 0.8,
                effect_channel: Some(0),
                looped,
                ..Sound::none()
            }),
            None => self.warn(format!("no sound {name}")),
        }
    }

    fn play_voice(&mut self, name: &str) {
        self.voice = Some(name.to_string());
        if self.skip {
            return;
        }
        match self.sound(name, &["snd/voice1", "snd"]) {
            Some(data) => self.sounds.push(Sound {
                data,
                volume: 1.0,
                voice: true,
                ..Sound::none()
            }),
            None => self.warn(format!("no voice {name}")),
        }
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    // ---- the scenario --------------------------------------------------------------------

    fn goto(&mut self, file: &str, label: Option<&str>) -> Result<(), String> {
        let file = if file.is_empty() {
            self.file.clone()
        } else {
            file.to_string()
        };
        let s = self
            .script(&file)
            .ok_or_else(|| format!("no scenario {file}"))?;
        let pc = match label {
            Some(l) if !l.is_empty() => {
                s.label(l).ok_or_else(|| format!("{file}: no label {l}"))?
            }
            _ => 0,
        };
        self.file = file.rsplit('/').next().unwrap_or(&file).to_string();
        self.pc = pc;
        Ok(())
    }

    fn eval(&self, a: &Arg) -> Value {
        match a {
            Arg::Str(s) => Value::Str(s.clone()),
            Arg::Num(n) => Value::Float(*n),
            Arg::Bool(b) => Value::Bool(*b),
            Arg::Expr(e) => eval::eval(
                e,
                &Env {
                    f: &self.f,
                    sf: &self.sf,
                },
            ),
        }
    }

    fn num(&self, op: &Op, k: &str, def: f64) -> f64 {
        op.arg(k)
            .map(|a| self.eval(a))
            .and_then(|v| v.num())
            .unwrap_or(def)
    }

    fn truth(&self, op: &Op, k: &str) -> bool {
        op.arg(k).is_some_and(|a| self.eval(a).truthy())
    }

    /// From `pc` (just after an [if] or [else]), the op after the matching [else]/[elsif] or
    /// [endif] (`to_end`: only [endif]); returns its index and the tag found.
    fn skip_if(&self, s: &ks::Script, mut pc: usize, to_end: bool) -> (usize, String) {
        let mut depth = 0;
        while pc < s.ops.len() {
            if let Op::Tag { name, .. } = &s.ops[pc] {
                match name.as_str() {
                    "if" => depth += 1,
                    "endif" if depth == 0 => return (pc + 1, name.clone()),
                    "endif" => depth -= 1,
                    "else" | "elsif" if depth == 0 && !to_end => return (pc + 1, name.clone()),
                    _ => {}
                }
            }
            pc += 1;
        }
        (pc, String::new())
    }

    fn run(&mut self) {
        let mut budget = 20000;
        while self.wait == Wait::None && self.mode == Mode::Talk && budget > 0 {
            budget -= 1;
            let Some(s) = self.script(&self.file.clone()) else {
                self.end_game();
                return;
            };
            let Some(op) = s.ops.get(self.pc).cloned() else {
                // The end of a file returns to its caller, or the game ends.
                if let Some((f, pc)) = self.stack.pop() {
                    self.file = f;
                    self.pc = pc;
                    continue;
                }
                self.end_game();
                return;
            };
            self.pc += 1;
            match op {
                Op::Label { title, .. } => {
                    if !title.is_empty() {
                        self.label_title = title;
                    }
                }
                Op::Text(t) => self.pending.push_str(&t),
                Op::Script(_) => {}
                Op::Tag { ref name, .. } => {
                    let name = name.clone();
                    if let Err(e) = self.tag(&name, &op, &s) {
                        self.warn(e);
                    }
                }
            }
        }
    }

    fn end_game(&mut self) {
        self.stop_bgm(1000);
        self.reset_talk_sounds();
        self.open_title();
    }

    fn reset_talk_sounds(&mut self) {
        self.sounds.push(Sound::stop_effects());
        self.sounds.push(Sound {
            voice: true,
            stop: true,
            ..Sound::none()
        });
    }

    fn tag(&mut self, name: &str, op: &Op, s: &ks::Script) -> Result<(), String> {
        let st = |k: &str| arg_str(op, k);
        match name {
            "BG" => {
                let f = st("f").ok_or("BG without f")?;
                self.show_bg(&f, self.num(op, "time", 0.0), self.num(op, "delay", 0.0));
            }
            "CG" => {
                let id = self.num(op, "id", 0.0) as i64;
                let ix = self.num(op, "index", 0.0) as usize;
                let ui = self
                    .cgs
                    .get(&id)
                    .and_then(|l| l.get(ix))
                    .cloned()
                    .ok_or(format!("no CG {id}/{ix}"))?;
                self.show_bg(&ui, self.num(op, "time", 0.0), self.num(op, "delay", 0.0));
            }
            "FG" => {
                let f = st("f").ok_or("FG without f")?;
                let pos = st("pos").unwrap_or_else(|| "1/1".into());
                let method = st("method").unwrap_or_default();
                let layer = self.num(op, "layer", 0.0) as usize;
                self.show_fg(
                    &f,
                    &method,
                    &pos,
                    self.num(op, "time", 0.0),
                    self.num(op, "delay", 0.0),
                    layer,
                );
            }
            "FO" => {
                let pos = st("pos").unwrap_or_else(|| "1/1".into());
                if let Some((id, ..)) = self.fg.remove(&pos) {
                    let t = self.num(op, "time", 0.0);
                    self.fade(&id, 0.0, t, 0.0, true);
                }
            }
            "free" => self.free(self.num(op, "time", 0.0)),
            "FACE" | "FACEDELAY" => {
                let pos = st("pos").unwrap_or_else(|| "1/1".into());
                let method = st("method").unwrap_or_default();
                if let Some(f) = self.fg.get_mut(&pos) {
                    f.2 = method.clone();
                    let id = f.0.clone();
                    self.method(&id, &method);
                }
            }
            "move" => {
                let (Some(from), Some(to)) = (st("from"), st("to")) else {
                    return Ok(());
                };
                if self.fg.contains_key(&to) {
                    return Ok(());
                }
                if let Some(f) = self.fg.remove(&from) {
                    let x = Self::calc_xrate(&to) * W;
                    if let Some(n) = self.obj_mut(&f.0) {
                        n.pos[0] = x;
                    }
                    self.fg.insert(to, f);
                }
            }
            "CUTIN" => {
                let f = st("f").ok_or("CUTIN without f")?;
                let id = format!("GROUP.IMG.CUTIN.{}", Self::file_name(&f));
                if self.truth(op, "show") {
                    if let Some(n) = self.ui(&f) {
                        self.create(&id, n, Some(0));
                        let m = st("method").unwrap_or_default();
                        self.method(&id, &m);
                        self.cutin = Some((f, m));
                    }
                } else {
                    self.delete(&id);
                    self.cutin = None;
                }
            }
            "transition" | "colorfade" => {
                let show = self.truth(op, "target");
                let color = op
                    .arg("color")
                    .map(|a| self.eval(a))
                    .and_then(|v| v.num())
                    .unwrap_or(255.0) as i64 as u32;
                self.transition(show, color, self.num(op, "time", 1000.0));
            }
            "BGM" | "playbgm" => {
                let f = st("f").or_else(|| st("storage")).ok_or("BGM without f")?;
                self.play_bgm(&f);
            }
            "STOPBGM" | "stopbgm" => self.stop_bgm(0),
            "FADEOUTBGM" | "fadeoutbgm" => self.stop_bgm(1000),
            "SE" | "playse" => {
                let f = st("f").or_else(|| st("storage")).ok_or("SE without f")?;
                self.play_se(&f, self.truth(op, "loop"));
            }
            "stopse" => self.sounds.push(Sound::stop_effects()),
            "VOICE" => {
                let f = st("f").ok_or("VOICE without f")?;
                self.play_voice(&f);
            }
            "MOVIE" | "ENDMOVIE" | "playmovie" | "closemovie" => {}
            "OnName" => {
                let n = st("num").unwrap_or_default();
                self.name = self.chars.get(&n).cloned().unwrap_or(n);
            }
            "cpg" => {
                self.msg = std::mem::take(&mut self.pending);
                self.shown = if self.skip { f32::MAX } else { 0.0 };
                if !self.msg.is_empty() {
                    self.log.push((self.name.clone(), self.msg.clone()));
                    if self.log.len() > 300 {
                        self.log.remove(0);
                    }
                }
                self.auto_at = self.now + AUTO_MS + self.msg.chars().count() as f64 * 50.0;
                self.wait = Wait::Page {
                    nowait: self.truth(op, "nowait"),
                    cn: self.truth(op, "cn"),
                    noswap: self.truth(op, "noswap"),
                };
            }
            "swap" | "ct" => {
                self.msg.clear();
                self.pending.clear();
            }
            "switch" => {
                self.choices.clear();
                let mut pc = self.pc;
                while let Some(o) = s.ops.get(pc) {
                    if let Op::Tag { name, vargs, .. } = o {
                        match name.as_str() {
                            "endswitch" => break,
                            "case" => {
                                let label = match vargs.first() {
                                    Some(Arg::Str(t)) => t.clone(),
                                    Some(a) => show_value(&self.eval(a)).unwrap_or_default(),
                                    None => String::new(),
                                };
                                self.choices.push((label, pc + 1));
                            }
                            _ => {}
                        }
                    }
                    pc += 1;
                }
                self.switch_end = pc + 1;
                if !self.choices.is_empty() {
                    self.show_choices();
                    self.skip = false;
                    self.wait = Wait::Choice;
                }
            }
            // Falling into the next case leaves the switch.
            "case" => self.pc = self.switch_end,
            "endswitch" => {}
            "window" => {
                let on = self.truth(op, "target");
                self.set_window(on);
            }
            "WAIT" | "wait" => {
                let t = self.num(op, "time", 1000.0);
                if !self.skip {
                    self.wait = Wait::Until(self.now + t);
                }
            }
            "stflag" => {
                if let Op::Tag { args, .. } = op {
                    for (k, a) in args {
                        let v = self.eval(a);
                        let cur = self.f.get(k).cloned();
                        let nv = match (cur.as_ref().and_then(Value::num), v.num()) {
                            (Some(c), Some(n)) if cur.as_ref().is_some_and(Value::truthy) => {
                                Value::Float(c + n)
                            }
                            _ => v,
                        };
                        self.f.insert(k.clone(), nv);
                    }
                }
            }
            "system" => {
                if let Op::Tag { args, .. } = op {
                    for (k, a) in args {
                        let v = self.eval(a);
                        self.sf.insert(k.clone(), v);
                    }
                }
                self.save_system();
            }
            "SCENE" => {
                let id = st("id").unwrap_or_default();
                self.sf.insert(format!("SCENE_{id}"), Value::Bool(true));
                self.save_system();
            }
            "sysload" | "ENDPARTIAL" | "baseview" | "scriptdirs" | "imagedirs" | "current" => {}
            "jump" => {
                if op.arg("cond").is_some() && !self.truth(op, "cond") {
                    return Ok(());
                }
                let file = st("storage").unwrap_or_default();
                let target = st("target");
                self.goto(&file, target.as_deref())?;
            }
            "call" => {
                let file = st("storage").unwrap_or_default();
                if file.contains("define.ks") {
                    return Ok(());
                }
                let ret = (self.file.clone(), self.pc);
                self.goto(&file, st("target").as_deref())?;
                self.stack.push(ret);
            }
            "return" => {
                if let Some((f, pc)) = self.stack.pop() {
                    self.file = f;
                    self.pc = pc;
                }
            }
            "if" | "elsif" => {
                if name == "elsif" {
                    // reached after a branch ran
                    self.pc = self.skip_if(s, self.pc, true).0;
                    return Ok(());
                }
                if !self.truth(op, "exp") {
                    let (mut pc, mut found) = self.skip_if(s, self.pc, false);
                    // an [elsif] whose condition holds runs; otherwise on to the next
                    while found == "elsif" {
                        let cond = s.ops.get(pc - 1).is_some_and(|o| self.truth(o, "exp"));
                        if cond {
                            break;
                        }
                        (pc, found) = self.skip_if(s, pc, false);
                    }
                    self.pc = pc;
                }
            }
            "else" => self.pc = self.skip_if(s, self.pc, true).0,
            "endif" => {}
            "s" => self.wait = Wait::Stop,
            "macro" => {
                // skip to the [endmacro]
                while let Some(o) = s.ops.get(self.pc) {
                    self.pc += 1;
                    if matches!(o, Op::Tag { name, .. } if name == "endmacro") {
                        break;
                    }
                }
            }
            _ => self.warn(format!("{}: [{name}] not handled", self.file)),
        }
        Ok(())
    }

    fn show_choices(&mut self) {
        let n = self.choices.len();
        let ui = format!("ui/720p/talk/selwindow_{n}.ui");
        let Some(mut node) = self
            .ui(&ui)
            .or_else(|| self.ui(&format!("selwindow_{n}.ui")))
        else {
            return;
        };
        for (i, (label, _)) in self.choices.iter().enumerate() {
            if let Some(c) = node.find_mut(&format!("case{i}"))
                && let Some(l) = c.find_mut("LABEL")
                && let Some(t) = l.label.as_mut()
            {
                t.text = label.clone();
            }
        }
        node.pos = [0.5 * W, 0.33 * H];
        self.create("GROUP.SEL.WINDOW", node, None);
    }

    fn choose(&mut self, i: usize) {
        let Some(&(_, pc)) = self.choices.get(i) else {
            return;
        };
        self.delete("GROUP.SEL.WINDOW");
        self.choices.clear();
        self.pc = pc;
        self.wait = Wait::None;
    }

    /// The page ends: the text stays until the next one; the position is kept for saves.
    fn end_page(&mut self, cn: bool, noswap: bool) {
        if !noswap {
            // the text stays shown (MSG_BK) until the next page
        }
        if cn {
            self.name.clear();
        }
        self.wait = Wait::None;
        self.page = self.snapshot();
    }

    // ---- frames and input ----------------------------------------------------------------

    pub fn frame(&mut self) -> Result<(), String> {
        self.frame += 1;
        self.now += FRAME_MS;
        self.run_anims();
        if self.mode == Mode::Talk {
            match self.wait {
                Wait::Until(t) if self.now >= t || self.skip => self.wait = Wait::None,
                Wait::Page { nowait, cn, noswap } => {
                    let total = self.msg.chars().count() as f32;
                    if self.shown < total {
                        self.shown += CPS / 60.0;
                    }
                    let done = self.shown >= total;
                    if done && (nowait || self.skip || (self.auto && self.now >= self.auto_at)) {
                        self.end_page(cn, noswap);
                    }
                }
                _ => {}
            }
            self.run();
        }
        // pictures not drawn for a while give their memory back
        if self.frame.is_multiple_of(300) {
            let f = self.frame;
            self.imgs.retain(|_, v| f - v.1 < 600);
            self.texts.retain(|_, v| f - v.1 < 120);
        }
        Ok(())
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The talk scene's views, one a line (for the native tools).
    pub fn dump(&self) -> String {
        fn walk(n: &Node, depth: usize, out: &mut String) {
            out.push_str(&format!(
                "{}{} k{} vis{} a{:.2} c{:08x} b{:08x} pos{:?} size{:?} piv{:?} sc{:?} img{:?}\n",
                "  ".repeat(depth),
                n.name,
                n.kind,
                n.visible as u8,
                n.alpha,
                n.color,
                n.bufcolor,
                n.pos,
                n.size,
                n.pivot,
                n.scale,
                n.image
            ));
            for c in &n.children {
                walk(c, depth + 1, out);
            }
        }
        let mut s = format!(
            "name={:?} chars={} msg={:?} wait={:?} file={} pc={}\n",
            self.name,
            self.chars.len(),
            self.msg,
            self.wait,
            self.file,
            self.pc
        );
        for h in &self.hits {
            let [x, y] = h
                .inv
                .invert()
                .map_or([-1.0, -1.0], |m| m.apply(h.size[0] / 2.0, h.size[1] / 2.0));
            s.push_str(&format!("hit {} at {:.0};{:.0}\n", h.name, x, y));
        }
        walk(
            if self.mode == Mode::Title {
                self.title.as_ref().unwrap_or(&self.base)
            } else {
                &self.base
            },
            0,
            &mut s,
        );
        s
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x as f32, y as f32);
        self.hover = self.hit(x as f32, y as f32);
    }

    fn hit(&self, x: f32, y: f32) -> Option<String> {
        self.hits.iter().rev().find_map(|h| {
            let [u, v] = h.inv.apply(x, y);
            (u >= 0.0 && v >= 0.0 && u < h.size[0] && v < h.size[1]).then(|| h.name.clone())
        })
    }

    /// A click (x, y on the screen; -1, -1 from the keyboard).
    pub fn click(&mut self, x: i32, y: i32) {
        let hit = if x < 0 {
            None
        } else {
            self.hit(x as f32, y as f32)
        };
        match self.mode {
            Mode::Title => match hit.as_deref() {
                Some("btn_start") => self.start_game(),
                Some("btn_continue") => {
                    if let Some(s) = self.latest_save() {
                        self.load_from(&s);
                    }
                }
                Some("btn_load") => {
                    self.mode = Mode::Slots {
                        save: false,
                        page: 0,
                    }
                }
                _ => {}
            },
            Mode::Slots { save, .. } => match hit.as_deref().and_then(|h| h.strip_prefix("slot")) {
                Some(n) => {
                    let n: usize = n.parse().unwrap_or(0);
                    let file = format!("s{n:02}.txt");
                    if save {
                        self.save_as(&file);
                        self.close_menu();
                    } else if self.a.host.read_save(&file).is_some() {
                        self.load_from(&file);
                    }
                }
                None => {
                    if hit.as_deref() == Some("back") {
                        self.close_menu();
                    }
                }
            },
            Mode::Log { .. } => self.mode = Mode::Talk,
            Mode::Talk => self.talk_click(hit),
        }
    }

    fn close_menu(&mut self) {
        self.mode = if self.file.is_empty() || self.base.children.is_empty() {
            Mode::Title
        } else {
            Mode::Talk
        };
        if self.mode == Mode::Title && self.title.is_none() {
            self.open_title();
        }
    }

    fn talk_click(&mut self, hit: Option<String>) {
        if !self.window
            && self.wait != Wait::Choice
            && matches!(self.wait, Wait::Page { .. })
            && hit.is_none()
        {
            // a hidden window comes back first
        }
        if let Some(h) = hit.as_deref() {
            if let Some(i) = h.strip_prefix("case").and_then(|n| n.parse::<usize>().ok()) {
                if self.wait == Wait::Choice {
                    self.choose(i);
                }
                return;
            }
            match h {
                "btn_save" => {
                    self.mode = Mode::Slots {
                        save: true,
                        page: 0,
                    };
                    return;
                }
                "btn_load" => {
                    self.mode = Mode::Slots {
                        save: false,
                        page: 0,
                    };
                    return;
                }
                "btn_quicksave" => {
                    self.save_as(QUICK);
                    return;
                }
                "btn_quickload" => {
                    if self.a.host.read_save(QUICK).is_some() {
                        self.load_from(QUICK);
                    }
                    return;
                }
                "btn_auto" => {
                    self.auto = !self.auto;
                    self.auto_at = self.now + AUTO_MS;
                    return;
                }
                "btn_skip" => {
                    self.skip = !self.skip;
                    return;
                }
                "btn_log" | "btn_backlog" => {
                    self.mode = Mode::Log {
                        top: self.log.len().saturating_sub(1),
                    };
                    return;
                }
                "btn_voice" => {
                    if let Some(v) = self.voice.clone() {
                        self.play_voice(&v);
                    }
                    return;
                }
                "btn_close" => {
                    self.set_window(false);
                    self.hidden_by_user = true;
                    return;
                }
                _ => {}
            }
        }
        if self.hidden_by_user {
            self.hidden_by_user = false;
            self.set_window(true);
            return;
        }
        self.skip = false;
        self.auto = false;
        match self.wait {
            Wait::Until(_) => self.wait = Wait::None,
            Wait::Page { cn, noswap, .. } => {
                let total = self.msg.chars().count() as f32;
                if self.shown < total {
                    self.shown = total;
                } else {
                    self.end_page(cn, noswap);
                }
            }
            _ => {}
        }
    }

    /// Right click / Escape: closes a menu, or hides the window.
    pub fn cancel(&mut self) {
        match self.mode {
            Mode::Slots { .. } | Mode::Log { .. } => self.close_menu(),
            Mode::Talk => {
                if self.hidden_by_user {
                    self.hidden_by_user = false;
                    self.set_window(true);
                } else if self.window {
                    self.hidden_by_user = true;
                    self.set_window(false);
                }
            }
            Mode::Title => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        match self.mode {
            Mode::Talk if up => {
                self.mode = Mode::Log {
                    top: self.log.len().saturating_sub(1),
                }
            }
            Mode::Log { top } => {
                if up {
                    self.mode = Mode::Log {
                        top: top.saturating_sub(1),
                    };
                } else if top + 1 >= self.log.len() {
                    self.mode = Mode::Talk;
                } else {
                    self.mode = Mode::Log { top: top + 1 };
                }
            }
            Mode::Slots { save, page } => {
                let page = if up {
                    page.saturating_sub(1)
                } else {
                    (page + 1).min(1)
                };
                self.mode = Mode::Slots { save, page };
            }
            _ => {}
        }
    }

    /// Skips text while held (the page's skip button).
    pub fn set_skip(&mut self, on: bool) {
        if self.mode == Mode::Talk && self.wait != Wait::Choice {
            self.skip = on;
        }
    }

    // ---- saves ---------------------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            file: self.file.clone(),
            pc: self.pc,
            title: self.label_title.clone(),
            text: self.msg.chars().take(40).collect(),
            bg: self.bg.as_ref().map(|b| b.1.clone()),
            fg: self
                .fg
                .iter()
                .map(|(p, f)| (p.clone(), (f.1.clone(), f.2.clone())))
                .collect(),
            cutin: self.cutin.clone(),
            bgm: self.bgm.clone(),
            window: self.window || self.hidden_by_user,
            f: self.f.clone(),
        }
    }

    fn save_as(&mut self, file: &str) {
        let s = &self.page;
        // the page saved is the one on the screen: its text, not the one before
        let text: String = self.msg.chars().take(40).collect();
        let mut out = format!(
            "file={}\npc={}\ntitle={}\ntext={}\nwindow={}\n",
            s.file,
            s.pc,
            s.title.replace('\n', " "),
            text.replace('\n', " "),
            s.window as u8
        );
        if let Some(b) = &s.bg {
            out.push_str(&format!("bg={b}\n"));
        }
        for (p, (ui, m)) in &s.fg {
            out.push_str(&format!("fg={p}|{ui}|{m}\n"));
        }
        if let Some((ui, m)) = &s.cutin {
            out.push_str(&format!("cutin={ui}|{m}\n"));
        }
        if let Some(b) = &s.bgm {
            out.push_str(&format!("bgm={b}\n"));
        }
        for (k, v) in &s.f {
            if let Some(v) = show_value(v) {
                out.push_str(&format!("f.{k}={v}\n"));
            }
        }
        out.push_str(&format!(
            "stamp={}\n",
            self.sf
                .get("save_stamp")
                .and_then(Value::num)
                .unwrap_or(0.0) as i64
                + 1
        ));
        let stamp = self
            .sf
            .get("save_stamp")
            .and_then(Value::num)
            .unwrap_or(0.0)
            + 1.0;
        self.sf.insert("save_stamp".into(), Value::Float(stamp));
        self.save_system();
        self.a.host.write_save(file, out.as_bytes());
    }

    fn read_save(&mut self, file: &str) -> Option<Snapshot> {
        let b = self.a.host.read_save(file)?;
        let mut s = Snapshot::default();
        for line in String::from_utf8_lossy(&b).lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k {
                "file" => s.file = v.into(),
                "pc" => s.pc = v.parse().unwrap_or(0),
                "title" => s.title = v.into(),
                "text" => s.text = v.into(),
                "window" => s.window = v == "1",
                "bg" => s.bg = Some(v.into()),
                "bgm" => s.bgm = Some(v.into()),
                "fg" => {
                    let mut it = v.splitn(3, '|');
                    if let (Some(p), Some(ui)) = (it.next(), it.next()) {
                        s.fg.insert(p.into(), (ui.into(), it.next().unwrap_or("").into()));
                    }
                }
                "cutin" => {
                    let (ui, m) = v.split_once('|').unwrap_or((v, ""));
                    s.cutin = Some((ui.into(), m.into()));
                }
                _ => {
                    if let Some(k) = k.strip_prefix("f.") {
                        s.f.insert(k.into(), parse_value(v));
                    }
                }
            }
        }
        (!s.file.is_empty()).then_some(s)
    }

    fn save_stamp(&mut self, file: &str) -> i64 {
        let Some(b) = self.a.host.read_save(file) else {
            return -1;
        };
        String::from_utf8_lossy(&b)
            .lines()
            .find_map(|l| l.strip_prefix("stamp=").and_then(|v| v.parse().ok()))
            .unwrap_or(0)
    }

    fn latest_save(&mut self) -> Option<String> {
        let mut best: Option<(i64, String)> = None;
        let names: Vec<String> = std::iter::once(QUICK.to_string())
            .chain((1..=SLOTS * 2).map(|n| format!("s{n:02}.txt")))
            .collect();
        for n in names {
            let s = self.save_stamp(&n);
            if s >= 0 && best.as_ref().is_none_or(|b| s > b.0) {
                best = Some((s, n));
            }
        }
        best.map(|b| b.1)
    }

    fn load_from(&mut self, file: &str) {
        let Some(s) = self.read_save(file) else {
            return;
        };
        self.reset_talk();
        self.mode = Mode::Talk;
        self.f = s.f.clone();
        self.label_title = s.title.clone();
        if let Some(bg) = &s.bg {
            self.show_bg(bg, 0.0, 0.0);
        }
        for (p, (ui, m)) in &s.fg {
            self.show_fg(ui, m, p, 0.0, 0.0, 0);
        }
        if let Some((ui, m)) = &s.cutin {
            let id = format!("GROUP.IMG.CUTIN.{}", Self::file_name(ui));
            if let Some(n) = self.ui(ui) {
                self.create(&id, n, Some(0));
                self.method(&id, m);
                self.cutin = Some((ui.clone(), m.clone()));
            }
        }
        match &s.bgm {
            Some(b) => self.play_bgm(b),
            None => self.stop_bgm(0),
        }
        self.set_window(s.window);
        self.file = s.file.clone();
        self.pc = s.pc;
        self.page = s;
    }

    // ---- drawing -------------------------------------------------------------------------

    /// A picture by path, or the clip of a sprite sheet: `/ui/img_talk/x.png` is clip `x.png` of
    /// `ui/img_talk.ss.webp` (laid out by `ui/img_talk.ssg`). Returns the part to draw too.
    fn picture(&mut self, path: &str, tint: u32) -> Option<(Rc<Img>, [f32; 4])> {
        if self.a.exists(path) {
            let img = self.image(path, tint)?;
            let src = [0.0, 0.0, img.w as f32, img.h as f32];
            return Some((img, src));
        }
        let (dir, clip) = path.trim_start_matches('/').rsplit_once('/')?;
        if !self.sheets.contains_key(dir) {
            let mut clips = HashMap::new();
            let ssg = format!("{dir}.ssg");
            if let Some(v) = self
                .a
                .read(&ssg)
                .and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok())
            {
                for c in v.get("cliplist").map(Value::list).unwrap_or(&[]) {
                    let (Some(id), Some(b)) =
                        (c.get("clipid").and_then(Value::str), c.get("subbnd"))
                    else {
                        continue;
                    };
                    let n = |i: usize| b.at(i).and_then(Value::num).unwrap_or(0.0) as f32;
                    clips.insert(id.to_string(), [n(0), n(1), n(0) + n(2), n(1) + n(3)]);
                }
            }
            self.sheets.insert(dir.to_string(), clips);
        }
        let src = *self.sheets.get(dir)?.get(clip)?;
        let img = self.image(&format!("{dir}.ss.webp"), tint)?;
        Some((img, src))
    }

    fn image(&mut self, path: &str, tint: u32) -> Option<Rc<Img>> {
        let key = (path.to_string(), tint | 0xff);
        if let Some(v) = self.imgs.get_mut(&key) {
            v.1 = self.frame;
            return Some(v.0.clone());
        }
        let img = if tint | 0xff == 0xffff_ffff {
            let p = self.a.resolve(path)?.to_string();
            let b = self.a.read(&p)?;
            match Img::decode(&b) {
                Ok(i) => i,
                Err(e) => {
                    self.warn(format!("{path}: {e}"));
                    return None;
                }
            }
        } else {
            let base = self.image(path, 0xffff_ffff)?;
            let c = text::rgba(tint);
            base.tinted([c[0], c[1], c[2]])
        };
        let img = Rc::new(img);
        self.imgs.insert(key, (img.clone(), self.frame));
        Some(img)
    }

    fn solid(&mut self, rgba: u32) -> Rc<Img> {
        let key = (String::new(), rgba | 0xff);
        if let Some(v) = self.imgs.get_mut(&key) {
            v.1 = self.frame;
            return v.0.clone();
        }
        let mut c = text::rgba(rgba);
        c[3] = 255;
        let img = Rc::new(Img::solid(c));
        self.imgs.insert(key, (img.clone(), self.frame));
        img
    }

    fn label_img(
        &mut self,
        text: &str,
        w: u32,
        h: u32,
        l: &crate::ui::Label,
        scale: f32,
    ) -> Rc<Img> {
        let key = (
            format!("{:08x}{text}", l.color),
            w,
            h,
            (l.size * scale * 10.0) as u32 ^ l.align as u32,
        );
        if let Some(v) = self.texts.get_mut(&key) {
            v.1 = self.frame;
            return v.0.clone();
        }
        let size = l.size * scale;
        let st = Style {
            size,
            color: text::rgba(l.color),
            stroke: text::rgba(if l.stroke_size > 0.0 {
                l.stroke
            } else {
                0x0000_0080
            }),
            stroke_px: (l.stroke_size.max(1.5) * scale).round() as i32,
            align: if l.align >= 9 { 1 } else { l.align % 3 },
            valign: if l.align >= 9 {
                (l.align - 9).min(2)
            } else {
                l.align / 3
            },
            pitch: size * 1.45,
        };
        let img = Rc::new(self.font.render(text, w, h, &st));
        self.texts.insert(key, (img.clone(), self.frame));
        img
    }

    fn quad(m: &Affine, w: f32, h: f32) -> [[f32; 2]; 4] {
        [
            m.apply(0.0, 0.0),
            m.apply(w, 0.0),
            m.apply(0.0, h),
            m.apply(w, h),
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &mut self,
        n: &Node,
        m: Affine,
        alpha: f32,
        path: &str,
        hover: Option<&str>,
        out: &mut Vec<Draw>,
        hits: &mut Vec<Hit>,
    ) {
        if !n.visible {
            return;
        }
        let alpha = alpha * n.opacity();
        if alpha <= 0.003 {
            return;
        }
        let m = m.then(&n.local());
        let clip = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
        let [w, h] = n.size;
        if let Some(c) = n.fill {
            let img = self.solid(c);
            out.push(Draw {
                img,
                corners: Self::quad(&m, w, h),
                src: [0.0, 0.0, 4.0, 4.0],
                alpha,
                add: false,
                clip,
            });
        }
        if let Some(p) = &n.image
            && let Some((img, src)) = self.picture(p, n.color)
        {
            let (iw, ih) = (src[2] - src[0], src[3] - src[1]);
            let (bw, bh) = if w > 0.0 && h > 0.0 { (w, h) } else { (iw, ih) };
            out.push(Draw {
                img,
                corners: Self::quad(&m, bw, bh),
                src,
                alpha,
                add: n.blend == 3,
                clip,
            });
        }
        if let Some(l) = &n.label {
            let text = if path.ends_with("MSG_FW") {
                let k = if self.shown >= f32::MAX / 2.0 {
                    usize::MAX
                } else {
                    self.shown as usize
                };
                self.msg.chars().take(k).collect()
            } else if path.ends_with(".NAME") || path == "NAME" {
                self.name.clone()
            } else {
                l.text.clone()
            };
            if !text.is_empty() && w > 0.0 && h > 0.0 {
                let s = m.sx();
                // top-aligned text may run a little below its box
                let extra = if l.align < 3 {
                    (l.size * s * 0.5) as u32
                } else {
                    0
                };
                let (tw, th) = ((w * s).ceil() as u32, (h * s).ceil() as u32 + extra);
                let white;
                let l = if path.ends_with("MSG_FW") {
                    // cpg colours the page white
                    white = crate::ui::Label {
                        color: 0xffff_ffff,
                        ..l.clone()
                    };
                    &white
                } else {
                    l
                };
                let img = self.label_img(&text, tw, th, l, s);
                let corners = Self::quad(&m, w, th as f32 / s);
                out.push(Draw {
                    img: img.clone(),
                    corners,
                    src: [0.0, 0.0, img.w as f32, img.h as f32],
                    alpha,
                    add: false,
                    clip,
                });
            }
        }
        if n.kind == 4
            && w > 0.0
            && h > 0.0
            && let Some(inv) = m.invert()
        {
            hits.push(Hit {
                name: n.name.clone(),
                inv,
                size: [w, h],
            });
        }
        let hovered = n.kind == 4 && hover == Some(n.name.as_str());
        for c in &n.children {
            let p = if path.is_empty() {
                c.name.clone()
            } else {
                format!("{path}.{}", c.name)
            };
            if n.kind == 4 && (c.name == "ON" || c.name == "OFF") {
                let mut c2 = c.clone();
                c2.visible = (c.name == "ON") == hovered;
                self.draw_node(&c2, m, alpha, &p, hover, out, hits);
                continue;
            }
            self.draw_node(c, m, alpha, &p, hover, out, hits);
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        let mut hits = Vec::new();
        let hover = self.hover.clone();
        let screen = Affine::scale(SCALE, SCALE);
        let talk = !matches!(self.mode, Mode::Title)
            && !self.base.children.is_empty()
            && !self.file.is_empty();
        if talk {
            let base = std::mem::take(&mut self.base);
            self.draw_node(
                &base,
                screen,
                1.0,
                "",
                hover.as_deref(),
                &mut out,
                &mut hits,
            );
            self.base = base;
        } else if let Some(t) = self.title.take() {
            let s = if t.size[0] > 0.0 { W / t.size[0] } else { 1.0 };
            self.draw_node(
                &t,
                screen.then(&Affine::scale(s, s)),
                1.0,
                "",
                hover.as_deref(),
                &mut out,
                &mut hits,
            );
            self.title = Some(t);
        }
        match self.mode {
            Mode::Slots { save, page } => {
                hits.clear();
                self.draw_slots(save, page, &mut out, &mut hits);
            }
            Mode::Log { top } => {
                hits.clear();
                self.draw_log(top, &mut out);
            }
            _ => {}
        }
        if self.mode == Mode::Talk && self.wait != Wait::Choice {
            hits.retain(|h| !h.name.starts_with("case"));
        }
        self.hits = hits;
        out
    }

    fn panel(&mut self, rect: [f32; 4], rgba: u32, alpha: f32, out: &mut Vec<Draw>) {
        let img = self.solid(rgba);
        let s = SCALE;
        let [x, y, w, h] = rect.map(|v| v * s);
        out.push(Draw {
            img,
            corners: [[x, y], [x + w, y], [x, y + h], [x + w, y + h]],
            src: [0.0, 0.0, 4.0, 4.0],
            alpha,
            add: false,
            clip: [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32],
        });
    }

    fn text_at(&mut self, text: &str, rect: [f32; 4], size: f32, align: i64, out: &mut Vec<Draw>) {
        let l = crate::ui::Label {
            text: String::new(),
            size,
            color: 0xffff_ffff,
            stroke: 0x0000_00c0,
            stroke_size: 1.5,
            align,
        };
        let s = SCALE;
        let [x, y, w, h] = rect.map(|v| v * s);
        let img = self.label_img(text, w as u32, h as u32, &l, s);
        out.push(Draw {
            img: img.clone(),
            corners: [[x, y], [x + w, y], [x, y + h], [x + w, y + h]],
            src: [0.0, 0.0, img.w as f32, img.h as f32],
            alpha: 1.0,
            add: false,
            clip: [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32],
        });
    }

    fn box_hit(name: &str, rect: [f32; 4], hits: &mut Vec<Hit>) {
        let [x, y, w, h] = rect.map(|v| v * SCALE);
        hits.push(Hit {
            name: name.into(),
            inv: Affine::translate(-x, -y),
            size: [w, h],
        });
    }

    fn draw_slots(&mut self, save: bool, page: usize, out: &mut Vec<Draw>, hits: &mut Vec<Hit>) {
        self.panel([0.0, 0.0, W, H], 0x0010_1830, 0.85, out);
        self.text_at(
            if save { "セーブ" } else { "ロード" },
            [60.0, 24.0, 400.0, 50.0],
            34.0,
            0,
            out,
        );
        self.text_at("戻る", [1100.0, 24.0, 140.0, 50.0], 28.0, 1, out);
        Self::box_hit("back", [1100.0, 20.0, 140.0, 50.0], hits);
        let hover = self.hover.clone();
        for i in 0..SLOTS {
            let n = page * SLOTS + i + 1;
            let (col, row) = (i % 2, i / 2);
            let rect = [
                60.0 + col as f32 * 590.0,
                96.0 + row as f32 * 100.0,
                570.0,
                88.0,
            ];
            let name = format!("slot{n}");
            let lit = hover.as_deref() == Some(name.as_str());
            self.panel(rect, if lit { 0x3050_80ff } else { 0x2030_50ff }, 0.9, out);
            let file = format!("s{n:02}.txt");
            let label = match self.read_save(&file) {
                Some(s) => format!("{n:02}  {}\n{}", s.title, s.text),
                None => format!("{n:02}  ----"),
            };
            self.text_at(
                &label,
                [
                    rect[0] + 16.0,
                    rect[1] + 10.0,
                    rect[2] - 32.0,
                    rect[3] - 16.0,
                ],
                22.0,
                0,
                out,
            );
            Self::box_hit(&name, rect, hits);
        }
        self.text_at(
            &format!(
                "{} / 2   (ホイールでページ切り替え・右クリックで閉じる)",
                page + 1
            ),
            [60.0, 680.0, 1160.0, 36.0],
            18.0,
            0,
            out,
        );
    }

    fn draw_log(&mut self, top: usize, out: &mut Vec<Draw>) {
        self.panel([0.0, 0.0, W, H], 0x0008_1020, 0.88, out);
        let start = top.saturating_sub(4);
        let lines: Vec<(String, String)> = self.log.iter().skip(start).take(5).cloned().collect();
        for (i, (name, text)) in lines.iter().enumerate() {
            let y = 40.0 + i as f32 * 130.0;
            if !name.is_empty() {
                self.text_at(name, [80.0, y, 900.0, 34.0], 22.0, 0, out);
            }
            self.text_at(text, [100.0, y + 34.0, 1080.0, 90.0], 24.0, 0, out);
        }
    }
}

impl Sound {
    fn none() -> Sound {
        Sound {
            data: Vec::new(),
            volume: 1.0,
            bgm: false,
            voice: false,
            effect_channel: None,
            stop: false,
            fade_ms: 0,
            looped: false,
        }
    }
    fn stop_effects() -> Sound {
        Sound {
            effect_channel: Some(-1),
            stop: true,
            ..Sound::none()
        }
    }
}

fn parse_value(v: &str) -> Value {
    match v {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => v
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Str(v.to_string())),
    }
}

fn show_value(v: &Value) -> Option<String> {
    Some(match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.replace('\n', " "),
        _ => return None,
    })
}
