//! Commands to the objects `\scp` chose.

use std::rc::Rc;

use crate::engine::{At, Engine, SoundEvent};
use crate::expr::Value;
use crate::obj::{OKind, Pic, TextBox, Tween};
use crate::script::Arg;

/// The name of a list line (`name=value`; a line without `=` is all name).
fn line_key(l: &str) -> &str {
    l.split_once('=').map_or(l, |(k, _)| k).trim()
}

fn line_value(l: &str) -> &str {
    l.split_once('=').map_or("", |(_, v)| v).trim()
}

fn file_name(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap_or(p).to_string()
}

fn change_ext(p: &str, ext: &str) -> String {
    let name_at = p.rfind(['\\', '/']).map_or(0, |i| i + 1);
    match p[name_at..].rfind('.') {
        Some(i) => format!("{}{}", &p[..name_at + i], ext),
        None => format!("{p}{ext}"),
    }
}

impl Engine {
    pub(crate) fn obj_command(&mut self, at: &At, name: &str, args: &[Arg]) -> Result<(), String> {
        let sel = self.threads[self.cur]
            .frames
            .last()
            .map(|f| f.scp.clone())
            .unwrap_or_default();
        if sel.is_empty() {
            self.warn(format!("\\{name}: no object chosen ({})", at.file()));
            return Ok(());
        }
        let v = self.eval_args(args);
        if name.starts_with('?')
            || matches!(
                name,
                "k" | "g"
                    | "gbc"
                    | "gsc"
                    | "sc"
                    | "kl"
                    | "k2"
                    | "efn"
                    | "cfe"
                    | "skp"
                    | "tt"
                    | "info"
            )
        {
            self.threads[self.cur]
                .results
                .retain(|(n, _), _| !matches!(n.as_str(), "resultint" | "resultstr" | "resultobj"));
        }
        for id in sel {
            let Some(kind) = self.objs.get(id).and_then(|o| o.as_ref()).map(|o| o.kind) else {
                continue;
            };
            match kind {
                OKind::Sys => self.sys_cmd(at, name, &v)?,
                OKind::Eff => self.eff_cmd(at, name, &v),
                OKind::Cam => self.cam_cmd(name, &v),
                OKind::Rep => self.rep_cmd(at, name, &v),
                OKind::SKey => self.skey_cmd(name, &v),
                OKind::Inp => self.inp_cmd(name, &v),
                OKind::Misc => self.misc_cmd(name, &v),
                OKind::Cfg => self.cfg_cmd(name, &v),
                OKind::Log => self.log_cmd(name, &v),
                OKind::Flags => {
                    if name.starts_with('?') || name == "k" || name == "g" {
                        // while loading, every line counts as read (the skip goes on)
                        self.res_int(0, self.loading.is_some() as i64);
                    }
                }
                OKind::Dat => self.dat_cmd(None, name, &v),
                OKind::MDat => self.dat_cmd(Some('m'), name, &v),
                OKind::TDat => self.dat_cmd(Some('t'), name, &v),
                OKind::Math => {
                    let n = |i: usize| v.get(i).map_or(0, |v| v.int());
                    let r = match name {
                        "rnd" if n(0) > 0 => (self.random() % n(0) as u64) as i64,
                        "max" => n(0).max(n(1)),
                        "min" => n(0).min(n(1)),
                        "abs" => n(0).abs(),
                        _ => 0,
                    };
                    self.res_int(0, r);
                }
                OKind::Movie => {
                    if name == "?p" {
                        self.res_int(0, 0);
                    }
                }
                OKind::Snd => self.snd_cmd(at, id, name, &v),
                OKind::List => self.list_cmd(at, id, name, &v),
                OKind::Other => {}
                _ => self.show_cmd(at, id, name, &v),
            }
        }
        Ok(())
    }

