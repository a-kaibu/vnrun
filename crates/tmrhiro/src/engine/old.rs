//! The commands of the 2006-2008 games (group [`OLD`]): one line each, the command's byte then
//! its arguments. What each one does was worked out from the scripts of the 2006 maimiko trial
//! and its command handler (a jump table on the command byte).

use std::rc::Rc;

use nexas::gfx::{Draw, Image};

use super::{Button, Engine, Flash, Wait, part};
use crate::script::{Command, OLD};

/// The variables are named by a bank (`$GLO147`); this is where each bank's numbers go.
const BANKS: [(&str, u8); 8] = [
    ("map", 0),
    ("glo", 1),
    ("tmp", 2),
    ("sta", 3),
    ("sys", 5),
    ("hsc", 6),
    ("cgv", 7),
    ("ssf", 8),
];

/// The icons of mapselect_charactor (x, y, w, h, and how far left of its place each is
/// drawn), from the maimiko program; each stands on its place, its frames one above the other.
const MAP_ICONS: [[i32; 5]; 15] = [
    [2, 0, 61, 64, 30],
    [78, 0, 32, 64, 18],
    [140, 0, 39, 64, 20],
    [211, 0, 31, 64, 14],
    [268, 0, 39, 64, 20],
    [332, 0, 45, 64, 20],
    [398, 0, 33, 64, 18],
    [465, 0, 30, 64, 15],
    [528, 0, 35, 64, 16],
    [576, 0, 64, 64, 32],
    [640, 0, 64, 64, 32],
    [704, 0, 64, 64, 32],
    [768, 0, 64, 64, 32],
    [832, 0, 64, 64, 32],
    [896, 0, 64, 64, 32],
];

/// Where the slots of 0x46 put a character (from the middle).
const OLD_SLOT_X: [f32; 5] = [0.0, 0.0, -200.0, 200.0, 0.0];

/// Hex digits from `at`, `n` of them.
fn hex(c: &Command, at: usize, n: usize) -> i32 {
    let b = c.args.get(at..at + n).unwrap_or(&[]);
    i32::from_str_radix(std::str::from_utf8(b).unwrap_or(""), 16).unwrap_or(0)
}

/// What the window and the choices need in a game without settings (our own defaults, from the
/// pictures' sizes).
pub const SETTINGS: &str = "
[MESSAGEWINDOW]
MW_DRAW_POS_X=0
MW_DRAW_POS_Y=390
TEXT_DRAW_POS_X=100
TEXT_DRAW_POS_Y=66
TEXT_FONT=22
TEXT_DRAW_NEXT=30
TEXT_MAX=58
NP_PLATE_POS=60,-28
CW_DRAW_POS_X=738
CW_DRAW_POS_Y=172
CW_DRAW_WIDTH=32
CW_DRAW_HEIGHT=32
CW_DRAW_PAT=12
CW_DRAW_TIME=80
[SELECT]
SELECT_WIDTH=456
SELECT_HEIGHT=40
SELECT_POSY=180
SELECT_NEXT_Y=24
SELECT_TEXT_FONT_SIZE=22
[SAVE]
SAVE_BASE_POS=12,110,0,0,384,88,10,5,2,0,96,0,0,392,0
SAVE_PAGE_POS=460,66,0,0,30,32,10,0,0,32,0,0,0
SAVE_B_THUM_POS=16,8
SAVE_B_TEXT_POS=118,12,18
SAVE_IMAGES=sys_save01,sys_save02
[LOAD]
LOAD_BASE_POS=12,110,0,0,384,88,10,5,2,0,96,0,0,392,0
LOAD_PAGE_POS=460,66,0,0,30,32,10,0,0,32,0,0,0
LOAD_B_THUM_POS=16,8
LOAD_B_TEXT_POS=118,12,18
LOAD_IMAGES=sys_load01,sys_load02
[SAVE]
SAVE_THUM_SIZE=94,70
[SYSTEM]
SYSTEM_PAGE_MAX=2
SYSTEM_IMAGES_01=sys_system01,sys_system02
SYSTEM_IMAGES_02=sys_system03,sys_system04
SOUND_ORDER=bgm,se,voice
SYSTEM_PARTS_0101=0,S_SCREEN,SW_GROUP,283,113,0,0,150,28,2,2,1,210,0,0,0,0,0,-,-
SYSTEM_PARTS_0102=0,S_GRAPHIC,SW_GROUP,378,143,0,0,56,28,2,2,1,85,0,0,0,0,0,-,-
SYSTEM_PARTS_0103=0,S_TEXTSPEED,SW_GROUP,493,234,0,0,28,28,5,5,1,35,0,0,0,0,0,-,-
SYSTEM_PARTS_0104=0,S_AUTOSPEED,SW_GROUP,493,264,0,0,28,28,5,5,1,35,0,0,0,0,0,-,-
SYSTEM_PARTS_0105=0,S_FONT_SMOOTH,SW_GROUP,509,294,0,0,54,28,2,2,1,88,0,0,0,0,0,-,-
SYSTEM_PARTS_0106=0,S_SKIP_S_STOP,SW_GROUP,490,323,0,0,76,28,2,2,1,113,0,0,0,0,0,-,-
SYSTEM_PARTS_0107=0,S_SKIP,SW_GROUP,509,353,0,0,54,28,2,2,1,88,0,0,0,0,0,-,-
SYSTEM_PARTS_0108=0,S_SOUND,SW_GROUP,229,443,0,0,50,28,6,2,3,89,0,0,0,0,30,-,-
SYSTEM_PARTS_0109=0,S_SOUND_VOLUME,SW_SLIDE,466,445,0,0,0,24,3,3,1,0,30,0,0,0,0,200,0
SYSTEM_PARTS_9901=0,S_PAGE,SW_GROUP,438,8,0,0,126,34,2,2,1,162,0,0,0,0,0,-,-
SYSTEM_PARTS_9902=0,S_GAME,SW_ONE,264,560,0,0,148,30,1,1,1,0,0,0,0,0,0,-,-
SYSTEM_PARTS_9903=0,S_TITLE,SW_ONE,440,560,0,0,124,30,1,1,1,0,0,0,0,0,0,-,-
SYSTEM_PARTS_9904=0,S_END,SW_ONE,604,560,0,0,124,30,1,1,1,0,0,0,0,0,0,-,-
[BACKLOG]
BACKLOG_IMAGES=sys_backlog,sys_backlogbutton
BACKLOG_MAX=5
BACKLOG_FONT_SIZE=20
BACKLOG_NEXT_LINE=26
BACKLOG_POS_X=110
BACKLOG_POS_Y=64
BACKLOG_NEXT_Y=108
BACKLOG_NAME_X=0
BACKLOG_NAME_Y=-26
BACKLOG_VOICE=400,-28,0,0,198,24
";

