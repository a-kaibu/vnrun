//! The card game of the 2010 games (50.0064 "GAME01": "かけひき"), as the exe plays it. The
//! girl holds five cards face down, three OK and two NG, shuffled. She lifts them one at a time
//! (the next one still in her hand, in turn) and says something; an OK card gets one of her OK
//! lines and an NG card one of her NG lines (section [GAME01] of _mg_sys: OKnn / NGnn =
//! picture,voice,text). The player holds 我慢 three times, 勃起 and 搾乳, and a pass:
//!
//! - 我慢: both her card and the 我慢 card go away and the cards she lifts start again;
//! - 勃起: her card decides it: OK is 成功 (大成功 when EXTRA_SETTING is on and a 我慢 was spent
//!   on an OK card), NG is 失敗, and after 失敗 the game asks whether to try again;
//! - 搾乳: ends the game with 搾乳;
//! - pass: she lifts the next card; once she has lifted all of them she lifts them again in the
//!   same order (saying the same), and every fifth pass costs a 我慢 card.
//!
//! The result goes to variable 0 of bank 2: 0 失敗, 1 成功, 2 大成功, 3 搾乳. The exe draws the
//! cards in 3D; here they lie flat, her cards above and the player's below.

use std::rc::Rc;

use nexas::gfx::Draw;

use super::{Engine, Wait, draw, inside, part};
use crate::ini::Ini;

/// A line: picture, voice and text.
type Line = [String; 3];

const CARD: [i32; 2] = [76, 106];
const SHEET: &str = "mg_card01";
/// her cards' top and the player's
const GIRL_Y: i32 = 16;
const HAND_Y: i32 = 444;
const PASS: [i32; 4] = [647, 561, 144, 32];
const RESTART: [i32; 4] = [8, 562, 146, 32];
const YES: [i32; 4] = [142, 330, 192, 80];
const NO: [i32; 4] = [466, 330, 192, 80];

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// she has lifted a card and the player picks
    Pick,
    /// her card shows what it was, until then
    Reveal(u64),
    /// the result is up, until then
    Result(u64),
    /// try again?
    Retry,
}

pub(super) struct CardGame {
    ok: Vec<Line>,
    ng: Vec<Line>,
    start: Vec<String>,
    finish: [Vec<String>; 4],
    bonus_on: bool,
    /// her cards: OK or NG while she has them
    girl: [Option<bool>; 5],
    /// the player's cards: 我慢 ×3, 勃起, 搾乳
    hand: [bool; 5],
    /// the card she lifts next (in turn) and those lifted since the last 我慢, with their lines
    next: usize,
    lifted: Vec<(usize, usize)>,
    /// how many of `lifted` have been lifted (again)
    count: usize,
    again: bool,
    current: usize,
    /// her picture and what she says
    face: String,
    text: String,
    passes: u32,
    bonus: bool,
    picked: usize,
    result: i32,
    phase: Phase,
    /// the background: MG_Base_01, 03 (bonus), 04 (勃起) or 05 (失敗)
    base: &'static str,
}

