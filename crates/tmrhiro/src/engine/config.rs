//! The system (config) screen, laid out by the ini's SYSTEM_PARTS_<page><n> lines:
//!
//! ```text
//! flag,KEY,kind,x,y,src x,src y,w,h,count,per group,groups,step x,step y,src step x,src step y,
//!   group step x,group step y,range,-
//! ```
//!
//! SW_GROUP is a row of buttons of which one is chosen (in each group), SW_SLIDE a slider (its
//! knob `w` x `h`, moving over `range` pixels) and SW_ONE a button. Page 99 is on every page.
//! The buttons are in sys_system_button_01, a row each for plain, lit and chosen; each page has
//! its picture sys_system_base_<page>. The older games draw each page whole, plain and lit
//! (SYSTEM_IMAGES_<page>, our own key, see `old::SETTINGS`).
//!
//! What the settings do here: the volumes and sounds on or off, the auto mode's speed, the
//! window's colour, the pages, back to the game or the title, and quitting. The rest are kept
//! and shown but change nothing. They are kept in config.dat.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::gfx::Draw;

use super::{Engine, Wait, draw, inside, part};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Group,
    Slide,
    One,
}

#[derive(Clone)]
struct Part {
    key: String,
    kind: Kind,
    f: Vec<i32>,
}

impl Part {
    fn n(&self) -> usize {
        self.f[6].max(1) as usize
    }
    fn per(&self) -> usize {
        self.f[7].max(1) as usize
    }
    fn range(&self) -> i32 {
        self.f.get(15).copied().filter(|&r| r > 0).unwrap_or(256)
    }
    /// The k-th button: where it is and where its picture is.
    fn button(&self, k: usize) -> ([i32; 4], [i32; 2]) {
        let f = &self.f;
        let (i, g) = ((k % self.per()) as i32, (k / self.per()) as i32);
        let at = [
            f[0] + i * f[9] + g * f.get(13).copied().unwrap_or(0),
            f[1] + i * f[10] + g * f.get(14).copied().unwrap_or(0),
            f[4],
            f[5],
        ];
        (at, [f[2] + i * f[11], f[3] + i * f[12]])
    }
    /// The k-th slider's track.
    fn track(&self, k: usize) -> [i32; 4] {
        let f = &self.f;
        let k = k as i32;
        [f[0] + k * f[9], f[1] + k * f[10], self.range() + f[4], f[5]]
    }
}

pub(super) struct ConfigView {
    page: i32,
}

/// The settings kept between plays: per key, the chosen button of each group or each slider's
/// value.
#[derive(Default)]
pub(super) struct Settings(pub HashMap<String, Vec<i32>>);

impl Settings {
    fn decode(b: &[u8]) -> Settings {
        let mut s = Settings::default();
        for line in String::from_utf8_lossy(b).lines() {
            if let Some((k, v)) = line.split_once('=') {
                let v = v.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                s.0.insert(k.to_string(), v);
            }
        }
        s
    }
    fn encode(&self) -> Vec<u8> {
        let mut keys: Vec<&String> = self.0.keys().collect();
        keys.sort();
        let mut out = String::new();
        for k in keys {
            let v: Vec<String> = self.0[k].iter().map(|n| n.to_string()).collect();
            out += &format!("{k}={}\n", v.join(","));
        }
        out.into_bytes()
    }
}

impl Engine {
    fn config_parts(&self, page: i32) -> Vec<Part> {
        let mut out = Vec::new();
        for p in [page, 99] {
            for n in 1..=30 {
                let Some(v) = self.ini.get(&format!("SYSTEM/SYSTEM_PARTS_{p:02}{n:02}")) else {
                    continue;
                };
                let v: Vec<&str> = v.split(',').map(|s| s.trim()).collect();
                if v.len() < 16 {
                    continue;
                }
                let kind = match v[2] {
                    "SW_GROUP" => Kind::Group,
                    "SW_SLIDE" => Kind::Slide,
                    "SW_ONE" => Kind::One,
                    _ => continue,
                };
                let f = v[3..].iter().map(|s| s.parse().unwrap_or(0)).collect();
                out.push(Part {
                    key: v[1].to_string(),
                    kind,
                    f,
                });
            }
        }
        out
    }

    /// Reads config.dat, with the ini's defaults for what it does not have.
    pub(super) fn load_settings(&mut self) {
        let saved = self
            .files
            .read_save("config.dat")
            .map(|b| Settings::decode(&b))
            .unwrap_or_default();
        let mut s = Settings::default();
        let pages = self.ini.int("SYSTEM/SYSTEM_PAGE_MAX", 2);
        for page in 1..=pages {
            for p in self.config_parts(page) {
                if s.0.contains_key(&p.key) || p.kind == Kind::One {
                    continue;
                }
                let v = saved
                    .0
                    .get(&p.key)
                    .cloned()
                    .unwrap_or_else(|| self.default_setting(&p));
                s.0.insert(p.key.clone(), v);
            }
        }
        self.settings = s;
    }

