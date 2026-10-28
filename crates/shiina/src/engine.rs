//! Runs the story files a frame at a time. The game's own .SCN code (a VM not carried out
//! here) reads the story text and draws the title and the message window from the system
//! pictures; this engine does the same from what the files say:
//!
//! - the title: `SYSTITLE.S25` frame 0, the glow of frames 250-270 and the buttons of
//!   frames 100-117 (three each: normal, under the pointer, not available);
//! - the message window: `SYS_SYS.S25` frame 10 with the name plate (12), the click prompt
//!   (20-38) and the buttons along its foot (50-75);
//! - the story: from `A001.TXT`, each file naming the next (`;￥ジャンプ＜a002＞へ`).
//!
//! Commands carried out: L_BG (the background), L_CHR (a picture on a numbered layer, moved
//! by x and y from the place its S25 frame gives), DRAW / DRAW_EX (show the changes with a
//! fade of so many ms; the wipes by a mask are drawn as fades), VOICE, SE (1: looped),
//! MUSIC, MUSIC_FADE and EX (5: wait for a click). EFECT (shakes and flashes) is skipped.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};
use vncommon::rule::Rule;

use crate::arc::{self, Archives};
use crate::s25;
use crate::story::{self, Item};

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
const FIRST: &str = "a001";
const TITLE_PIC: &str = "systitle.s25";
const SYS_PIC: &str = "sys_sys.s25";
/// the message window: text place, size, line step and letters per line
const TEXT_X: i32 = 44;
const TEXT_Y: i32 = 492;
const TEXT_SIZE: i32 = 22;
const TEXT_STEP: i32 = 26;
const PER_LINE: usize = 32;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A frame of an S25 picture and its place.
struct Pic {
    image: Rc<Image>,
    x: i32,
    y: i32,
}

#[derive(Clone, Debug, PartialEq)]
struct Layer {
    name: String,
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum Wait {
    None,
    Click,
    /// a click with the window away
    Pause,
    Fade,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Title,
    Game,
    Menu,
    Slots { save: bool, from_title: bool },
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Start,
    Load,
    QuickSave,
    QuickLoad,
    Menu,
    Skip,
    Auto,
    Voice,
    Close,
    Exit,
    None,
}

/// The title's buttons: first frame and what it does (None: not in the trial). 一時読込
/// (106) takes the place of つづきから and おまけ (112) that of 設定 in other states.
const TITLE_BUTTONS: [(usize, Act); 4] = [
    (100, Act::Start),
    (103, Act::Load),
    (109, Act::None),
    (115, Act::Exit),
];

/// The window's buttons: 一時保存 一時読込 設定 早送り 自動送り 読み返し 声再生 閉じる.
const WINDOW_BUTTONS: [(usize, Act); 8] = [
    (53, Act::QuickSave),
    (50, Act::QuickLoad),
    (73, Act::Menu),
    (62, Act::Skip),
    (66, Act::Auto),
    (70, Act::Menu),
    (59, Act::Voice),
    (56, Act::Close),
];

struct Fade {
    from: Vec<Draw>,
    start: f64,
    ms: f64,
    /// DRAW_EX kind 2: the old picture goes where its rule picture is dark first
    rule: Option<Rc<Image>>,
}

/// How soft a rule wipe's edge is (part of the rule's brightness range).
const RULE_VAGUE: f32 = 0.25;

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,
    stories: HashMap<String, Rc<Vec<Item>>>,
    raw: HashMap<String, Rc<Vec<u8>>>,
    pics: HashMap<(String, usize), Option<Rc<Pic>>>,
    text_cache: HashMap<(String, i32), Rc<Image>>,