/// The hero's default names (as ＄α, ＄β and ＄γ) from the game's program, where they sit just
/// before the name of the first script, the short one first.
pub fn player_names(exe: &[u8]) -> Option<[String; 3]> {
    let at = exe.windows(7).position(|w| w == b"\0start\0")?;
    let mut names = exe[..at]
        .split(|&b| b == 0)
        .rev()
        .filter(|s| !s.is_empty())
        .take(3)
        .map(|s| (s[0] >= 0x81).then(|| nexas::sjis::decode(s)));
    let (a, b, c) = (names.next()??, names.next()??, names.next()??);
    Some([a, b, c])
}

impl Engine {
    fn old_var(name: &str) -> Option<(u8, u16)> {
        let name = name.trim().strip_prefix('$')?;
        let (bank, n) = name.split_at_checked(3)?;
        let bank = BANKS.iter().find(|b| b.0.eq_ignore_ascii_case(bank))?.1;
        Some((bank, n.parse().ok()?))
    }

    /// A number: a variable (`kind` 1) or written out.
    fn old_value(&self, kind: u8, s: &str) -> i32 {
        if kind == 1 {
            Self::old_var(s).map_or(0, |(b, n)| self.var(b, n))
        } else {
            s.trim().parse().unwrap_or(0)
        }
    }

    fn old_set(&mut self, mode: u8, var: &str, value: i32) {
        let Some(key) = Self::old_var(var) else {
            return;
        };
        let v = self.var(key.0, key.1);
        let v = match mode {
            1 => value,
            2 => v + value,
            3 => v - value,
            4 => v * value,
            5 if value != 0 => v / value,
            6 if value != 0 => v % value,
            _ => v,
        };
        // the variables are bytes
        self.vars.insert(key, v as u8 as i32);
    }

    fn old_compare(cmp: u8, a: i32, b: i32) -> bool {
        match cmp {
            1 => a == b,
            2 => a != b,
            3 => a > b,
            4 => a < b,
            5 => a >= b,
            6 => a <= b,
            _ => false,
        }
    }