    fn default_setting(&self, p: &Part) -> Vec<i32> {
        let def = |k: &str| self.ini.ints(&format!("SYSTEM/SYS_DEF_{k}"));
        match (p.key.as_str(), p.kind) {
            ("S_TEXTSPEED", _) | ("S_AUTOSPEED", _) => {
                let k = if p.key == "S_TEXTSPEED" {
                    "TEXTSPEED"
                } else {
                    "AUTOSPEED"
                };
                let v = def(k).first().copied().unwrap_or(p.per() as i32 / 2 + 1);
                vec![(v - 1).clamp(0, p.per() as i32 - 1)]
            }
            ("S_WINDOW_STAT", _) => {
                let stat = self
                    .ini
                    .get("SYSTEM/SYS_DEF_WINDOW_STAT")
                    .and_then(|v| u32::from_str_radix(v.trim(), 16).ok())
                    .unwrap_or(0x8000_0000);
                stat.to_be_bytes()
                    .iter()
                    .map(|&b| b as i32 * p.range() / 256)
                    .collect()
            }
            ("S_SOUND_VOLUME", _) => {
                let v = def("SOUND_VOLUME");
                (0..p.n())
                    .map(|k| v.get(k).copied().unwrap_or(100) * (p.range() - 1) / 100)
                    .collect()
            }
            (_, Kind::Slide) => vec![p.range() - 1; p.n()],
            // SYS_DEF_<key> holds the chosen button of each group
            (key, _) => {
                let v = def(key.trim_start_matches("S_"));
                (0..p.n() / p.per())
                    .map(|g| v.get(g).copied().unwrap_or(0).clamp(0, p.per() as i32 - 1))
                    .collect()
            }
        }
    }

    fn setting(&self, key: &str) -> Option<&Vec<i32>> {
        self.settings.0.get(key)
    }

    /// How loud a kind of sound plays (0..=1), from the volume sliders and the on/off buttons.
    pub(super) fn volume(&self, bgm: bool, voice: bool) -> f32 {
        let order = self
            .ini
            .get("SYSTEM/SOUND_ORDER")
            .unwrap_or("-,voice,se,bgm")
            .to_string();
        let kind = if bgm {
            "bgm"
        } else if voice {
            "voice"
        } else {
            "se"
        };
        let Some(k) = order.split(',').position(|s| s.trim() == kind) else {
            return 1.0;
        };
        if self
            .setting("S_SOUND")
            .and_then(|v| v.get(k))
            .is_some_and(|&v| v != 0)
        {
            return 0.0;
        }
        let range = self
            .config_parts(self.ini.int("SYSTEM/SYSTEM_PAGE_MAX", 2))
            .into_iter()
            .chain(self.config_parts(1))
            .find(|p| p.key == "S_SOUND_VOLUME")
            .map_or(256, |p| p.range());
        self.setting("S_SOUND_VOLUME")
            .and_then(|v| v.get(k))
            .map_or(1.0, |&v| {
                (v as f32 / (range - 1).max(1) as f32).clamp(0.0, 1.0)
            })
    }

    /// How much longer than normal the auto mode waits (1 in the middle of the speeds).
    pub(super) fn auto_factor(&self) -> f32 {
        let Some(&v) = self.setting("S_AUTOSPEED").and_then(|v| v.first()) else {
            return 1.0;
        };
        let n = self
            .config_parts(2)
            .into_iter()
            .chain(self.config_parts(1))
            .find(|p| p.key == "S_AUTOSPEED")
            .map_or(5, |p| p.per()) as f32;
        ((n - v as f32) / (n / 2.0)).max(0.2)
    }

    /// The window's colour (AARRGGBB) from the sliders, if the game has them.
    pub(super) fn window_stat(&self) -> Option<u32> {
        let v = self.setting("S_WINDOW_STAT")?;
        let range = (1..=3)
            .flat_map(|p| self.config_parts(p))
            .find(|p| p.key == "S_WINDOW_STAT")
            .map_or(256, |p| p.range());
        let b = |k: usize| (v.get(k).copied().unwrap_or(0) * 256 / range).clamp(0, 255) as u32;
        Some(b(0) << 24 | b(1) << 16 | b(2) << 8 | b(3))
    }

    pub(super) fn config_open(&self) -> bool {
        self.config.is_some()
    }

    pub(super) fn open_config(&mut self) {
        self.config = Some(ConfigView { page: 1 });
    }

    fn config_images(&self, page: i32) -> Option<(String, String)> {
        let v = self.ini.get(&format!("SYSTEM/SYSTEM_IMAGES_{page:02}"))?;
        let (a, b) = v.split_once(',')?;
        Some((a.trim().to_string(), b.trim().to_string()))
    }