    cur: String,
    pc: usize,
    bg: Option<String>,
    layers: BTreeMap<i32, Layer>,
    window: bool,
    /// the window put away by its button, until a click
    closed: bool,
    name: Option<String>,
    text: Option<Vec<String>>,
    wait: Wait,
    view: View,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    rules: Vec<(usize, Rule)>,
    now: f64,
    bgm: Option<String>,
    se_loop: Option<String>,
    voice: Option<String>,
    sounds: Vec<SoundEvent>,
    pointer: (i32, i32),
    skip: bool,
    auto: bool,
    /// when the page on screen went up (for the automatic mode)
    shown_at: f64,
    ended: bool,
    instead_of_title: Option<String>,
    /// the page on screen, for saves: (story, item)
    saved_at: Option<(String, usize)>,
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut())?;
        if !arcs.has(&format!("{FIRST}.txt")) {
            return Err(format!("no {FIRST}.txt in the archives"));
        }
        let mut e = Engine {
            files,
            arcs,
            width: 800,
            height: 600,
            log: false,
            font: None,
            stories: HashMap::new(),
            raw: HashMap::new(),
            pics: HashMap::new(),
            text_cache: HashMap::new(),
            cur: FIRST.into(),
            pc: 0,
            bg: None,
            layers: BTreeMap::new(),
            window: false,
            closed: false,
            name: None,
            text: None,
            wait: Wait::None,
            view: View::Title,
            frozen: None,
            rules: Vec::new(),
            fade: None,
            now: 0.0,
            bgm: None,
            se_loop: None,
            voice: None,
            sounds: Vec::new(),
            pointer: (0, 0),
            skip: false,
            auto: false,
            shown_at: 0.0,
            ended: false,
            instead_of_title: None,
            saved_at: None,
        };
        if let Some(p) = e.pic(TITLE_PIC, 0) {
            (e.width, e.height) = (p.image.w.max(320), p.image.h.max(240));
        }
        Ok(e)
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font = Some(Rc::new(
            fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
                .map_err(|e| e.to_string())?,
        ));
        self.text_cache.clear();
        Ok(())
    }

    /// Starts that story file at once instead of showing the title (for the native runner).
    pub fn start_at(&mut self, name: &str) {
        self.instead_of_title = Some(name.to_lowercase());
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("shiina: {s}");
        }
    }

    fn story(&mut self, name: &str) -> Option<Rc<Vec<Item>>> {
        let key = name.to_lowercase();
        if let Some(s) = self.stories.get(&key) {
            return Some(s.clone());
        }
        let b = self.arcs.read(self.files.as_mut(), &format!("{key}.txt"))?;
        let s = Rc::new(story::parse(&b));
        self.stories.insert(key, s.clone());
        Some(s)
    }

    // ---- running ----

    pub fn frame(&mut self) -> Result<(), String> {
        if let Some(n) = self.instead_of_title.take() {
            self.begin(&n);
        }
        self.now += FRAME_MS;
        if let Some(f) = &self.fade
            && self.now - f.start >= f.ms
        {
            self.fade = None;
        }
        if self.view == View::Game {
            match self.wait {
                Wait::Fade if self.fade.is_none() => self.wait = Wait::None,
                Wait::Click | Wait::Pause if self.skip => self.advance(),
                Wait::Click if self.auto && self.fade.is_none() => {
                    let letters: usize = self
                        .text
                        .as_ref()
                        .map_or(0, |t| t.iter().map(|l| l.chars().count()).sum());
                    if self.now - self.shown_at >= 1500.0 + letters as f64 * 90.0 {
                        self.advance();
                    }
                }
                _ => {}
            }
            self.run();
        }
        Ok(())
    }

    fn run(&mut self) {
        for _ in 0..20000 {
            if self.wait != Wait::None || self.ended || self.view != View::Game {
                return;
            }
            let Some(s) = self.story(&self.cur.clone()) else {
                self.trace(&format!("no story {}", self.cur));
                self.back_to_title();
                return;
            };
            let Some(item) = s.get(self.pc).cloned() else {
                self.trace(&format!("{} ended without a jump", self.cur));
                self.back_to_title();
                return;
            };
            self.pc += 1;
            self.exec(item);
        }
    }

    fn exec(&mut self, item: Item) {
        match item {
            Item::Page(name, lines) => {
                if self.log {
                    self.trace(&format!("{}:{} {name:?} {lines:?}", self.cur, self.pc - 1));
                }
                self.present(0.0, None);
                self.window = true;
                self.closed = false;
                self.name = name;
                self.text = Some(lines);
                self.saved_at = Some((self.cur.clone(), self.pc - 1));
                self.shown_at = self.now;
                self.wait = Wait::Click;
            }
            Item::Jump(n) => {
                self.trace(&format!("jump {n}"));
                if self.story(&n).is_some() {
                    self.cur = n;
                    self.pc = 0;
                } else {
                    // the end of the trial
                    self.back_to_title();
                }
            }
            Item::Cmd(c, a) => self.command(&c, &a),
        }
    }

    fn command(&mut self, c: &str, a: &[String]) {
        if self.log {
            self.trace(&format!("{}:{} {c} {a:?}", self.cur, self.pc - 1));
        }
        let num = |i: usize| -> i64 { a.get(i).and_then(|s| s.trim().parse().ok()).unwrap_or(0) };
        let word = |i: usize| -> String { a.get(i).map_or(String::new(), |s| story::base_name(s)) };
        match c {
            "L_BG" => {
                self.freeze();
                let n = word(0);
                self.bg = (!n.is_empty()).then_some(n);
            }
            "L_CHR" => {
                self.freeze();
                let k = num(0) as i32;
                let n = word(1);
                if n.is_empty() {
                    self.layers.remove(&k);
                } else {
                    self.layers.insert(
                        k,
                        Layer {
                            name: n,
                            x: num(2) as i32,
                            y: num(3) as i32,
                        },
                    );
                }
            }
            // the last number: 1 waits for the change to end, 0 goes on (text over it)
            "DRAW" => self.show(num(0) as f64, num(1) != 0, None),
            // kind, rule picture (kind 2), ms, wait
            "DRAW_EX" => {
                let rule = if num(0) == 2 {
                    let r = word(1);
                    self.pic(&r, 0).map(|p| p.image.clone())
                } else {
                    None
                };
                self.show(num(2) as f64, num(3) != 0, rule)
            }
            "VOICE" => {
                let n = word(0);
                self.voice = (!n.is_empty()).then(|| n.clone());
                if !self.skip && !n.is_empty() {
                    let d = self.sound(&n);
                    self.push_sound("voice", d, 1.0, false, 0.0);
                }
            }
            "SE" => {
                let n = word(0);
                if n.is_empty() {
                    self.se_loop = None;
                    self.push_sound("se", None, 0.0, false, 300.0);
                    self.push_sound("seloop", None, 0.0, false, 300.0);
                } else if num(1) == 1 {
                    self.se_loop = Some(n.clone());
                    let d = self.sound(&n);
                    self.push_sound("seloop", d, 0.8, true, 0.0);
                } else if !self.skip {
                    let d = self.sound(&n);
                    self.push_sound("se", d, 0.8, false, 0.0);
                }
            }
            "MUSIC" => {
                let n = word(0);
                if n.is_empty() {
                    self.stop_music(0.0);
                } else {
                    self.play_music(&n);
                }
            }
            "MUSIC_FADE" => {
                let ms = if a.is_empty() { 1000 } else { num(0) };
                self.stop_music(ms as f32);
            }
            "EX" if num(0) == 5 => {
                self.present(0.0, None);
                self.window = false;
                self.wait = Wait::Pause;
            }
            _ => {}
        }
    }

    // ---- pictures ----

    fn raw(&mut self, name: &str) -> Option<Rc<Vec<u8>>> {
        if let Some(b) = self.raw.get(name) {
            return Some(b.clone());
        }
        let b = Rc::new(self.arcs.read(self.files.as_mut(), name)?);
        if self.raw.len() > 8 {
            self.raw.retain(|k, _| k == TITLE_PIC || k == SYS_PIC);
        }
        self.raw.insert(name.to_string(), b.clone());
        Some(b)
    }

    fn pic(&mut self, name: &str, k: usize) -> Option<Rc<Pic>> {
        let key = (name.to_lowercase(), k);
        if let Some(p) = self.pics.get(&key) {
            return p.clone();
        }
        let p = self.raw(&key.0).and_then(|b| s25::frame(&b, k)).map(|f| {
            Rc::new(Pic {
                image: Rc::new(f.image),
                x: f.x,
                y: f.y,
            })
        });
        if p.is_none() {
            self.trace(&format!("no picture {name} #{k}"));
        }
        if self.pics.len() > 160 {
            // the system pictures' frames stay
            self.pics.retain(|k, _| k.0 == TITLE_PIC || k.0 == SYS_PIC);
        }
        self.pics.insert(key, p.clone());
        p
    }

    fn put(&mut self, name: &str, k: usize, out: &mut Vec<Draw>) {
        if let Some(p) = self.pic(name, k) {
            out.push(draw(&p.image, p.x as f32, p.y as f32, 1.0));
        }
    }

    fn rect(&mut self, name: &str, k: usize) -> Option<[i32; 4]> {
        let p = self.pic(name, k)?;
        Some([p.x, p.y, p.x + p.image.w as i32, p.y + p.image.h as i32])
    }

    // ---- drawing ----

    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.fade = None;
            self.frozen = Some(self.stage_draws());
        }
    }

    fn show(&mut self, ms: f64, wait: bool, rule: Option<Rc<Image>>) {
        self.present(ms, rule);
        if !wait && self.wait == Wait::Fade {
            self.wait = Wait::None;
        }
    }

    fn present(&mut self, ms: f64, rule: Option<Rc<Image>>) {
        if let Some(from) = self.frozen.take()
            && ms > 0.0
            && !self.skip
        {
            self.fade = Some(Fade {
                from,
                start: self.now,
                ms,
                rule,
            });
            if self.wait == Wait::None {
                self.wait = Wait::Fade;
            }
        }
    }

    fn solid(&self, rgba: [u8; 4]) -> Draw {
        let mut d = draw(&white(rgba), 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.width as f32, self.height as f32];
        d
    }

    fn stage_draws(&mut self) -> Vec<Draw> {
        let mut out = vec![self.solid([0, 0, 0, 255])];
        if let Some(bg) = self.bg.clone() {
            self.put(&bg, 0, &mut out);
        }
        let layers: Vec<Layer> = self.layers.values().cloned().collect();
        for l in layers {
            if let Some(p) = self.pic(&l.name, 0) {
                out.push(draw(&p.image, (p.x + l.x) as f32, (p.y + l.y) as f32, 1.0));
            }
        }
        out
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        self.rules.clear();
        match self.view {
            View::Title => self.title_draws(&mut out),
            View::Slots {
                from_title: true, ..
            } => self.title_draws(&mut out),
            _ => {
                out = match self.frozen.clone() {
                    Some(f) => f,
                    None => self.stage_draws(),
                };
                if let Some(f) = &self.fade {
                    let p = ((self.now - f.start) / f.ms.max(1.0)).clamp(0.0, 1.0) as f32;
                    for d in &f.from {
                        let mut d = d.clone();
                        match &f.rule {
                            Some(r) => self.rules.push((
                                out.len(),
                                Rule {
                                    image: r.clone(),
                                    progress: p,
                                    vague: RULE_VAGUE,
                                    appear: false,
                                },
                            )),
                            None => d.place.alpha *= 1.0 - p,
                        }
                        out.push(d);
                    }
                }
                if self.window && !self.closed {
                    self.window_draws(&mut out);
                }
            }
        }
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save, .. } => self.slot_draws(save, &mut out),
            View::Game | View::Title => {}
        }
        out
    }

    fn title_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 255]));
        self.put(TITLE_PIC, 0, out);
        // the crest's glow: frames 250-270 there and back
        let n = 21;
        let k = (self.now / 70.0) as usize % (2 * n - 2);
        let k = if k < n { k } else { 2 * n - 2 - k };
        self.put(TITLE_PIC, 250 + k, out);
        let live = self.view == View::Title;
        let (px, py) = self.pointer;
        for (first, act) in TITLE_BUTTONS {
            let k = if act == Act::None {
                first + 2
            } else if live
                && self
                    .rect(TITLE_PIC, first)
                    .is_some_and(|r| inside(r, px, py))
            {
                first + 1
            } else {
                first
            };
            self.put(TITLE_PIC, k, out);
        }
    }

    fn text(&mut self, s: &str, size: i32) -> Rc<Image> {
        let key = (s.to_string(), size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(Font::outline(f.clone(), size).render(
                &nexas::sjis::encode(s),
                [255, 255, 255],
                2,
            )),
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 200 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        self.put(SYS_PIC, 10, out);
        if let Some(n) = self.name.clone() {
            self.put(SYS_PIC, 12, out);
            let img = self.text(&n, 20);
            let x = ((200 - img.w as i32) / 2).max(4);
            out.push(draw(&img, x as f32, 445.0, 1.0));
        }
        let (px, py) = self.pointer;
        for (first, act) in WINDOW_BUTTONS {
            let on = (act == Act::Skip && self.skip) || (act == Act::Auto && self.auto);
            let over = self.rect(SYS_PIC, first).is_some_and(|r| inside(r, px, py));
            self.put(SYS_PIC, if over || on { first + 1 } else { first }, out);
        }
        if let Some(t) = self.text.clone() {
            let mut y = TEXT_Y;
            for line in &t {
                for piece in wrap(line, PER_LINE) {
                    let img = self.text(&piece, TEXT_SIZE);
                    out.push(draw(&img, TEXT_X as f32, y as f32, 1.0));
                    y += TEXT_STEP;
                }
            }
        }
        if self.wait == Wait::Click && self.fade.is_none() {
            let k = (self.now / 50.0) as usize % 19;
            self.put(SYS_PIC, 20 + k, out);
        }
    }

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        let mut d = self.solid(if over {
            [150, 60, 30, 230]
        } else {
            [40, 20, 30, 210]
        });
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let img = self.text(t, 20);
        let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(draw(&img, x, y, 1.0));
    }

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.width as i32, self.height as i32);
        let names = ["セーブ", "ロード", "タイトルへ戻る", "ゲームに戻る"];
        let (bw, bh, gap) = (300, 48, 18);
        let top = (h - (bh * 4 + gap * 3)) / 2;
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let y = top + i as i32 * (bh + gap);
                (*n, [(w - bw) / 2, y, (w + bw) / 2, y + bh])
            })
            .collect()
    }

    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 150]));
        for (t, r) in self.menu_items() {
            self.button(t, r, out);
        }
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as i32, self.height as i32);
        let (bw, bh, gap) = (w - 160, 38, 8);
        let top = (h - (bh + gap) * (SLOTS as i32 + 1)) / 2 + 16;
        (0..=SLOTS as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [80, y, 80 + bw, y + bh]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 200]));
        let rects = self.slot_rects();
        for (i, r) in rects.iter().enumerate() {
            let label = if i == SLOTS {
                "戻る".to_string()
            } else {
                match self.read_slot(&slot_name(i)) {
                    Some(s) => format!("{:02}  {}", i + 1, clip(&s.title, 30)),
                    None => format!("{:02}  ----", i + 1),
                }
            };
            self.button(&label, *r, out);
        }
        let head = self.text(if save { "セーブ" } else { "ロード" }, 26);
        out.push(draw(&head, 80.0, (rects[0][1] - 38) as f32, 1.0));
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let ds = self.draws();
        vncommon::rule::render_ruled(self.width, self.height, &ds, &self.rules)
    }

    /// The rule wipes of the last [`Engine::draws`]: (draw index, rule).
    pub fn rules(&self) -> &[(usize, Rule)] {
        &self.rules
    }

    // ---- sounds ----

    fn sound(&mut self, name: &str) -> Option<Vec<u8>> {
        let b = self.arcs.read(self.files.as_mut(), name);
        if b.is_none() {
            self.trace(&format!("no sound {name}"));
        }
        b.map(arc::ogg)
    }

    fn push_sound(
        &mut self,
        channel: &str,
        data: Option<Vec<u8>>,
        volume: f32,
        repeat: bool,
        fade_ms: f32,
    ) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data,
            volume,
            repeat,
            fade_ms,
        });
    }

    fn play_music(&mut self, name: &str) {
        if self.bgm.as_deref() == Some(name) {
            return;
        }
        self.bgm = Some(name.to_string());
        let d = self.sound(name);
        self.push_sound("bgm", d, 0.7, true, 0.0);
    }

    fn stop_music(&mut self, ms: f32) {
        self.bgm = None;
        self.push_sound("bgm", None, 0.0, false, ms.max(0.0));
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        match self.wait {
            Wait::Click => {
                self.wait = Wait::None;
                self.text = None;
                self.name = None;
            }
            Wait::Pause => self.wait = Wait::None,
            _ => {}
        }
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::Start => self.begin(FIRST),
            Act::Load => {
                let from_title = self.view == View::Title;
                self.view = View::Slots {
                    save: false,
                    from_title,
                };
            }
            Act::QuickSave => self.save("quick"),
            Act::QuickLoad => {
                if self.read_slot("quick").is_some() {
                    self.load("quick");
                }
            }
            Act::Menu => self.view = View::Menu,
            Act::Skip => self.skip = !self.skip,
            Act::Auto => {
                self.auto = !self.auto;
                self.shown_at = self.now;
            }
            Act::Voice => {
                if let Some(v) = self.voice.clone() {
                    let d = self.sound(&v);
                    self.push_sound("voice", d, 1.0, false, 0.0);
                }
            }
            Act::Close => self.closed = true,
            Act::Exit => {
                self.trace("exit");
                self.ended = true;
            }
            Act::None => {}
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        match self.view {
            View::Title => {
                for (first, act) in TITLE_BUTTONS {
                    if act != Act::None
                        && self.rect(TITLE_PIC, first).is_some_and(|r| inside(r, x, y))
                    {
                        self.act(act);
                        return;
                    }
                }
                return;
            }
            View::Menu => {
                let hit = self
                    .menu_items()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                match hit {
                    Some(0) => {
                        self.view = View::Slots {
                            save: true,
                            from_title: false,
                        }
                    }
                    Some(1) => {
                        self.view = View::Slots {
                            save: false,
                            from_title: false,
                        }
                    }
                    Some(2) => self.back_to_title(),
                    Some(3) => self.view = View::Game,
                    _ => {}
                }
                return;
            }
            View::Slots { save, .. } => {
                let hit = self.slot_rects().into_iter().position(|r| inside(r, x, y));
                match hit {
                    Some(i) if i < SLOTS => {
                        let n = slot_name(i);
                        if save {
                            self.save(&n);
                        } else if self.read_slot(&n).is_some() {
                            self.load(&n);
                        }
                    }
                    Some(_) => self.close_slots(),
                    None => {}
                }
                return;
            }
            View::Game => {}
        }
        if self.closed {
            self.closed = false;
            return;
        }
        if self.fade.is_some() && matches!(self.wait, Wait::Click | Wait::Pause) {
            self.fade = None;
            return;
        }
        match self.wait {
            Wait::Click => {
                let hit = WINDOW_BUTTONS
                    .iter()
                    .find(|(first, _)| self.rect(SYS_PIC, *first).is_some_and(|r| inside(r, x, y)))
                    .map(|b| b.1);
                if let Some(a) = hit {
                    self.act(a);
                    return;
                }
                self.push_sound("voice", None, 0.0, false, 0.0);
                self.advance();
            }
            Wait::Pause => self.advance(),
            _ => {}
        }
    }

    fn close_slots(&mut self) {
        self.view = match self.view {
            View::Slots {
                from_title: true, ..
            } => View::Title,
            _ => View::Menu,
        };
    }

    pub fn cancel(&mut self) {
        match self.view {
            View::Menu => self.view = View::Game,
            View::Slots { .. } => self.close_slots(),
            View::Title => {}
            View::Game => {
                self.skip = false;
                if self.closed {
                    self.closed = false;
                } else if self.wait == Wait::Click {
                    self.view = View::Menu;
                }
            }
        }
    }

    pub fn key(&mut self, k: usize) {
        match k {
            4 => {
                let (x, y) = self.pointer;
                if self.view != View::Game || matches!(self.wait, Wait::Click | Wait::Pause) {
                    self.click(x, y);
                }
            }
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && matches!(self.wait, Wait::Click | Wait::Pause) && self.view == View::Game {
            self.push_sound("voice", None, 0.0, false, 0.0);
            self.advance();
        }
    }

    /// Whether the title (a choice of buttons) is up.
    pub fn choosing(&self) -> bool {
        self.view == View::Title
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    fn reset_scene(&mut self) {
        self.bg = None;
        self.layers.clear();
        self.window = false;
        self.closed = false;
        self.text = None;
        self.name = None;
        self.wait = Wait::None;
        self.frozen = None;
        self.fade = None;
        self.skip = false;
        self.auto = false;
        self.voice = None;
        self.stop_music(300.0);
        self.se_loop = None;
        for ch in ["se", "seloop", "voice"] {
            self.push_sound(ch, None, 0.0, false, 300.0);
        }
    }

    /// Starts a story file from the top.
    fn begin(&mut self, name: &str) {
        self.reset_scene();
        self.view = View::Game;
        self.cur = name.to_string();
        self.pc = 0;
        self.saved_at = None;
    }

    fn back_to_title(&mut self) {
        self.reset_scene();
        self.saved_at = None;
        self.view = View::Title;
    }

    // ---- saves ----

    fn read_slot(&mut self, name: &str) -> Option<Slot> {
        let b = self.files.read_save(name)?;
        Slot::parse(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, slot: &str) {
        let Some((cur, pc)) = self.saved_at.clone() else {
            return;
        };
        let mut s = String::new();
        let first = self.text.clone().unwrap_or_default().concat();
        let title = match &self.name {
            Some(n) => format!("【{n}】{first}"),
            None => first,
        };
        s += &format!("title {}\n", title.replace('\n', " "));
        s += &format!("at {pc} {cur}\n");
        if let Some(b) = &self.bg {
            s += &format!("bg {b}\n");
        }
        for (k, l) in &self.layers {
            s += &format!("layer {k} {} {} {}\n", l.x, l.y, l.name);
        }
        if let Some(b) = &self.bgm {
            s += &format!("bgm {b}\n");
        }
        if let Some(b) = &self.se_loop {
            s += &format!("seloop {b}\n");
        }
        self.files.write_save(slot, s.as_bytes());
        self.view = View::Game;
    }

    fn load(&mut self, slot: &str) {
        let Some(s) = self.read_slot(slot) else {
            return;
        };
        self.reset_scene();
        self.view = View::Game;
        self.cur = s.at.0.clone();
        self.pc = s.at.1;
        self.bg = s.bg;
        self.layers = s.layers;
        if let Some(b) = s.bgm {
            self.play_music(&b);
        }
        if let Some(b) = s.se_loop {
            let d = self.sound(&b);
            self.se_loop = Some(b);
            self.push_sound("seloop", d, 0.8, true, 0.0);
        }
        self.saved_at = Some(s.at);
        self.frozen = Some(vec![self.solid([0, 0, 0, 255])]);
        self.present(400.0, None);
    }
}

struct Slot {
    title: String,
    at: (String, usize),
    bg: Option<String>,
    layers: BTreeMap<i32, Layer>,
    bgm: Option<String>,
    se_loop: Option<String>,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let mut slot = Slot {
            title: String::new(),
            at: (String::new(), 0),
            bg: None,
            layers: BTreeMap::new(),
            bgm: None,
            se_loop: None,
        };
        let mut seen = false;
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "title" => slot.title = rest.to_string(),
                "at" => {
                    let (a, b) = rest.split_once(' ')?;
                    slot.at = (b.to_string(), a.parse().ok()?);
                    seen = true;
                }
                "bg" => slot.bg = Some(rest.to_string()),
                "layer" => {
                    let p: Vec<&str> = rest.splitn(4, ' ').collect();
                    if p.len() == 4 {
                        slot.layers.insert(
                            p[0].parse().unwrap_or(0),
                            Layer {
                                name: p[3].to_string(),
                                x: p[1].parse().unwrap_or(0),
                                y: p[2].parse().unwrap_or(0),
                            },
                        );
                    }
                }
                "bgm" => slot.bgm = Some(rest.to_string()),
                "seloop" => slot.se_loop = Some(rest.to_string()),
                _ => {}
            }
        }
        seen.then_some(slot)
    }
}

fn slot_name(i: usize) -> String {
    format!("slot{i:02}")
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

fn white(rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(1, 1);
    i.fill(rgba);
    Rc::new(i)
}

/// Breaks a line into pieces of about `n` characters; closing marks stay on the line before.
fn wrap(s: &str, n: usize) -> Vec<String> {
    const CLOSING: &str = "」』）】〉》、。，．・ー…！？!?)ぁぃぅぇぉっゃゅょァィゥェォッャュョ";
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut end = (i + n.max(1)).min(chars.len());
        let mut k = 0;
        while end < chars.len() && k < 2 && CLOSING.contains(chars[end]) {
            end += 1;
            k += 1;
        }
        out.push(chars[i..end].iter().collect());
        i = end;
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
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
    #[test]
    fn wrap_keeps_closing_marks() {
        assert_eq!(super::wrap("あいう」", 3), vec!["あいう」"]);
        assert_eq!(super::wrap("あいうえお", 3), vec!["あいう", "えお"]);
    }
}