impl Engine {
    /// 50.0064: the card game of a [section] of _mg_sys.
    pub(super) fn game_start(&mut self, name: &str) {
        let mut ini = Ini::default();
        if let Some(b) = self.read("srp", "_mg_sys") {
            ini.add(&b);
        }
        let field = |key: &str| -> Vec<String> {
            ini.get(&format!("{name}/{key}"))
                .map(|v| v.splitn(3, ',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default()
        };
        let lines = |kind: &str| -> Vec<Line> {
            (1..=5)
                .map(|k| field(&format!("{kind}{k:02}")))
                .filter(|v| v.len() == 3)
                .map(|v| [v[0].clone(), v[1].clone(), v[2].clone()])
                .collect()
        };
        let (ok, ng) = (lines("OK"), lines("NG"));
        if ok.is_empty() || ng.is_empty() {
            eprintln!("tmrhiro: no card game {name}");
            return;
        }
        let bgm = ini
            .get(&format!("{name}/BGM"))
            .map(|s| s.trim().to_string());
        let game = CardGame {
            ok,
            ng,
            start: field("START_CHARA"),
            finish: [
                field("FIN_CHARA_01"),
                field("FIN_CHARA_02"),
                field("FIN_CHARA_03"),
                field("FIN_CHARA_04"),
            ],
            bonus_on: ini.int(&format!("{name}/EXTRA_SETTING"), 0) != 0,
            girl: [None; 5],
            hand: [true; 5],
            next: 0,
            lifted: Vec::new(),
            count: 0,
            again: false,
            current: 0,
            face: String::new(),
            text: String::new(),
            passes: 0,
            bonus: false,
            picked: 0,
            result: 0,
            phase: Phase::Pick,
            base: "MG_Base_01",
        };
        self.game = Some(game);
        self.window = false;
        self.text.clear();
        if let Some(b) = bgm {
            self.play_bgm(&b);
        }
        self.wait = Wait::Game;
        self.game_deal();
    }

    /// A new game: her cards shuffled, the player's all back.
    fn game_deal(&mut self) {
        let mut cards = vec![true, false, true, false, true];
        let mut girl = [None; 5];
        for g in girl.iter_mut() {
            let k = self.random(cards.len() as u32) as usize;
            *g = Some(cards.remove(k));
        }
        let Some(game) = &mut self.game else {
            return;
        };
        game.girl = girl;
        game.hand = [true; 5];
        game.next = 0;
        game.lifted.clear();
        game.count = 0;
        game.again = false;
        game.passes = 0;
        game.bonus = false;
        game.base = "MG_Base_01";
        game.phase = Phase::Pick;
        let start = game.start.clone();
        game.face = start.first().cloned().unwrap_or_default();
        game.text.clear();
        if let Some(v) = start.get(1) {
            self.play(&["vic"], v, None, true);
        }
        self.game_lift();
    }

    /// She lifts her next card (or, the second time round, the next of those lifted before).
    fn game_lift(&mut self) {
        let Some(game) = &self.game else {
            return;
        };
        let (card, line) = if game.again {
            match game.lifted.get(game.count) {
                Some(&l) => l,
                None => return,
            }
        } else {
            let Some(card) = (0..5)
                .map(|k| (game.next + k) % 5)
                .find(|&k| game.girl[k].is_some())
            else {
                return;
            };
            let n = if game.girl[card] == Some(true) {
                game.ok.len()
            } else {
                game.ng.len()
            };
            let line = self.random(n as u32) as usize;
            let game = self.game.as_mut().unwrap();
            game.next = (card + 1) % 5;
            game.lifted.push((card, line));
            (card, line)
        };
        let Some(game) = &mut self.game else {
            return;
        };
        game.count += 1;
        game.current = card;
        let lines = if game.girl[card] == Some(true) {
            &game.ok
        } else {
            &game.ng
        };
        let [face, voice, text] = lines[line.min(lines.len() - 1)].clone();
        game.face = face;
        game.text = text;
        self.play(&["vic"], &voice, None, true);
    }

    pub(super) fn game_click(&mut self, x: i32, y: i32) {
        let Some(game) = &self.game else {
            return;
        };
        match game.phase {
            Phase::Pick => {
                if inside(RESTART, x, y) {
                    self.play(&["se"], "MG_SE_009", Some(9), false);
                    self.game_deal();
                    return;
                }
                if inside(PASS, x, y) {
                    let se = if game.count == 5 {
                        "MG_SE_007"
                    } else {
                        "MG_SE_006"
                    };
                    self.play(&["se"], se, Some(9), false);
                    self.game_pass();
                    return;
                }
                let Some(k) = (0..5).find(|&k| game.hand[k] && inside(hand_rect(k), x, y)) else {
                    return;
                };
                let ok = game.girl[game.current] == Some(true);
                self.play(&["se"], "MG_SE_004", Some(9), false);
                let sound = match k {
                    0..=2 if ok => Some("MG_SE_011"),
                    0..=2 => Some("MG_SE_010"),
                    4 => Some("MG_SE_014"),
                    _ => None,
                };
                if let Some(s) = sound {
                    self.play(&["se"], s, Some(8), false);
                }
                let now = self.now;
                let game = self.game.as_mut().unwrap();
                game.picked = k;
                if k == 3 {
                    game.base = "MG_Base_04";
                }
                game.phase = Phase::Reveal(now + 1200);
            }
            Phase::Retry => {
                if inside(YES, x, y) {
                    self.play(&["se"], "MG_SE_009", Some(9), false);
                    self.game_deal();
                } else if inside(NO, x, y) {
                    self.play(&["se"], "MG_SE_009", Some(9), false);
                    self.game_end();
                }
            }
            // a click hurries the result along
            Phase::Result(_) if game.result != 0 => self.game_end(),
            Phase::Result(_) => self.game.as_mut().unwrap().phase = Phase::Retry,
            Phase::Reveal(_) => {}
        }
    }

    fn game_pass(&mut self) {
        let Some(game) = &mut self.game else {
            return;
        };
        let left = game.girl.iter().filter(|g| g.is_some()).count();
        if game.count >= left {
            game.count = 0;
            game.again = true;
        }
        game.passes += 1;
        if game.passes == 5 {
            game.passes = 0;
            if let Some(k) = (0..3).find(|&k| game.hand[k]) {
                game.hand[k] = false;
            }
        }
        self.game_lift();
    }

    /// Runs the game's timers (called once a frame).
    pub(super) fn game_frame(&mut self) {
        let Some(game) = &mut self.game else {
            return;
        };
        let now = self.now;
        match game.phase {
            Phase::Reveal(t) if now >= t => {
                let ok = game.girl[game.current] == Some(true);
                match game.picked {
                    0..=2 => {
                        // 我慢: her card and the player's go, and her cards start again
                        if ok && game.bonus_on {
                            game.bonus = true;
                            game.base = "MG_Base_03";
                        }
                        game.girl[game.current] = None;
                        game.hand[game.picked] = false;
                        game.lifted.clear();
                        game.count = 0;
                        game.again = false;
                        game.passes = 0;
                        game.phase = Phase::Pick;
                        self.game_lift();
                        return;
                    }
                    3 => {
                        game.result = match (ok, game.bonus) {
                            (false, _) => 0,
                            (true, false) => 1,
                            (true, true) => 2,
                        };
                    }
                    _ => game.result = 3,
                }
                let r = game.result as usize;
                let finish = game.finish[if r == 0 { 0 } else { r.min(3) }].clone();
                if r == 0 {
                    game.base = "MG_Base_05";
                }
                game.face = finish.first().cloned().unwrap_or_default();
                game.text.clear();
                game.phase = Phase::Result(now + 4000);
                let se = ["MG_SE_012", "MG_SE_013", "MG_SE_013", "MG_SE_023"][r];
                self.play(&["se"], se, Some(8), false);
                if let Some(v) = finish.get(1) {
                    self.play(&["vic"], v, None, true);
                }
            }
            Phase::Result(t) if now >= t => {
                if game.result == 0 {
                    game.phase = Phase::Retry;
                } else {
                    self.game_end();
                }
            }
            _ => {}
        }
    }

    fn game_end(&mut self) {
        if let Some(game) = self.game.take() {
            self.vars.insert((2, 0), game.result);
        }
        self.wait = Wait::None;
    }

    pub(super) fn game_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(game) = &self.game else {
            return;
        };
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let all = [0.0, 0.0, w, h];
        let (base, face, text) = (game.base, game.face.clone(), game.text.clone());
        let (girl, hand, current, phase, result) =
            (game.girl, game.hand, game.current, game.phase, game.result);
        let hover = self.hover;
        if let Some(g) = self.image(base) {
            out.push(draw(&g.image, 0.0, 0.0, 1.0, all));
        }
        if !face.is_empty() {
            let cw = self
                .macros
                .get(&face.to_lowercase())
                .and_then(|p| p.first().cloned())
                .and_then(|c| self.image(&c))
                .map_or(w, |g| g.canvas.0 as f32);
            let y = self.ini.int("GAMESYSTEM/CHARA_SPR_PLUS_Y", 0) as f32;
            for p in self.pictures(&face, (w - cw) / 2.0, y) {
                out.push(draw(&p.image, p.x, p.y, 1.0, all));
            }
        }
        let Some(sheet) = self.image(SHEET) else {
            return;
        };
        let card = |out: &mut Vec<Draw>, src: [i32; 2], r: [i32; 4]| {
            let s = [src[0] - sheet.x, src[1] - sheet.y, r[2], r[3]];
            out.push(part(&sheet.image, s, r[0] as f32, r[1] as f32));
        };
        let shown = !matches!(phase, Phase::Pick | Phase::Retry);
        for (k, g) in girl.iter().enumerate() {
            let Some(ok) = g else {
                continue;
            };
            let mut r = girl_rect(k);
            let up = k == current && phase != Phase::Retry;
            if up {
                r[1] += 24;
            }
            let src = match (up && shown, ok) {
                (true, true) => [0x140, 0],
                (true, false) => [0xf0, 0],
                _ => [0x190, 0],
            };
            card(out, src, r);
            if up {
                card(out, [0x190, 0x80], r);
            }
        }
        for (k, &alive) in hand.iter().enumerate() {
            if !alive {
                continue;
            }
            let r = hand_rect(k);
            let lit = phase == Phase::Pick && inside(r, hover.0, hover.1);
            let picked = shown && k == self.game.as_ref().map_or(9, |g| g.picked);
            let x = [0, 0, 0, 0x50, 0xa0][k];
            let y = if lit || picked { 0x80 } else { 0 };
            card(out, [x, y], r);
        }
        if phase == Phase::Pick {
            for (r, x) in [(PASS, 0x1e0), (RESTART, 0x280)] {
                let y = if inside(r, hover.0, hover.1) { 40 } else { 0 };
                card(out, [x, y], r);
            }
        }
        // what she says, in the box
        if !text.is_empty()
            && let Some(b) = self.image("MG_MessageBox")
        {
            let (bx, by) = ((w as i32 - b.image.w as i32) / 2, 290);
            out.push(draw(&b.image, bx as f32, by as f32, 0.85, all));
            if let Some(font) = self.text_font(22) {
                let chars: Vec<char> = text.chars().collect();
                for (i, row) in chars.chunks(19).take(4).enumerate() {
                    let s: String = row.iter().collect();
                    let img = Rc::new(font.render(&nexas::sjis::encode(&s), [255, 255, 255], 2));
                    let y = by + 16 + i as i32 * 28;
                    out.push(draw(&img, (bx + 22) as f32, y as f32, 1.0, all));
                }
            }
        }
        match phase {
            Phase::Result(_) => {
                // 失敗 / 成功 / 大成功 / 搾乳, one above the other
                if let Some(g) = self.image("MG_DrawText") {
                    let r = [380 - g.x, 200 * result - g.y + 10, 500, 190];
                    out.push(part(&g.image, r, 150.0, 190.0));
                }
            }
            Phase::Retry => {
                if let Some(g) = self.image("MG_Retry") {
                    out.push(draw(&g.image, g.x as f32, g.y as f32, 1.0, all));
                }
                if let Some(g) = self.image("MG_DrawText") {
                    for (r, x) in [(YES, 0), (NO, 0xc0)] {
                        let y = if inside(r, hover.0, hover.1) { 0 } else { 0x50 };
                        let src = [x - g.x, y - g.y, r[2], r[3]];
                        out.push(part(&g.image, src, r[0] as f32, r[1] as f32));
                    }
                }
            }
            _ => {}
        }
    }
}

fn girl_rect(k: usize) -> [i32; 4] {
    [
        400 - CARD[0] / 2 + 109 * (k as i32 - 2),
        GIRL_Y,
        CARD[0],
        CARD[1],
    ]
}

fn hand_rect(k: usize) -> [i32; 4] {
    [
        400 - CARD[0] / 2 + 109 * (k as i32 - 2),
        HAND_Y,
        CARD[0],
        CARD[1],
    ]
}
