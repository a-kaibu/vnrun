//! Saves (`\scp,dat`): our own format in place of the game's. A save holds the scenario script
//! the game thread was in, the story line it showed last, the save texts and date, and the game
//! thread's variables. A load starts that scenario again from `@@TOP` and skips through it (the
//! game's own skip mode, nothing waiting) up to that line, then sets the saved variables back.

use std::collections::HashMap;
use std::rc::Rc;

use crate::engine::{Engine, GAME, SoundEvent, VKind, Var, Wait};
use crate::expr::Value;

/// The most sub-frames a load may skip through.
const MAX_SKIP: u32 = 200_000;

pub struct Loading {
    pub file: String,
    pub op: usize,
    pub vars: Vec<(String, Var)>,
}

#[derive(Default)]
struct Save {
    file: String,
    op: usize,
    date: [i64; 6],
    text: Vec<String>,
    vars: Vec<(String, Var)>,
}

fn save_name(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("qlie-save-{n}.txt")
}

fn sep(v: &[String]) -> String {
    v.iter()
        .map(|s| s.replace(['\t', '\n', '\u{1f}'], " "))
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

impl Save {
    fn write(&self) -> String {
        let mut out = format!("file\t{}\nop\t{}\n", self.file, self.op);
        let d: Vec<String> = self.date.iter().map(|n| n.to_string()).collect();
        out += &format!("date\t{}\n", d.join(" "));
        out += &format!("text\t{}\n", sep(&self.text));
        for (n, v) in &self.vars {
            let k = if v.kind == VKind::Int { "i" } else { "s" };
            let vals: Vec<String> = v.vals.iter().map(|x| x.str()).collect();
            out += &format!("v\t{n}\t{k}\t{}\n", sep(&vals));
        }
        out
    }

    fn read(b: &[u8]) -> Option<Save> {
        let mut s = Save::default();
        for line in String::from_utf8_lossy(b).lines() {
            let mut p = line.splitn(2, '\t');
            let (Some(k), Some(rest)) = (p.next(), p.next()) else {
                continue;
            };
            match k {
                "file" => s.file = rest.to_string(),
                "op" => s.op = rest.parse().ok()?,
                "date" => {
                    for (i, n) in rest.split(' ').take(6).enumerate() {
                        s.date[i] = n.parse().unwrap_or(0);
                    }
                }
                "text" => s.text = rest.split('\u{1f}').map(String::from).collect(),
                "v" => {
                    let mut p = rest.splitn(3, '\t');
                    let (Some(n), Some(k), Some(v)) = (p.next(), p.next(), p.next()) else {
                        continue;
                    };
                    let int = k == "i";
                    let vals = v
                        .split('\u{1f}')
                        .map(|x| {
                            if int {
                                Value::Int(x.parse().unwrap_or(0))
                            } else {
                                Value::Str(x.to_string())
                            }
                        })
                        .collect();
                    let kind = if int { VKind::Int } else { VKind::Str };
                    s.vars.push((
                        n.to_string(),
                        Var {
                            kind,
                            vals,
                            own: Vec::new(),
                        },
                    ));
                }
                _ => {}
            }
        }
        (!s.file.is_empty()).then_some(s)
    }
}

impl Engine {
    fn read_save(&mut self, mem: Option<char>, name: &str) -> Option<Save> {
        let b = if let Some(c) = mem {
            self.mem_saves.get(&format!("{c}:{name}"))?.clone()
        } else {
            self.g.files.read_save(&save_name(name))?
        };
        Save::read(&b)
    }

    fn write_save(&mut self, mem: Option<char>, name: &str, b: Vec<u8>) {
        if let Some(c) = mem {
            let key = format!("{c}:{name}");
            self.mem_order.retain(|k| *k != key);
            if b.is_empty() {
                self.mem_saves.remove(&key);
            } else {
                self.mem_saves.insert(key.clone(), b);
                self.mem_order.push(key);
                // `\lim`: only so many of a store's newest saves are kept
                let limit = if c == 't' { self.tdat_limit } else { 0 };
                let prefix = format!("{c}:");
                while limit > 0
                    && self
                        .mem_order
                        .iter()
                        .filter(|k| k.starts_with(&prefix))
                        .count()
                        > limit
                {
                    let at = self.mem_order.iter().position(|k| k.starts_with(&prefix));
                    if let Some(at) = at {
                        let k = self.mem_order.remove(at);
                        self.mem_saves.remove(&k);
                    }
                }
            }
        } else {
            self.g.files.write_save(&save_name(name), &b);
        }
    }

    /// `\scp,dat` commands (`mem`: the in-memory store, `m` for `mdat` and `t` for `tdat`).
    pub(crate) fn dat_cmd(&mut self, mem: Option<char>, name: &str, v: &[Value]) {
        let file = v.first().map(|v| v.str()).unwrap_or_default();
        match name {
            "g" | "?g" => {
                let there = self.read_save(mem, &file).is_some();
                self.res_int(0, there as i64);
            }
            "gd" | "?gd" => {
                let s = self.read_save(mem, &file).unwrap_or_default();
                for (i, n) in s.date.iter().enumerate() {
                    self.res_int(i as i64, *n);
                }
                for i in 0..4 {
                    self.res_str(i as i64, s.text.get(i).cloned().unwrap_or_default());
                }
            }
            "s" => {
                let ok = self.save(mem, &file);
                self.res_int(0, ok as i64);
            }
            "l" => {
                let ok = self.load(mem, &file);
                self.res_int(0, ok as i64);
            }
            "d" => self.write_save(mem, &file, Vec::new()),
            "i" => {
                if let Some(c) = mem {
                    let prefix = format!("{c}:");
                    self.mem_saves.retain(|k, _| !k.starts_with(&prefix));
                    self.mem_order.retain(|k| !k.starts_with(&prefix));
                }
            }
            "lim" => {
                if mem == Some('t') {
                    self.tdat_limit = v.first().map_or(0, |v| v.int()).max(0) as usize;
                }
            }
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                }
            }
        }
    }

    fn save(&mut self, mem: Option<char>, name: &str) -> bool {
        // before the first story line: the scenario's start (op 0)
        let Some((file, op)) = self.last_text.clone().or_else(|| {
            let f = self.threads[GAME].frames.first()?;
            Some((f.unit.files[0].clone(), 0))
        }) else {
            self.warn(format!("save {name}: no scenario"));
            return false;
        };
        // the variables from before the line: the load runs it again
        let vars = match (&self.last_text, &self.line_vars) {
            (Some(_), Some(v)) => v.clone(),
            _ => self.game_vars(),
        };
        let s = Save {
            file,
            op,
            date: self.today,
            text: self.savetext.to_vec(),
            vars,
        };
        self.write_save(mem, name, s.write().into_bytes());
        if mem.is_none() {
            let im = self.game_image(0);
            let thumb = crate::engine::resample(&im, im.w / 4, im.h / 4);
            self.thumbs.insert(name.to_lowercase(), Rc::new(thumb));
        }
        true
    }

    /// Starts the saved scenario over and skips to the saved line, now.
    fn load(&mut self, mem: Option<char>, name: &str) -> bool {
        let Some(s) = self.read_save(mem, name) else {
            return false;
        };
        let (cur, refer) = (self.cur, self.refer);
        // the game thread starts over, without its objects
        let globals: Vec<Var> = self.threads[GAME].globals.drain().map(|(_, v)| v).collect();
        for v in globals {
            self.free_game_var(v);
        }
        let builtin = self.builtin_ids();
        for id in 0..self.objs.len() {
            if self.objs[id]
                .as_ref()
                .is_some_and(|o| o.thread == GAME && !builtin.contains(&id))
            {
                self.drop_obj(id);
            }
        }
        for id in builtin {
            if let Some(o) = self.objs[id].as_mut()
                && o.snd.playing
            {
                o.snd = Default::default();
                self.sounds.push(SoundEvent {
                    channel: format!("snd{id}"),
                    data: None,
                    volume: 0.0,
                    repeat: false,
                    fade_ms: 0,
                });
            }
        }
        {
            let th = &mut self.threads[GAME];
            th.rep.clear();
            th.et.clear();
            th.nt.clear();
            th.results.clear();
            th.on_frame = None;
        }
        self.fx = Default::default();
        self.last_text = None;
        self.line_vars = None;
        if let Err(e) = self.start(GAME, &s.file, "@@TOP") {
            self.warn(e);
            return false;
        }
        if s.op == 0 {
            self.restore_vars(s.vars);
            self.cur = cur;
            self.refer = refer;
            return true;
        }
        self.loading = Some(crate::save::Loading {
            file: s.file,
            op: s.op,
            vars: s.vars,
        });
        let (fast, mode) = (self.fast, self.play_mode);
        let sounds_before = self.sounds.len();
        let mut n = 0;
        while self.loading.is_some() && n < MAX_SKIP {
            n += 1;
            self.fast = true;
            self.play_mode = 4;
            if let Err(e) = self.skip_frame() {
                self.warn(e);
                break;
            }
            if self.threads[GAME].frames.is_empty() || self.threads[GAME].wait == Wait::Btn {
                self.warn(format!(
                    "load {name}: the scenario stopped before the saved line"
                ));
                break;
            }
        }
        if let Some(l) = self.loading.take() {
            self.restore_vars(l.vars);
        }
        self.fast = fast;
        self.play_mode = mode;
        // of the sounds started while skipping, only the last on each channel
        let skipped: Vec<SoundEvent> = self.sounds.drain(sounds_before..).collect();
        let mut last: HashMap<String, SoundEvent> = HashMap::new();
        let mut order = Vec::new();
        for e in skipped {
            if !last.contains_key(&e.channel) {
                order.push(e.channel.clone());
            }
            last.insert(e.channel.clone(), e);
        }
        for c in order {
            if let Some(e) = last.remove(&c)
                && (e.repeat || e.data.is_none())
            {
                self.sounds.push(e);
            }
        }
        self.cur = cur;
        self.refer = refer;
        true
    }

    /// The game thread's numbers and texts, by name.
    pub(crate) fn game_vars(&self) -> Vec<(String, Var)> {
        let mut vars: Vec<(String, Var)> = self.threads[GAME]
            .globals
            .iter()
            .filter(|(_, v)| v.kind != VKind::Obj)
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        vars
    }

    /// Reached the saved line: the skip ends, the saved variables come back.
    pub(crate) fn load_reached(&mut self, file: &str, op: usize) {
        if self
            .loading
            .as_ref()
            .is_some_and(|l| l.op == op && l.file.eq_ignore_ascii_case(file))
            && let Some(l) = self.loading.take()
        {
            self.restore_vars(l.vars);
            self.fast = false;
            self.play_mode = 0;
        }
    }

    fn restore_vars(&mut self, vars: Vec<(String, Var)>) {
        let g = &mut self.threads[GAME].globals;
        for (n, v) in vars {
            // the AVG system's own state (`_...`) is as the skip left it
            if n.starts_with('_') && g.contains_key(&n) {
                continue;
            }
            match g.get_mut(&n) {
                Some(old) if old.kind == v.kind => *old = v,
                Some(_) => {}
                None => {
                    g.insert(n, v);
                }
            }
        }
    }
}

/// Year, month, day, hour, minute, second of a time in seconds since 1970 (local time is the
/// caller's: it adds its offset).
pub fn civil(secs: i64) -> [i64; 6] {
    let days = secs.div_euclid(86400);
    let s = secs.rem_euclid(86400);
    // Howard Hinnant's days_from_civil, inverted
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    [y, m, d, s / 3600, s / 60 % 60, s % 60]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil(0), [1970, 1, 1, 0, 0, 0]);
        assert_eq!(civil(1_700_000_000), [2023, 11, 14, 22, 13, 20]);
    }
}