    /// Pictures, buttons, message windows.
    fn show_cmd(&mut self, at: &At, id: usize, name: &str, v: &[Value]) {
        let now = self.now;
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        let given = |i: usize| v.get(i).is_some_and(|v| !v.str().is_empty());
        match name {
            "i" => {
                if let Some(o) = self.obj(id) {
                    let (x, y, z) = (o.x, o.y, o.z);
                    o.reset();
                    (o.x, o.y, o.z) = (x, y, z);
                }
            }
            "l" | "l2" => {
                let files: Vec<String> = v.iter().map(|v| v.str()).collect();
                self.load_pic(at, id, &files);
            }
            "p4" => {
                // x, y, z, scale x, y, z, turn x, y, z, priority, time, loop
                if let Some(o) = self.obj(id) {
                    let (x, y) = (int(0) as f32, int(1) as f32);
                    let ms = int(10).max(0) as f64;
                    if ms > 0.0 {
                        let mut gx = Tween::new(o.x);
                        let mut gy = Tween::new(o.y);
                        gx.set(x, ms, now);
                        gy.set(y, ms, now);
                        o.glide = Some((gx, gy));
                    } else {
                        o.glide = None;
                        (o.x, o.y) = (x, y);
                    }
                    let sx = if given(3) { int(3) as f32 } else { 100.0 };
                    let sy = if given(4) { int(4) as f32 } else { 100.0 };
                    o.scale.0.set(sx, ms, now);
                    o.scale.1.set(sy, ms, now);
                    if given(9) {
                        o.z = int(9) as i32;
                    }
                }
            }
            "col" => {
                // opacity, multiplied colour, added colour, time, loop
                if let Some(o) = self.obj(id)
                    && given(0)
                {
                    o.alpha
                        .set(int(0).clamp(0, 255) as f32, int(3).max(0) as f64, now);
                }
            }
            "alg2" => {
                if let Some(o) = self.obj(id) {
                    o.align = (int(0) as f32 / 100.0, int(1) as f32 / 100.0);
                }
            }
            "pri" => {
                if let Some(o) = self.obj(id) {
                    o.z = int(0) as i32;
                }
            }
            "p" => {
                if let Some(o) = self.obj(id) {
                    o.glide = None;
                    if given(0) {
                        o.x = int(0) as f32;
                    }
                    if given(1) {
                        o.y = int(1) as f32;
                    }
                    if given(2) {
                        o.z = int(2) as i32;
                    }
                }
            }
            "?p" => {
                let (x, y, z) = self
                    .obj(id)
                    .map_or((0, 0, 0), |o| (o.x as i64, o.y as i64, o.z as i64));
                self.res_int(0, x);
                self.res_int(1, y);
                self.res_int(2, z);
            }
            "e" => {
                if let Some(o) = self.obj(id) {
                    o.shown = true;
                    o.hiding = false;
                    if o.play_named("Enable") {
                        o.entering = o.kind.button();
                    } else if o.kind.button() {
                        o.entering = false;
                        o.play_named("OffMouse");
                    } else if o.motion.is_none() {
                        o.play(Some(0));
                    }
                }
            }
            "d" => {
                let now_ = int(0) != 0;
                if let Some(o) = self.obj(id) {
                    if !o.shown {
                        return;
                    }
                    o.entering = false;
                    if !now_ && o.play_named("Disable") {
                        o.hiding = true;
                    } else {
                        o.shown = false;
                        o.hiding = false;
                    }
                }
            }
            "a" => {
                if let Some(o) = self.obj(id) {
                    o.alpha.set(int(0) as f32, int(1) as f64, now);
                }
            }
            "m" => {
                if let Some(o) = self.obj(id) {
                    match v.first() {
                        Some(Value::Int(n)) => o.play(Some(*n as usize)),
                        Some(Value::Str(n)) if n.trim().parse::<usize>().is_ok() => {
                            o.play(Some(n.trim().parse().unwrap()))
                        }
                        Some(Value::Str(n)) if !o.play_named(n) => o.play(Some(0)),
                        _ => {}
                    }
                }
            }
            "ps" => {
                if let Some(o) = self.obj(id) {
                    o.speed = (int(0) as f32).max(1.0);
                }
            }
            "us" => {
                if let Some(o) = self.obj(id) {
                    o.us = v.iter().map(|v| v.str()).collect();
                }
            }
            "?us" => {
                let us = self.obj(id).map(|o| o.us.clone()).unwrap_or_default();
                for (i, u) in us.into_iter().enumerate() {
                    self.res_str(i as i64, u);
                }
            }
            "v" | "mv" => {
                if let Some(o) = self.obj(id) {
                    if name == "mv" {
                        o.bar.1 = int(0).max(0);
                    } else {
                        o.bar.0 = int(0);
                    }
                    o.bar.0 = o.bar.0.clamp(0, o.bar.1.max(0));
                }
            }
            "?v" | "?mv" => {
                let b = self.obj(id).map_or((0, 0), |o| o.bar);
                self.res_int(0, if name == "?v" { b.0 } else { b.1 });
            }
            "li" | "ls" => {
                // a picture (sound) of the .b put in place: number, file
                let (n, f) = (int(0).max(0) as usize, s(1));
                if let Some(rest) = f.to_lowercase().strip_prefix("$$$saveimage,") {
                    // a save's picture, the size of the one it replaces
                    let name = rest.trim_end_matches(".bmp").to_string();
                    let Some(thumb) = self.thumbs.get(&name).cloned() else {
                        return;
                    };
                    if let Some(Pic::Anim { images, .. }) =
                        self.obj(id).and_then(|o| o.pic.as_mut())
                    {
                        let (w, h) = images
                            .get(n)
                            .and_then(|i| i.as_ref())
                            .map_or((thumb.w, thumb.h), |i| (i.w, i.h));
                        if images.len() <= n {
                            images.resize(n + 1, None);
                        }
                        images[n] = Some(Rc::new(crate::engine::resample(&thumb, w, h)));
                    }
                    return;
                }
                let Some(path) = self.resolve(at, &f) else {
                    self.warn(format!("{}: no file {f}", at.file()));
                    return;
                };
                if name == "li" {
                    let im = self.image(&path);
                    if let Some(Pic::Anim { images, .. }) =
                        self.obj(id).and_then(|o| o.pic.as_mut())
                    {
                        if images.len() <= n {
                            images.resize(n + 1, None);
                        }
                        images[n] = im;
                    }
                } else {
                    let d = self.g.read(&path).map(Rc::new);
                    if let Some(o) = self.obj(id) {
                        if o.sounds.len() <= n {
                            o.sounds.resize(n + 1, None);
                        }
                        o.sounds[n] = d;
                    }
                }
            }
            "ds" => {
                if let Some(o) = self.obj(id) {
                    o.sense = Some([int(0) as i32, int(1) as i32, int(2) as i32, int(3) as i32]);
                }
            }
            "ui" => {
                if let Some(o) = self.obj(id) {
                    o.ui = int(0);
                }
            }
            "lk" => {
                if let Some(o) = self.obj(id) {
                    o.lock = int(0) != 0;
                }
            }
            "ts" => {
                if let Some(o) = self.obj(id) {
                    let t = o.text.get_or_insert_with(TextBox::default);
                    t.rect = [int(0) as i32, int(1) as i32, int(2) as i32, int(3) as i32];
                    t.dirty = true;
                }
            }
            "o" => {
                let text = s(0);
                if let Some(o) = self.obj(id) {
                    let t = o.text.get_or_insert_with(|| TextBox {
                        rect: [0, 0, 600, 150],
                        ..TextBox::default()
                    });
                    let mut style = t.style;
                    crate::text::parse(&text, &mut style, &mut t.items);
                    t.style = style;
                    t.dirty = true;
                }
            }
            "cc" => {
                if let Some(t) = self.obj(id).and_then(|o| o.text.as_mut()) {
                    t.items.clear();
                    t.shown = 0.0;
                    t.dirty = true;
                }
            }
            "mc" => {
                if let Some(t) = self.obj(id).and_then(|o| o.text.as_mut()) {
                    t.shown = crate::text::count(&t.items) as f32;
                    t.dirty = true;
                }
            }
            "?mc" => {
                let typing = self
                    .obj(id)
                    .and_then(|o| o.text.as_ref())
                    .is_some_and(|t| t.shown < crate::text::count(&t.items) as f32);
                self.res_int(0, typing as i64);
            }
            "?wh" => {
                let (w, h) = self.obj(id).map_or((0, 0), |o| o.size(int(0) as usize));
                self.res_int(0, w as i64);
                self.res_int(1, h as i64);
            }
            "?l" => {
                let files = self.obj(id).map(|o| o.files.clone()).unwrap_or_default();
                for i in 0..6 {
                    self.res_str(i, files.get(i as usize).cloned().unwrap_or_default());
                }
            }
            "?e" => {
                let e = self.obj(id).is_some_and(|o| o.shown);
                self.res_int(0, e as i64);
            }
            "?tt" => {
                // the length of the motion
                let len = self
                    .obj(id)
                    .and_then(|o| {
                        let m = o.motion.unwrap_or(0);
                        o.abmp().map(|a| a.length(m))
                    })
                    .unwrap_or(0);
                self.res_int(0, len as i64);
            }
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                }
            }
        }
    }

    /// The log: `\o`/`\a` add a line, `\s,n,...` sets the nth newest, `\?o,n` reads it.
    fn log_cmd(&mut self, name: &str, v: &[Value]) {
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        let len = self.backlog.len();
        match name {
            "o" | "a" => {
                self.backlog.push([s(0), s(1), s(2)]);
                if self.backlog.len() > 300 {
                    self.backlog.remove(0);
                }
            }
            "i" => self.backlog.clear(),
            "s" => {
                let n = int(0);
                if n >= 0 && (n as usize) < len {
                    self.backlog[len - 1 - n as usize] = [s(1), s(2), s(3)];
                }
            }
            "?o" => {
                let n = int(0);
                let e = if n >= 0 && (n as usize) < len {
                    self.backlog[len - 1 - n as usize].clone()
                } else {
                    Default::default()
                };
                for (i, x) in e.into_iter().enumerate() {
                    self.res_str(i as i64, x);
                }
            }
            "?c" => self.res_int(0, len as i64),
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                }
            }
        }
    }

    fn load_pic(&mut self, at: &At, id: usize, files: &[String]) {
        let main = files.first().cloned().unwrap_or_default();
        let mut pic = None;
        let lower = main.to_lowercase();
        if let Some(rest) = lower.strip_prefix("$$$gamethreadimage") {
            let z = rest
                .trim_start_matches('z')
                .trim_start_matches(',')
                .trim_end_matches(".bmp");
            let im = self.game_image(crate::expr::str_int(z) as i32);
            pic = Some(Pic::Still(Rc::new(im)));
        } else if !main.is_empty() {
            match self.resolve(at, &main) {
                None => self.warn(format!("{}: no picture {main}", at.file())),
                Some(path) if path.to_lowercase().ends_with(".b") => {
                    if let Some(a) = self.abmp(&path) {
                        let n = a.images.len().max(files.len().saturating_sub(1));
                        let mut images = Vec::with_capacity(n);
                        for i in 0..n {
                            let ext = files.get(i + 1).filter(|f| !f.is_empty()).cloned();
                            let im = match ext {
                                Some(f) => match self.resolve(at, &f) {
                                    Some(p) => self.image(&p),
                                    None => {
                                        self.warn(format!("{}: no picture {f}", at.file()));
                                        None
                                    }
                                },
                                None => a
                                    .images
                                    .get(i)
                                    .and_then(|b| crate::engine::decode_image(b))
                                    .map(Rc::new),
                            };
                            images.push(im);
                        }
                        pic = Some(Pic::Anim { abmp: a, images });
                    }
                }
                Some(path) => {
                    if let Some(im) = self.image(&path) {
                        pic = Some(Pic::Still(im));
                    } else {
                        self.warn(format!("{path}: not a picture"));
                    }
                }
            }
        }
        if let Some(o) = self.obj(id) {
            o.files = files.to_vec();
            let anim = matches!(pic, Some(Pic::Anim { .. }));
            o.pic = pic;
            o.motion = None;
            o.heads.clear();
            if anim {
                if o.shown {
                    o.play_named("Enable");
                } else {
                    o.play(Some(0));
                }
            }
        }
    }

    fn snd_cmd(&mut self, at: &At, id: usize, name: &str, v: &[Value]) {
        let now = self.now;
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        let channel = format!("snd{id}");
        match name {
            "i" => {
                let playing = self.obj(id).is_some_and(|o| o.snd.playing);
                if let Some(o) = self.obj(id) {
                    o.snd = Default::default();
                }
                if playing {
                    self.push_sound(SoundEvent {
                        channel,
                        data: None,
                        volume: 0.0,
                        repeat: false,
                        fade_ms: 0,
                    });
                }
            }
            "l" => {
                let f = v.first().map(|v| v.str()).unwrap_or_default();
                let path = self.resolve(at, &f);
                let data = path.as_ref().and_then(|p| self.g.read(p)).map(Rc::new);
                if data.is_none() {
                    self.warn(format!("no sound {f}"));
                }
                if let Some(o) = self.obj(id) {
                    o.snd.file = f;
                    o.snd.data = data;
                }
            }
            "v" => {
                let (vol, ms) = (int(0) as f32, int(1) as f64);
                let ev = self.obj(id).and_then(|o| {
                    o.snd.volume.set(vol, ms, now);
                    o.snd.playing.then(|| (o.snd.data.clone(), o.snd.repeat))
                });
                if let Some((data, repeat)) = ev {
                    self.push_sound(SoundEvent {
                        channel,
                        data: if vol <= 0.0 && ms <= 0.0 { None } else { data },
                        volume: (vol / 255.0).clamp(0.0, 2.0),
                        repeat,
                        fade_ms: ms as u32,
                    });
                    if vol <= 0.0
                        && let Some(o) = self.obj(id)
                    {
                        o.snd.playing = false;
                    }
                }
            }
            "p" => {
                let repeat = int(0) != 0;
                let ev = self.obj(id).and_then(|o| {
                    o.snd.repeat = repeat;
                    o.snd.playing = o.snd.data.is_some();
                    let vol = o.snd.volume.at(now);
                    o.snd
                        .data
                        .clone()
                        .map(|d| (d, (vol / 255.0).clamp(0.0, 2.0), o.snd.volume))
                });
                if let Some((data, vol, tw)) = ev {
                    let left = (tw.start + tw.ms - now).max(0.0);
                    // fading in: start at the current volume, then go on to the target
                    self.push_sound(SoundEvent {
                        channel: channel.clone(),
                        data: Some(data.clone()),
                        volume: vol,
                        repeat,
                        fade_ms: 0,
                    });
                    if left > 0.0 {
                        self.push_sound(SoundEvent {
                            channel,
                            data: Some(data),
                            volume: (tw.to / 255.0).clamp(0.0, 2.0),
                            repeat,
                            fade_ms: left as u32,
                        });
                    }
                }
            }
            "?p" => {
                let p = self.obj(id).is_some_and(|o| o.snd.playing && o.snd.repeat);
                self.res_int(0, p as i64);
            }
            "tt" | "?tt" => self.res_int(0, 0),
            _ => {}
        }
    }

    fn list_cmd(&mut self, at: &At, id: usize, name: &str, v: &[Value]) {
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        let other = |e: &mut Engine, i: usize| -> Vec<String> {
            match v.get(i) {
                Some(Value::Obj(o)) => e.obj(*o).map(|o| o.list.clone()).unwrap_or_default(),
                _ => Vec::new(),
            }
        };
        let list = match self.obj(id) {
            Some(o) => std::mem::take(&mut o.list),
            None => return,
        };
        let mut list = list;
        let find =
            |l: &[String], k: &str| l.iter().position(|x| line_key(x).eq_ignore_ascii_case(k));
        match name {
            "i" => list.clear(),
            "l" => {
                let f = s(0);
                let text = self
                    .resolve(at, &f)
                    .and_then(|p| self.g.read(&p))
                    .map(|b| crate::script::decode(&b))
                    .unwrap_or_default();
                list = text
                    .lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
            }
            "a" => list.push(s(0)),
            "ad" => {
                let (sep, text) = (s(0), s(1));
                if sep.is_empty() {
                    list.push(text);
                } else {
                    list.extend(text.split(sep.as_str()).map(String::from));
                }
            }
            "v" if s(0).trim().is_empty() => {}
            "v" => {
                let line = format!("{}={}", s(0), s(1));
                match find(&list, &s(0)) {
                    Some(i) => list[i] = line,
                    None => list.push(line),
                }
            }
            "s" => match v.first() {
                Some(Value::Int(i)) if *i >= 0 => {
                    let i = *i as usize;
                    if list.len() <= i {
                        list.resize(i + 1, String::new());
                    }
                    list[i] = s(1);
                }
                _ => {
                    let line = format!("{}={}", s(0), s(1));
                    match find(&list, &s(0)) {
                        Some(i) => list[i] = line,
                        None => list.push(line),
                    }
                }
            },
            "d" => {
                let at = match v.first() {
                    Some(Value::Int(i)) => Some(*i as usize),
                    _ => find(&list, &s(0)),
                };
                if let Some(i) = at.filter(|i| *i < list.len()) {
                    list.remove(i);
                }
            }
            "dd" => {
                let i = v.first().map_or(0, |v| v.int());
                if i >= 0 && (i as usize) < list.len() {
                    list.remove(i as usize);
                }
            }
            "comb" => {
                for line in other(self, 0) {
                    match find(&list, line_key(&line)) {
                        Some(i) => list[i] = line,
                        None => list.push(line),
                    }
                }
            }
            "asn" => list = other(self, 0),
            "sort" => list.sort_by_key(|l| line_key(l).to_lowercase()),
            "?c" => self.res_int(0, list.len() as i64),
            "?s" | "?n" => {
                let i = v.first().map_or(0, |v| v.int());
                let line = usize::try_from(i)
                    .ok()
                    .and_then(|i| list.get(i))
                    .cloned()
                    .unwrap_or_default();
                let out = if name == "?n" {
                    line_key(&line).to_string()
                } else {
                    line
                };
                self.res_int(0, crate::expr::str_int(&out));
                self.res_str(0, out);
            }
            "?i" => {
                let i = find(&list, &s(0)).map_or(-1, |i| i as i64);
                self.res_int(0, i);
            }
            "?v" => {
                let val = match find(&list, &s(0)) {
                    Some(i) => line_value(&list[i]).to_string(),
                    None => s(1),
                };
                self.res_int(0, crate::expr::str_int(&val));
                self.res_str(0, val);
            }
            "bpos" => {
                // the first of the names found in the text: the name, where (from 1)
                let text = s(0);
                let best = list
                    .iter()
                    .map(|l| line_key(l))
                    .filter(|k| !k.is_empty())
                    .filter_map(|k| text.find(k).map(|i| (i, k)))
                    .min_by_key(|(i, k)| (*i, std::cmp::Reverse(k.len())));
                let (pos, key) = best.map_or((0, String::new()), |(i, k)| {
                    (text[..i].chars().count() as i64 + 1, k.to_string())
                });
                self.res_int(0, pos);
                self.res_str(0, key);
            }
            "brep" => {
                // every name in the text becomes its value, the longest names first
                let mut pairs: Vec<(&str, &str)> = list
                    .iter()
                    .map(|l| (line_key(l), line_value(l)))
                    .filter(|(k, _)| !k.is_empty())
                    .collect();
                pairs.sort_by_key(|(k, _)| std::cmp::Reverse(k.chars().count()));
                let text = s(0);
                let mut out = String::new();
                let mut rest = text.as_str();
                'next: while let Some(c) = rest.chars().next() {
                    for (k, val) in &pairs {
                        if rest.starts_with(k) {
                            out += val;
                            rest = &rest[k.len()..];
                            continue 'next;
                        }
                    }
                    out.push(c);
                    rest = &rest[c.len_utf8()..];
                }
                self.res_str(0, out);
            }
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                    self.res_str(0, String::new());
                }
            }
        }
        if let Some(o) = self.obj(id) {
            o.list = list;
        }
    }

    fn sys_cmd(&mut self, at: &At, name: &str, v: &[Value]) -> Result<(), String> {
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        match name {
            "e" => {
                self.warn("the game asked to quit".into());
                self.ended = true;
            }
            "r" => {
                let label = s(0);
                let file = s(1);
                if label.is_empty() {
                    self.reset("startup.s", "@@RESET");
                } else {
                    let file = if file.is_empty() {
                        at.unit.files[0].clone()
                    } else {
                        self.resolve(at, &file).unwrap_or(file)
                    };
                    self.reset(&file, &label);
                }
            }
            "c" | "l" | "s" => {
                let l = s(0);
                if l.starts_with("@@") {
                    self.system_label(&l);
                }
            }
            "p" => self.play_mode = int(0),
            "?p" => self.res_int(0, self.play_mode),
            "skp" => self.res_int(0, 0),
            "?n" => {
                for i in 0..4 {
                    let t = self.savetext[i].clone();
                    self.res_str(i as i64, t);
                }
            }
            "n" => {
                for i in 0..4 {
                    self.savetext[i] = s(i);
                }
            }
            "eve" => {
                let (ev, label) = (s(0), s(1));
                if ev.eq_ignore_ascii_case("onframe") {
                    let t = self.cur;
                    self.threads[t].on_frame = if label.is_empty() {
                        None
                    } else {
                        at.unit.label(&label).map(|pc| (at.unit.clone(), pc))
                    };
                }
            }
            "?mrd" => {
                // the file and label of a caller
                let n = int(0).max(0) as usize;
                let t = self.cur;
                let frames = &self.threads[t].frames;
                let f = frames.len().checked_sub(1 + n).and_then(|i| frames.get(i));
                let (file, line) = f.map_or((String::new(), 0), |f| {
                    let op = f.pc.saturating_sub(1);
                    let o = &f.unit.ops[op.min(f.unit.ops.len().saturating_sub(1))];
                    (f.unit.files[0].clone(), o.line as i64)
                });
                self.res_str(0, file);
                self.res_str(1, String::new());
                self.res_int(0, line);
            }
            "?sr" => self.res_str(0, String::new()),
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                }
            }
        }
        Ok(())
    }

    fn eff_cmd(&mut self, at: &At, name: &str, v: &[Value]) {
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        match name {
            "c" => self.capture(int(0) as i32),
            "i" => {
                self.fx.snap = None;
                self.fx.trans = None;
            }
            "bg" => {
                let c = int(0);
                self.bg = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
            }
            "fdi" => {
                let kind = int(0);
                let ms = int(1) as f64;
                if kind == 2 {
                    // from a colour
                    let c = int(2);
                    let col = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                    self.fx.cover = Some((col, 1.0, 0.0, self.now, ms));
                    return;
                }
                if self.fx.snap.is_none() {
                    return;
                }
                let mask = if kind == 3 {
                    None
                } else {
                    let m = s(2);
                    if m.is_empty() {
                        None
                    } else {
                        self.resolve(at, &m).and_then(|p| self.image(&p))
                    }
                };
                let vague = if v.get(4).is_some_and(|v| !v.str().is_empty()) {
                    (int(4) as f32 * 16.0).max(8.0)
                } else {
                    64.0
                };
                self.fx.trans = Some((self.now, ms, mask, vague));
                if ms <= 0.0 {
                    self.fx.snap = None;
                    self.fx.trans = None;
                }
            }
            "fdo" => {
                let ms = int(1) as f64;
                let c = int(2);
                let col = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                self.fx.cover = Some((col, 0.0, 1.0, self.now, ms));
            }
            _ => {}
        }
    }

    fn cam_cmd(&mut self, name: &str, v: &[Value]) {
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        match name {
            "mm" => self.fx.quake_px = (int(0) as f32).max(2.0),
            "p" => {
                let ms = int(2) as f64;
                if self.fx.quake_px > 0.0 && ms > 0.0 {
                    self.fx.quake = self.now + ms;
                }
            }
            "i" => {
                self.fx.quake = 0.0;
                self.fx.quake_px = 0.0;
            }
            _ => {}
        }
    }

    fn rep_cmd(&mut self, at: &At, name: &str, v: &[Value]) {
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        let t = self.cur;
        let tpl = match name {
            "a" => s(1),
            "et" | "nt" => s(0),
            _ => String::new(),
        };
        if !tpl.is_empty() {
            self.tpl_units.insert(tpl, at.unit.clone());
        }
        match name {
            "a" => {
                self.threads[t].rep.insert(s(0).to_lowercase(), s(1));
            }
            "d" => {
                self.threads[t].rep.remove(&s(0).to_lowercase());
            }
            "?r" => {
                // a story line's commands: its template with the line's arguments
                let line = s(0);
                let out = line
                    .strip_prefix('^')
                    .and_then(|l| {
                        let (n, rest) = l.split_once(',').unwrap_or((l, ""));
                        let key = format!("^{}", n.trim().to_lowercase());
                        let tpl = self.threads[t].rep.get(&key)?.clone();
                        Some(crate::engine::expand(&tpl, rest))
                    })
                    .unwrap_or(line);
                self.res_str(0, out);
            }
            "et" => self.threads[t].et = s(0),
            "nt" => self.threads[t].nt = s(0),
            _ => {}
        }
    }

    fn skey_cmd(&mut self, name: &str, v: &[Value]) {
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        match name {
            "i" => self.skey.clear(),
            "a" => {
                let code = v.first().map_or(0, |v| v.int());
                self.skey.retain(|(c, _, _)| *c != code);
                self.skey.push((code, s(1), s(2)));
            }
            _ => {}
        }
    }

    fn inp_cmd(&mut self, name: &str, v: &[Value]) {
        match name {
            "gbc" | "gsc" => {
                let b = if name == "gbc" {
                    self.take_button()
                } else {
                    None
                };
                match b {
                    Some(id) => {
                        let (us, ui) = self
                            .obj(id)
                            .map(|o| (o.us.clone(), o.ui))
                            .unwrap_or_default();
                        self.set_result("resultobj", 0, Value::Obj(id));
                        for i in 0..4 {
                            self.res_str(i, us.get(i as usize).cloned().unwrap_or_default());
                        }
                        self.res_int(0, ui);
                    }
                    None => self.set_result("resultobj", 0, Value::Null),
                }
            }
            "sc" | "?sc" => {
                let c = self.system_click();
                self.res_int(0, c as i64);
            }
            "kl" | "k2" | "k" => {
                let fired = self.keys_fired();
                let r = if name == "kl" {
                    fired.first().copied().unwrap_or(0)
                } else {
                    v.iter()
                        .map(|v| v.int())
                        .find(|c| fired.iter().any(|f| f & 0xfffff == c & 0xfffff))
                        .unwrap_or(0)
                };
                self.res_int(0, r);
            }
            "?mp" => {
                let (x, y) = self.mouse;
                self.res_int(0, x as i64);
                self.res_int(1, y as i64);
            }
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                }
            }
        }
    }

    fn misc_cmd(&mut self, name: &str, v: &[Value]) {
        let s = |i: usize| v.get(i).map(|v| v.str()).unwrap_or_default();
        match name {
            "efn" => self.res_str(0, file_name(&s(0))),
            "cfe" => self.res_str(0, change_ext(&s(0), &s(1))),
            "info" => self.res_str(0, String::new()),
            _ => {}
        }
    }

    fn cfg_cmd(&mut self, name: &str, v: &[Value]) {
        let int = |i: usize| v.get(i).map_or(0, |v| v.int());
        match name {
            "fsc2" => {
                let c = int(1);
                self.font
                    .edges
                    .insert(int(0) as u8, [(c >> 16) as u8, (c >> 8) as u8, c as u8]);
            }
            "?cvv" | "?vtv" | "?vv" | "?vs" | "?vb" => self.res_int(0, 255),
            "?md" => self.res_int(0, 1),
            _ => {
                if name.starts_with('?') {
                    self.res_int(0, 0);
                }
            }
        }
    }
}

/// `\bsub`'s replacement: `$1` and `\1` name groups.
pub fn regex_replace(text: &str, pat: &str, rep: &str, all: bool) -> Option<String> {
    let re = regex::Regex::new(pat).ok()?;
    let mut r = String::new();
    let cs: Vec<char> = rep.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if (cs[i] == '$' || cs[i] == '\\') && cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            r += &format!("${{{}}}", cs[i + 1]);
            i += 2;
        } else if cs[i] == '$' {
            r += "$$";
            i += 1;
        } else {
            r.push(cs[i]);
            i += 1;
        }
    }
    Some(if all {
        re.replace_all(text, r.as_str()).into_owned()
    } else {
        re.replace(text, r.as_str()).into_owned()
    })
}
