//! Our own save slots. The scripts' state is inside Lua coroutines, which cannot be written
//! out, so a slot holds what the player did since the game started (each page clicked away,
//! the title button, each choice) and the page that was showing; loading starts the game again
//! and replays those inputs with no waiting and no sound until that page shows.

pub const SLOTS: usize = 12;

pub fn slot_name(k: usize) -> String {
    format!("debsave{k:02}.txt")
}

/// What the player did to go on from a script's wait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// a page clicked away
    Page,
    /// a title button, by its code
    Title(i32),
    /// a choice, by its number
    Choice(usize),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveData {
    pub inputs: Vec<Input>,
    pub name: String,
    pub text: String,
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

impl SaveData {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = String::from("debsave\t1\n");
        o += &format!("name\t{}\n", clean(&self.name));
        o += &format!("text\t{}\n", clean(&self.text));
        // runs of pages as one line: "p <count>"
        let mut i = 0;
        while i < self.inputs.len() {
            match self.inputs[i] {
                Input::Page => {
                    let n = self.inputs[i..]
                        .iter()
                        .take_while(|x| **x == Input::Page)
                        .count();
                    o += &format!("p\t{n}\n");
                    i += n;
                    continue;
                }
                Input::Title(c) => o += &format!("t\t{c}\n"),
                Input::Choice(k) => o += &format!("c\t{k}\n"),
            }
            i += 1;
        }
        o.into_bytes()
    }

    pub fn decode(b: &[u8]) -> Option<SaveData> {
        let t = std::str::from_utf8(b).ok()?;
        let mut lines = t.lines();
        if !lines.next()?.starts_with("debsave\t") {
            return None;
        }
        let mut d = SaveData::default();
        for l in lines {
            let f: Vec<&str> = l.split('\t').collect();
            let n = || f.get(1).and_then(|s| s.parse::<i64>().ok());
            match f[0] {
                "name" => d.name = f.get(1).copied().unwrap_or("").to_string(),
                "text" => d.text = f.get(1).copied().unwrap_or("").to_string(),
                "p" => d
                    .inputs
                    .extend(std::iter::repeat_n(Input::Page, n()?.max(0) as usize)),
                "t" => d.inputs.push(Input::Title(n()? as i32)),
                "c" => d.inputs.push(Input::Choice(n()?.max(0) as usize)),
                _ => {}
            }
        }
        Some(d)
    }

    /// What the slot shows: the speaker and the start of the text.
    pub fn summary(&self) -> String {
        let text: String = self.text.replace('\u{3000}', "").chars().take(22).collect();
        if self.name.is_empty() {
            text
        } else {
            format!("{}「{}", self.name, text.trim_start_matches('「'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = SaveData {
            inputs: vec![
                Input::Page,
                Input::Page,
                Input::Title(0),
                Input::Page,
                Input::Choice(2),
                Input::Page,
                Input::Page,
                Input::Page,
            ],
            name: "紫".into(),
            text: "「えーっと\tここを」".into(),
        };
        let back = SaveData::decode(&d.encode()).unwrap();
        assert_eq!(back.inputs, d.inputs);
        assert_eq!(back.text, "「えーっと ここを」");
        assert_eq!(back.summary(), "紫「えーっと ここを」");
        assert!(SaveData::decode(b"aghsave\t1\n").is_none());
    }
}