    /// Goes to a label (`kind` 1) or a script (2); `then` is a label in that script.
    fn old_go(&mut self, kind: u8, name: &str, then: Option<&str>) {
        if kind == 2 {
            self.jump_file(name);
            if let Some(label) = then {
                self.jump_label(label);
            }
        } else {
            self.jump_label(name);
        }
    }

    /// Starts a transition for an effect byte (1: at once).
    fn old_fade(&mut self, effect: u8) {
        self.begin_fade(if effect > 1 { 1 } else { 0 });
    }

    /// Clears the pictures over the background (layers 1-10).
    fn old_clear(&mut self) {
        self.chars.iter_mut().for_each(|c| *c = None);
        self.overlay.clear();
    }

    fn old_stop(&mut self, bgm: bool, channel: Option<i32>, fade_ms: u32) {
        if bgm {
            self.bgm.clear();
        }
        self.sounds.push(super::Sound {
            data: Vec::new(),
            volume: 0.0,
            bgm,
            voice: false,
            effect_channel: channel,
            stop: true,
            fade_ms,
            loop_start: 0,
        });
    }

    /// Waits on the buttons once the last of a run of them (command `op`) is set up.
    fn old_buttons_wait(&mut self, op: u16) {
        let next = self.script.commands.get(self.pc);
        if !next.is_some_and(|n| n.group == OLD && n.op == op) && !self.buttons.is_empty() {
            self.wait = Wait::Buttons;
        }
    }

