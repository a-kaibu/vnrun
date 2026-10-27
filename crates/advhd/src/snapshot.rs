//! Serializable scenario state. The Lua interface and in-flight menu are not part of the scene.
//! Loading must recreate those separately before exposing save slots to the player.

use crate::layers::{Content, Effect, Layer, Params};
use crate::state::{FLAGS, LoopSound, State, VARS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Normal save slots use a fixed, path-safe name separate from the game's files.
pub fn slot_name(slot: i64) -> Option<String> {
    (1..=9999)
        .contains(&slot)
        .then(|| format!("slot-{slot:04}.json"))
}

pub fn slot_number(name: &str) -> Option<i64> {
    let digits = name.strip_prefix("slot-")?.strip_suffix(".json")?;
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let slot = digits.parse().ok()?;
    slot_name(slot).map(|_| slot)
}

#[derive(Serialize, Deserialize)]
pub struct ScenarioState {
    now: f64,
    flags: Vec<bool>,
    vars: Vec<f32>,
    strings: HashMap<String, String>,
    #[serde(default)]
    loops: HashMap<String, LoopSound>,
    layers: Vec<LayerState>,
}

#[derive(Serialize, Deserialize)]
struct LayerState {
    name: String,
    file: String,
    visible: bool,
    hit: bool,
    params: Params,
    effects: Vec<Effect>,
    parent: Option<String>,
    subs: Vec<SubState>,
}

#[derive(Serialize, Deserialize)]
struct SubState {
    id: i32,
    visible: bool,
    hit: bool,
    params: Params,
    effects: Vec<Effect>,
}

impl ScenarioState {
    pub fn capture(state: &State) -> Result<Self, String> {
        let scene = state
            .screen
            .sets
            .iter()
            .find(|set| set.name.is_empty())
            .ok_or("missing scenario set")?;
        if scene.snap.is_some() {
            return Err("cannot save during a transition".into());
        }
        let mut layers = Vec::new();
        for layer in &scene.layers {
            if matches!(layer.content, Content::Text(_))
                || (layer.file.is_empty() && !matches!(layer.content, Content::None))
            {
                return Err(format!("cannot save scenario layer {}", layer.name));
            }
            layers.push(LayerState {
                name: layer.name.clone(),
                file: layer.file.clone(),
                visible: layer.visible,
                hit: layer.hit,
                params: layer.p,
                effects: layer.effects.clone(),
                parent: layer.parent.clone(),
                subs: layer
                    .subs
                    .iter()
                    .map(|s| SubState {
                        id: s.id,
                        visible: s.visible,
                        hit: s.hit,
                        params: s.p,
                        effects: s.effects.clone(),
                    })
                    .collect(),
            });
        }
        Ok(Self {
            now: state.now,
            flags: state.flags.clone(),
            vars: state.vars.clone(),
            strings: state.strings.clone(),
            loops: state.active_loops.clone(),
            layers,
        })
    }

    /// Rebuild all layers before changing the live scenario, so missing assets cannot partially load.
    pub fn restore(self, state: &mut State) -> Result<(), String> {
        if self.flags.len() != FLAGS || self.vars.len() != VARS {
            return Err("invalid scenario variables".into());
        }
        let mut layers = Vec::with_capacity(self.layers.len());
        for saved in self.layers {
            let mut layer = if saved.file.is_empty() {
                Layer::new(&saved.name, Content::None)
            } else {
                state.picture_layer(&saved.name, &saved.file)
            };
            if !saved.file.is_empty() && matches!(layer.content, Content::None) {
                return Err(format!("missing scenario picture {}", saved.file));
            }
            if layer.subs.len() != saved.subs.len() {
                return Err(format!("changed scenario picture {}", saved.file));
            }
            for (sub, old) in layer.subs.iter_mut().zip(saved.subs) {
                if sub.id != old.id {
                    return Err(format!("changed scenario picture {}", saved.file));
                }
                sub.visible = old.visible;
                sub.hit = old.hit;
                sub.p = old.params;
                sub.effects = old.effects;
            }
            layer.visible = saved.visible;
            layer.hit = saved.hit;
            layer.p = saved.params;
            layer.effects = saved.effects;
            layer.parent = saved.parent;
            layers.push(layer);
        }
        let scene = state.screen.set("").ok_or("missing scenario set")?;
        scene.layers = layers;
        scene.snap = None;
        state.now = self.now;
        state.flags = self.flags;
        state.vars = self.vars;
        state.strings = self.strings;
        state.stop("*", 0.0);
        for (channel, sound) in self.loops {
            state.play(&channel, &sound.file, true, sound.volume, 0.0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::Files;

    struct Empty;
    impl Files for Empty {
        fn size(&mut self, _: &str) -> Option<u64> {
            None
        }
        fn read_range(&mut self, _: &str, _: u64, _: usize) -> Option<Vec<u8>> {
            None
        }
        fn list(&mut self) -> Vec<String> {
            Vec::new()
        }
        fn read_save(&mut self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn write_save(&mut self, _: &str, _: &[u8]) {}
    }

    #[test]
    fn slot_names_are_bounded_and_canonical() {
        assert_eq!(slot_name(1).as_deref(), Some("slot-0001.json"));
        assert_eq!(slot_number("slot-9999.json"), Some(9999));
        assert_eq!(slot_number("slot-0000.json"), None);
        assert_eq!(slot_number("../slot-0001.json"), None);
        assert_eq!(slot_name(10000), None);
    }

    #[test]
    fn roundtrip_restores_variables_layers_and_timeline() {
        let mut state = State::new(Box::new(Empty), 1280, 720);
        state
            .screen
            .add("")
            .layers
            .push(Layer::new("bg", Content::None));
        state.screen.set("").unwrap().layers[0].p.pos = [12.0, 34.0, 0.0];
        state.flags[9] = true;
        state.vars[7] = 1.5;
        state.now = 1234.0;
        state.strings.insert("name".into(), "テスト".into());
        let data = serde_json::to_vec(&ScenarioState::capture(&state).unwrap()).unwrap();
        state.screen.set("").unwrap().layers.clear();
        state.flags[9] = false;
        state.vars[7] = 0.0;
        state.now = 0.0;
        state.strings.clear();
        serde_json::from_slice::<ScenarioState>(&data)
            .unwrap()
            .restore(&mut state)
            .unwrap();
        assert!(state.flags[9]);
        assert_eq!(state.vars[7], 1.5);
        assert_eq!(state.now, 1234.0);
        assert_eq!(state.strings["name"], "テスト");
        assert_eq!(
            state.screen.set("").unwrap().layers[0].p.pos,
            [12.0, 34.0, 0.0]
        );
    }

    #[test]
    fn missing_assets_do_not_partially_replace_the_scene() {
        let mut state = State::new(Box::new(Empty), 1280, 720);
        state
            .screen
            .add("")
            .layers
            .push(Layer::new("old", Content::None));
        state.flags[0] = true;
        let mut saved = ScenarioState::capture(&state).unwrap();
        saved.layers[0].file = "missing.png".into();
        saved.flags[0] = false;
        assert!(saved.restore(&mut state).is_err());
        assert!(state.flags[0]);
        assert_eq!(state.screen.set("").unwrap().layers[0].name, "old");
    }
}
