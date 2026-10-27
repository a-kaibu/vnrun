//! Saved games. A save holds the game as it was when the page being read began: the
//! variables, the components, the backlog and where the main chart is. Handlers that were
//! running (the save menu itself among them) are not kept.

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct State {
    caption: String,
    now: f64,
    seq: u64,
    globals: Vec<(String, Var)>,
    objs: Vec<SavedObj>,
    main: Vec<SavedFrame>,
    started: bool,
    deadline: f64,
    history: Vec<String>,
    hist_line: i32,
}

#[derive(Serialize, Deserialize)]
struct SavedObj {
    kind: u8,
    props: Vec<(u8, Value)>,
    seq: u64,
    mes: Option<SavedMes>,
    items: Vec<MenuItem>,
    disabled: Vec<bool>,
    t0: f64,
    playing: bool,
    cab: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SavedMes {
    queue: Vec<Glyph>,
    chars: Vec<(char, i32, i32)>,
    x: i32,
    y: i32,
    indent: i32,
    waiting: bool,
    text: String,
    hist: bool,
}

#[derive(Serialize, Deserialize)]
struct SavedFrame {
    page: String,
    pc: usize,
    params: Vec<Value>,
    locals: Vec<(String, Var)>,
    result: Value,
    result_var: String,
    sender: String,
}

/// The file of save slot `no`.
fn slot_file(no: i64) -> String {
    format!("livemaker-{no}.json")
}

const CAPTIONS: &str = "livemaker-captions.json";

impl Engine {
    /// Only the main chart and idle handlers are running: a page can be saved.
    fn quiet(&self) -> bool {
        self.threads
            .iter()
            .all(|t| t.main || t.done || t.key.as_ref().is_some_and(|k| k.1 == ONIDLE))
    }

    /// Keeps the state as the page in message box `i` begins.
    pub(super) fn checkpoint(&mut self, i: usize) {
        let starts = self.objs[i].mes.as_ref().is_some_and(|m| {
            m.hist && m.chars.is_empty() && matches!(m.queue.front(), Some(Glyph::Char { .. }))
        });
        if !starts || !self.quiet() {
            return;
        }
        let Some(main) = self.threads.iter().find(|t| t.main && !t.done) else {
            return;
        };
        let main = main
            .frames
            .iter()
            .map(|f| SavedFrame {
                page: f.page.clone(),
                pc: f.pc,
                params: f.params.clone(),
                locals: f
                    .locals
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                result: f.result.clone(),
                result_var: f.result_var.clone(),
                sender: f.sender.clone(),
            })
            .collect();
        let started = self.threads.iter().any(|t| t.main && t.started);
        let deadline = self
            .threads
            .iter()
            .find(|t| t.main)
            .map_or(0.0, |t| t.deadline);
        let objs = self
            .objs
            .iter()
            .map(|o| SavedObj {
                kind: o.kind,
                props: o.props.iter().map(|(k, v)| (*k, v.clone())).collect(),
                seq: o.seq,
                mes: o.mes.as_ref().map(|m| SavedMes {
                    queue: m.queue.iter().cloned().collect(),
                    chars: m.chars.clone(),
                    x: m.x,
                    y: m.y,
                    indent: m.indent,
                    waiting: m.waiting,
                    text: m.text.clone(),
                    hist: m.hist,
                }),
                items: o.items.clone(),
                disabled: o.disabled.clone(),
                t0: o.t0,
                playing: o.playing,
                cab: o.cab.clone(),
            })
            .collect();
        self.checkpoint = Some(State {
            caption: String::new(),
            now: self.now,
            seq: self.seq,
            globals: self
                .globals
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            objs,
            main,
            started,
            deadline,
            history: self.history.clone(),
            hist_line: self.hist_line,
        });
    }

    pub(super) fn game_save(&mut self, no: i64, caption: String) {
        let Some(st) = self.checkpoint.as_mut() else {
            eprintln!("livemaker: nothing to save yet");
            return;
        };
        st.caption = caption.clone();
        match serde_json::to_vec(st) {
            Ok(b) => self.files.write_save(&slot_file(no), &b),
            Err(e) => eprintln!("livemaker: save {no}: {e}"),
        }
        self.captions();
        self.captions.insert(no, caption);
        let list: Vec<(i64, String)> = self.captions.iter().map(|(k, v)| (*k, v.clone())).collect();
        if let Ok(b) = serde_json::to_vec(&list) {
            self.files.write_save(CAPTIONS, &b);
        }
    }

    /// Reads a save; it takes over once the frame's charts have run.
    pub(super) fn game_load(&mut self, no: i64) {
        let st = self
            .files
            .read_save(&slot_file(no))
            .and_then(|b| serde_json::from_slice::<State>(&b).ok());
        match st {
            Some(st) => self.pending_load = Some(st),
            None => eprintln!("livemaker: save {no}: missing"),
        }
    }

    pub(super) fn captions(&mut self) -> &HashMap<i64, String> {
        if !self.captions_read {
            self.captions_read = true;
            if let Some(list) = self
                .files
                .read_save(CAPTIONS)
                .and_then(|b| serde_json::from_slice::<Vec<(i64, String)>>(&b).ok())
            {
                self.captions.extend(list);
            }
        }
        &self.captions
    }

    pub(super) fn restore(&mut self, st: State) {
        for i in 0..self.objs.len() {
            if self.objs[i].playing && matches!(self.objs[i].kind, lsb::SOUND | lsb::MOVIE) {
                self.stop(i, 0);
            }
        }
        self.objs.clear();
        self.threads.clear();
        self.motions.clear();
        self.flip = None;
        self.now = st.now;
        self.seq = st.seq;
        self.globals = st.globals.into_iter().collect();
        self.history = st.history;
        self.hist_line = st.hist_line;
        for so in st.objs {
            let mut o = Obj::new(so.kind, "", so.seq);
            o.props = so.props.iter().cloned().collect();
            o.mes = so.mes.map(|m| {
                Box::new(Mes {
                    queue: m.queue.into(),
                    chars: m.chars,
                    x: m.x,
                    y: m.y,
                    indent: m.indent,
                    waiting: m.waiting,
                    text: m.text,
                    hist: m.hist,
                    dirty: true,
                    ..Mes::default()
                })
            });
            o.cab = so.cab;
            self.objs.push(o);
            let i = self.objs.len() - 1;
            let src = self.objs[i].str(SOURCE);
            if !src.is_empty() {
                self.load_source(i, &src);
            }
            let o = &mut self.objs[i];
            o.props = so.props.into_iter().collect();
            o.items = so.items;
            o.disabled = so.disabled;
            o.t0 = so.t0;
            match o.kind {
                lsb::MENU => self.menu_setup(i),
                lsb::SOUND | lsb::MOVIE if so.playing => self.play(i),
                lsb::TIMER => self.objs[i].playing = so.playing,
                _ => {}
            }
        }
        let mut frames = Vec::new();
        for f in st.main {
            let Some(script) = self.script(&f.page) else {
                eprintln!("livemaker: save: {} is missing", f.page);
                return;
            };
            frames.push(Frame {
                page: f.page,
                script,
                pc: f.pc,
                params: f.params,
                locals: f.locals.into_iter().collect(),
                result: f.result,
                result_var: f.result_var,
                sender: f.sender,
            });
        }
        self.threads.push(Thread {
            frames,
            started: st.started,
            deadline: st.deadline,
            main: true,
            ..Thread::default()
        });
        self.checkpoint = None;
    }
}