    pub(super) fn run_old(&mut self, c: &Command) {
        let s = c.str(0);
        match c.op {
            // a line: bit 0 has a voice, 2 and 3 a name ("voice,name,text")
            0x14..=0x17 => {
                let k = c.op - 0x14;
                let mut parts: Vec<&str> = s.split(',').collect();
                let voice = if k & 1 != 0 && parts.len() > 1 {
                    Some(parts.remove(0))
                } else {
                    None
                };
                let name = if k > 1 && parts.len() > 1 {
                    Some(parts.remove(0))
                } else {
                    None
                };
                let text = parts.join(",");
                self.say_line(name, &text, voice);
            }
            // jump and call: [1 label / 2 script][fields] "name[,label]"
            0x1e | 0x1f => {
                let rest = c.str(2);
                let mut f = rest.split(',');
                let name = f.next().unwrap_or("").to_string();
                let then = f.next().filter(|_| c.u8(1) == 2).map(|l| l.to_string());
                if c.op == 0x1f {
                    self.stack.push((self.script.clone(), self.pc));
                }
                self.old_go(c.u8(0), &name, then.as_deref());
            }
            0x20 => match self.stack.pop() {
                Some((s, pc)) => {
                    self.script = s;
                    self.pc = pc;
                }
                None => self.wait = Wait::Ended,
            },
            0x21 => {}
            // if: [compare][1 variable / 2 number][1 label / 2 script] "var value target"
            0x22 => {
                let rest = c.str(3);
                let f: Vec<&str> = rest.split(' ').collect();
                if f.len() >= 3 {
                    let a = self.old_value(1, f[0]);
                    let b = self.old_value(c.u8(1), f[1]);
                    if Self::old_compare(c.u8(0), a, b) {
                        self.old_go(c.u8(2), f[2], None);
                    }
                }
            }
            // a choice: [?][count] then " \x14\x01text,label" for each option
            0x23 => {
                let options: Vec<(String, String)> = s
                    .split(" \u{14}\u{1}")
                    .skip(1)
                    .filter_map(|o| {
                        let (text, label) = o.rsplit_once(',')?;
                        Some((text.to_string(), label.trim().to_string()))
                    })
                    .collect();
                if !options.is_empty() {
                    self.window = false;
                    self.resume_pc = self.pc - 1;
                    self.wait = Wait::Choice(options);
                }
            }
            // buttons on a picture: [count][?] ",normal,lit" then " x1y1x2y2label" (hex, 3
            // digits each) for each button
            0x25 => {
                let rest = c.str(2);
                let mut f = rest.split(' ');
                let mut images = f.next().unwrap_or("").split(',').skip(1);
                self.button_images = (
                    images.next().unwrap_or("").to_string(),
                    images.next().unwrap_or("").to_string(),
                );
                self.buttons.clear();
                for b in f.filter(|b| b.len() > 12) {
                    let n = |k: usize| i32::from_str_radix(&b[k * 3..k * 3 + 3], 16).unwrap_or(0);
                    self.buttons.push(Button {
                        rect: [n(0), n(1), n(2) - n(0), n(3) - n(1)],
                        label: b[12..].to_string(),
                        icon: None,
                    });
                }
                if !self.buttons.is_empty() {
                    self.wait = Wait::Buttons;
                }
            }
            0x28 => {
                self.buttons.clear();
                self.button_images = (String::new(), String::new());
            }
            // the map: [?][?] ",base,date"
            0x2e => {
                let mut f = s.get(2..).unwrap_or("").split(',').skip(1);
                let base = f.next().unwrap_or("").to_string();
                let date = f.next().unwrap_or("").to_string();
                self.old_clear();
                self.set_bg(&base);
                self.overlay = self.pictures(&date, 0.0, 0.0);
                self.window = false;
            }
            // a place on the map: "002"[icon][plate] (1 and up, plus 0x14) x y (hex, 3 digits:
            // where the icon stands) ",label"
            0x31 => {
                let icon = (c.u8(3) as usize).wrapping_sub(0x15);
                if c.bytes(0).starts_with(b"002") && icon < MAP_ICONS.len() {
                    let (x, y) = (hex(c, 5, 3), hex(c, 8, 3));
                    let [_, _, w, h, left] = MAP_ICONS[icon];
                    let label = s.split(',').nth(1).unwrap_or("").to_string();
                    self.buttons.push(Button {
                        rect: [x - left, y - h, w, h],
                        label,
                        icon: Some([icon as i32, c.u8(4) as i32 - 0x15]),
                    });
                }
                self.old_buttons_wait(0x31);
            }
            // the background: 0x32 clears the pictures over it, 0x33 too ([effect][?] name),
            // 0x34 only changes it
            0x32 | 0x34 => {
                self.old_fade(c.u8(0));
                if c.op == 0x32 {
                    self.old_clear();
                }
                self.pan = None;
                self.set_bg(&c.str(1));
            }
            0x33 => {
                self.old_fade(c.u8(0));
                self.old_clear();
                self.pan = None;
                self.set_bg(&c.str(2));
            }
            // characters: [effect][count + 1] then their names split by spaces, left to right
            // (count 1: none)
            0x50 => {
                self.old_fade(c.u8(0));
                let rest = c.str(2);
                let names: Vec<&str> = rest.split(' ').collect();
                let n = (c.u8(1) as usize).saturating_sub(1).min(names.len());
                let xs: &[f32] = match n {
                    1 => &[0.0],
                    2 => &[-200.0, 200.0],
                    3 => &[-200.0, 0.0, 200.0],
                    _ => &[-200.0, 0.0, 200.0, 0.0],
                };
                self.chars.iter_mut().for_each(|c| *c = None);
                for (i, name) in names.iter().take(n).enumerate() {
                    if !name.is_empty() {
                        let at = (xs[i.min(xs.len() - 1)], 0.0);
                        self.set_chara(i, name, at, at, 0, None);
                    }
                }
            }
            // one character: [effect][slot 1-4] name ("null": none); slots 2 and 3 stand left and
            // right
            0x46 => {
                self.old_fade(c.u8(0));
                let slot = c.u8(1) as usize;
                let name = c.str(2);
                if slot < self.chars.len() {
                    if name == "null" {
                        self.chars[slot] = None;
                    } else {
                        let at = (OLD_SLOT_X[slot.min(4)], 0.0);
                        self.set_chara(slot, &name, at, at, 0, None);
                    }
                }
            }
            // a picture coming in: [effect][slot][?][?] ms (hex 4) then x0 y0 x1 y1 (hex 3, plus
            // 1000) and four more numbers, then the name; it slides and fades in
            0x4c => {
                self.old_fade(c.u8(0));
                let slot = c.u8(1) as usize;
                let ms = hex(c, 4, 4) as u64;
                let p = |k: usize| (hex(c, 8 + k * 3, 3) - 1000) as f32;
                let name = c.str(8 + 8 * 3);
                if slot < self.chars.len() {
                    self.set_chara(slot, &name, (p(0), p(1)), (p(2), p(3)), ms, Some(true));
                }
            }
            0x5c => self.shake = Some(self.now),
            // a picture over the characters: "001"[1 hide / 2 show] name
            0x55 => {
                if c.bytes(0).starts_with(b"001") {
                    self.overlay = match c.u8(3) {
                        2 => self.pictures(&c.str(4), 0.0, 0.0),
                        _ => Vec::new(),
                    };
                }
            }
            // a flash: r g b (hex), repeats, then in, hold and out (hex, tens of ms)
            0x5b => {
                let mut img = Image::new(self.screen.0, self.screen.1);
                img.fill([
                    hex(c, 0, 2) as u8,
                    hex(c, 2, 2) as u8,
                    hex(c, 4, 2) as u8,
                    255,
                ]);
                let ms = [7, 11, 15].map(|at| hex(c, at, 4) as u64 * 10);
                self.wait = Wait::Until(self.now + ms.iter().sum::<u64>());
                self.flash = Some(Flash {
                    image: Rc::new(img),
                    start: self.now,
                    ms,
                });
            }
            0x64 => self.play_bgm(&c.str(2)),
            0x65 => self.old_stop(true, None, (c.u8(0) as u32 * 100).saturating_sub(2000)),
            0x66 => self.old_stop(true, None, 0),
            0x69 | 0x6e | 0x73 => {
                let channel = match c.op {
                    0x6e => 0,
                    0x69 => 1,
                    _ => 2,
                };
                self.play(&["se"], &c.str(1), Some(channel), false);
            }
            0x6a | 0x74 => {
                let channel = if c.op == 0x6a { 1 } else { 2 };
                self.play(&["se"], &c.str(2), Some(channel), false);
            }
            0x6b => self.old_stop(false, Some(1), 0),
            0x6f => self.old_stop(false, Some(0), 0),
            0x76 => self.old_stop(false, Some(2), 0),
            // variables: [1 set / 2 add / 3 sub / 4 mul ...][1 variable / 2 number] "var value"
            0x82 => {
                let rest = c.str(2);
                if let Some((var, value)) = rest.split_once(' ') {
                    let v = self.old_value(c.u8(1), value);
                    self.old_set(c.u8(0), var, v);
                }
            }
            // if [compare][kind]",var,value" then [mode][kind]",var,value"
            0x9d => {
                let mut f = c.args.split(|&b| b == b' ');
                let (Some(test), Some(set)) = (f.next(), f.next()) else {
                    return;
                };
                let fields = |b: &[u8]| -> (u8, u8, Vec<String>) {
                    let text = nexas::sjis::decode(b.get(2..).unwrap_or(&[]));
                    let parts = text.split(',').skip(1).map(|p| p.to_string()).collect();
                    (
                        b.first().copied().unwrap_or(0),
                        b.get(1).copied().unwrap_or(0),
                        parts,
                    )
                };
                let (cmp, kind, t) = fields(test);
                let (mode, vkind, v) = fields(set);
                if t.len() >= 2 && v.len() >= 2 {
                    let a = self.old_value(1, &t[0]);
                    let b = self.old_value(kind, &t[1]);
                    if Self::old_compare(cmp, a, b) {
                        let value = self.old_value(vkind, &v[1]);
                        self.old_set(mode, &v[0], value);
                    }
                }
            }
            // a wait (hex ms; a click ends it)
            0x96..=0x98 => self.wait = Wait::Until(self.now + hex(c, 0, 8) as u64),
            0xa1 => self.title = s,
            0xa3 => self.wait = Wait::Ended,
            0xa4 => self.open_menu(true),
            0xa5 => self.open_menu(false),
            0xa6 => self.open_config(),
            _ => {
                if self.log {
                    eprintln!("tmrhiro: skipped old {:02x}", c.op);
                }
            }
        }
    }