    pub(super) fn config_click(&mut self, x: i32, y: i32) {
        let Some(page) = self.config.as_ref().map(|c| c.page) else {
            return;
        };
        for p in self.config_parts(page) {
            match p.kind {
                Kind::Group | Kind::One => {
                    let Some(k) = (0..p.n()).find(|&k| inside(p.button(k).0, x, y)) else {
                        continue;
                    };
                    match p.key.as_str() {
                        "S_PAGE" => {
                            if let Some(c) = &mut self.config {
                                c.page = k as i32 + 1;
                            }
                        }
                        "S_GAME" => self.config = None,
                        "S_TITLE" => {
                            self.config = None;
                            self.restart();
                        }
                        "S_END" => {
                            self.config = None;
                            self.wait = Wait::Ended;
                        }
                        "S_WINDOW_STAT_DEFAULT" => {
                            let parts = (1..=3).flat_map(|p| self.config_parts(p));
                            if let Some(s) = parts.into_iter().find(|p| p.key == "S_WINDOW_STAT") {
                                let v = self.default_setting(&s);
                                self.settings.0.insert(s.key, v);
                                self.window_base = None;
                            }
                        }
                        _ if p.kind == Kind::Group => {
                            let v = self.settings.0.entry(p.key.clone()).or_default();
                            let g = k / p.per();
                            if v.len() <= g {
                                v.resize(g + 1, 0);
                            }
                            v[g] = (k % p.per()) as i32;
                        }
                        _ => {}
                    }
                    self.save_settings();
                    return;
                }
                Kind::Slide => {
                    let Some(k) = (0..p.n()).find(|&k| inside(p.track(k), x, y)) else {
                        continue;
                    };
                    let value = (x - p.track(k)[0] - p.f[4] / 2).clamp(0, p.range() - 1);
                    let v = self.settings.0.entry(p.key.clone()).or_default();
                    if v.len() <= k {
                        v.resize(k + 1, 0);
                    }
                    v[k] = value;
                    if p.key == "S_WINDOW_STAT" {
                        self.window_base = None;
                    }
                    self.save_settings();
                    return;
                }
            }
        }
    }

    fn save_settings(&mut self) {
        let b = self.settings.encode();
        self.files.write_save("config.dat", &b);
    }

    pub(super) fn config_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(page) = self.config.as_ref().map(|c| c.page) else {
            return;
        };
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let images = self.config_images(page);
        let base = images
            .as_ref()
            .map_or(format!("sys_system_base_{page:02}"), |i| i.0.clone());
        match self.image(&base) {
            Some(g) => out.push(draw(
                &g.image,
                g.x as f32,
                g.y as f32,
                1.0,
                [0.0, 0.0, w, h],
            )),
            None => {
                let p = Rc::new(super::panel(self.screen.0, self.screen.1));
                out.push(draw(&p, 0.0, 0.0, 1.0, [0.0, 0.0, w, h]));
            }
        }
        let lit = images.and_then(|i| self.image(&i.1));
        let sheet = self.image("sys_system_button_01");
        let hover = self.hover;
        for p in self.config_parts(page) {
            let chosen = self.settings.0.get(&p.key).cloned().unwrap_or_default();
            match p.kind {
                Kind::Group | Kind::One => {
                    for k in 0..p.n() {
                        let (r, src) = p.button(k);
                        let on = match p.key.as_str() {
                            "S_PAGE" => k as i32 + 1 == page,
                            _ => chosen.get(k / p.per()) == Some(&((k % p.per()) as i32)),
                        };
                        let state = if on {
                            2
                        } else {
                            inside(r, hover.0, hover.1) as i32
                        };
                        match (&lit, &sheet) {
                            (Some(g), _) => {
                                if state > 0 {
                                    let mut d =
                                        draw(&g.image, g.x as f32, g.y as f32, 1.0, [0.0; 4]);
                                    d.place.clip = [
                                        r[0] as f32,
                                        r[1] as f32,
                                        (r[0] + r[2]) as f32,
                                        (r[1] + r[3]) as f32,
                                    ];
                                    out.push(d);
                                }
                            }
                            (None, Some(g)) => {
                                let src = [src[0] - g.x, src[1] + state * r[3] - g.y, r[2], r[3]];
                                out.push(part(&g.image, src, r[0] as f32, r[1] as f32));
                            }
                            _ => {}
                        }
                    }
                }
                Kind::Slide => {
                    for k in 0..p.n() {
                        let t = p.track(k);
                        let v = chosen.get(k).copied().unwrap_or(0);
                        match (&lit, &sheet) {
                            // the older games light the track up to the value
                            (Some(g), _) => {
                                let mut d = draw(&g.image, g.x as f32, g.y as f32, 1.0, [0.0; 4]);
                                d.place.clip = [
                                    t[0] as f32,
                                    t[1] as f32,
                                    (t[0] + v * (t[2] - p.f[4]) / p.range().max(1)) as f32,
                                    (t[1] + t[3]) as f32,
                                ];
                                out.push(d);
                            }
                            (None, Some(g)) => {
                                let src = [p.f[2] - g.x, p.f[3] - g.y, p.f[4], p.f[5]];
                                out.push(part(&g.image, src, (t[0] + v) as f32, t[1] as f32));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