    /// The places of the map: the icon (mapselect_charactor, 64 square, lit one row below) and,
    /// under the pointer, the place's name (mapselect_plate, 40 high).
    pub(super) fn map_draws(
        &mut self,
        icons: &[([i32; 4], [i32; 2])],
        hit: Option<[i32; 4]>,
        out: &mut Vec<Draw>,
    ) {
        let sheet = self.image("mapselect_charactor");
        let plate = self.image("mapselect_plate");
        for &(r, [icon, row]) in icons {
            let lit = hit == Some(r);
            let [sx, sy, ..] = MAP_ICONS[icon as usize];
            if let Some(g) = &sheet {
                // the frames walk while the pointer is on it
                let frame = if lit { (self.now / 200 % 4) as i32 } else { 0 };
                let src = [sx - g.x, sy + frame * 64 - g.y, r[2], r[3]];
                out.push(part(&g.image, src, r[0] as f32, r[1] as f32));
            }
            if lit && let Some(g) = &plate {
                let w = g.image.w as i32;
                let x = (r[0] + r[2] / 2 - w / 2).clamp(0, self.screen.0 as i32 - w);
                let y = if r[1] + r[3] + 40 > self.screen.1 as i32 {
                    r[1] - 40
                } else {
                    r[1] + r[3]
                };
                let src = [-g.x, row * 40 - g.y, w, 40];
                out.push(part(&g.image, src, x as f32, y as f32));
            }
        }
    }
}
