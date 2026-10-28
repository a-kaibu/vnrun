//! The scenario: ACT_<letter>.TXT, the text the game's own scripts (.spt) are made from and
//! which the game reads its messages from. It is read as it is:
//!
//! ```text
//! ***SC_A0100_000_A0100_010   a scene (act A, 100, 000) that goes on to A0100_010 when it ends
//! ***SS_A0100_30_02_40_50     a choice in A0100_30 between 2 answers, going to A0100_40 and _50;
//!                             the text that follows is the question, then each answer
//! EF_B0001_BG100080           a command (letters, digits and _); // starts a comment
//! 名前　（０４２１）[7]          the speaker of the next message and its voice (0: none)
//! 「text」                     the lines of a message, up to an empty line; [n] at the end of a
//!                             line is its number in the translation tables and is dropped
//! ```

use std::collections::HashMap;

/// A scene's name: act letter, then the two numbers.
pub type SceneId = (char, u32, u32);

#[derive(Debug, Clone)]
pub enum Line {
    Scene {
        id: SceneId,
        next: SceneId,
    },
    Select {
        id: SceneId,
        question: Vec<String>,
        answers: Vec<(String, SceneId)>,
    },
    Command(String),
    Message {
        name: Option<String>,
        voice: u32,
        text: Vec<String>,
    },
}

#[derive(Debug, Default)]
pub struct Act {
    pub lines: Vec<Line>,
    pub scenes: HashMap<SceneId, usize>,
    /// LB_<n> labels, by number: the line after them
    pub local: HashMap<u32, usize>,
}

/// "A0100", "000" at `parts[at..]` → ('A', 100, 0).
fn id_at(parts: &[&str], at: usize) -> Option<SceneId> {
    let head = parts.get(at)?;
    let mut c = head.chars();
    let l = c.next()?;
    if !l.is_ascii_alphabetic() {
        return None;
    }
    let n1: u32 = c.as_str().parse().ok()?;
    let n2: u32 = parts.get(at + 1)?.parse().ok()?;
    Some((l.to_ascii_uppercase(), n1, n2))
}

/// Full-width digits and others to a number.
fn number(s: &str) -> Option<u32> {
    let mut v: u32 = 0;
    let mut any = false;
    for c in s.chars() {
        let d = match c {
            '0'..='9' => c as u32 - '0' as u32,
            '０'..='９' => c as u32 - '０' as u32,
            _ => return None,
        };
        v = v.checked_mul(10)?.checked_add(d)?;
        any = true;
    }
    any.then_some(v)
}

/// "名前　（０４２１）" (and "[n]" after it): the name and the voice.
fn speaker(line: &str) -> Option<(String, u32)> {
    let s = line.trim_end();
    let s = strip_number(s);
    let s = s.strip_suffix('）')?;
    let open = s.rfind('（')?;
    let voice = number(&s[open + '（'.len_utf8()..])?;
    let name = s[..open].trim_end_matches(['　', ' ', '\t']).to_string();
    if name.is_empty() || name.starts_with('「') {
        return None;
    }
    Some((name, voice))
}

/// Drops "[123]" from the end of a line.
fn strip_number(s: &str) -> &str {
    let t = s.trim_end();
    if let Some(body) = t.strip_suffix(']')
        && let Some(open) = body.rfind('[')
        && body[open + 1..].chars().all(|c| c.is_ascii_digit())
        && open + 1 < body.len()
    {
        return body[..open].trim_end();
    }
    t
}

fn is_command(s: &str) -> bool {
    let first = s.split_whitespace().next().unwrap_or("");
    first.len() > 2
        && first.contains('_')
        && first
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        && first.as_bytes()[0].is_ascii_uppercase()
}

impl Act {
    pub fn parse(text: &str) -> Act {
        let mut act = Act::default();
        // the lines of the text block being read, and the speaker before it
        let mut block: Vec<String> = Vec::new();
        let mut who: Option<(String, u32)> = None;
        // a choice being read: its line, and the blocks after it
        let mut select: Option<(usize, usize, Vec<Vec<String>>)> = None;
        let flush = |act: &mut Act,
                     block: &mut Vec<String>,
                     who: &mut Option<(String, u32)>,
                     select: &mut Option<(usize, usize, Vec<Vec<String>>)>| {
            if block.is_empty() {
                return;
            }
            let text = std::mem::take(block);
            if let Some((_, count, blocks)) = select.as_mut()
                && blocks.len() <= *count
            {
                blocks.push(text);
                if blocks.len() > *count {
                    finish_select(act, select);
                }
                return;
            }
            let (name, voice) = match who.take() {
                Some((n, v)) => (Some(n), v),
                None => (None, 0),
            };
            act.lines.push(Line::Message { name, voice, text });
        };
        for raw in text.lines() {
            let line = raw.trim_end_matches('\r');
            let code = match line.find("//") {
                Some(i) => &line[..i],
                None => line,
            };
            let trimmed = code.trim();
            if line.trim_start().starts_with("//") {
                continue;
            }
            if trimmed.is_empty() {
                flush(&mut act, &mut block, &mut who, &mut select);
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("***") {
                flush(&mut act, &mut block, &mut who, &mut select);
                finish_select(&mut act, &mut select);
                let rest = rest.split_whitespace().next().unwrap_or("");
                let parts: Vec<&str> = rest.split('_').collect();
                match parts.first().copied() {
                    Some("SC") => {
                        if let (Some(id), Some(next)) = (id_at(&parts, 1), id_at(&parts, 3)) {
                            act.scenes.entry(id).or_insert(act.lines.len());
                            act.lines.push(Line::Scene { id, next });
                        }
                    }
                    Some("SS") => {
                        if let Some(id) = id_at(&parts, 1) {
                            let count: usize =
                                parts.get(3).and_then(|c| c.parse().ok()).unwrap_or(0);
                            let dests: Vec<SceneId> = parts[4.min(parts.len())..]
                                .iter()
                                .filter_map(|n| Some((id.0, id.1, n.parse().ok()?)))
                                .collect();
                            act.scenes.entry(id).or_insert(act.lines.len());
                            act.lines.push(Line::Select {
                                id,
                                question: Vec::new(),
                                answers: dests.into_iter().map(|d| (String::new(), d)).collect(),
                            });
                            select = Some((act.lines.len() - 1, count, Vec::new()));
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if block.is_empty() && is_command(trimmed) {
                flush(&mut act, &mut block, &mut who, &mut select);
                let cmd = trimmed.split_whitespace().next().unwrap().to_string();
                if let Some(n) = cmd.strip_prefix("LB_").and_then(|n| n.parse().ok()) {
                    act.local.insert(n, act.lines.len());
                } else {
                    act.lines.push(Line::Command(cmd));
                }
                continue;
            }
            if block.is_empty()
                && who.is_none()
                && select.is_none()
                && let Some(s) = speaker(trimmed)
            {
                who = Some(s);
                continue;
            }
            // text keeps its leading full-width space
            let t = strip_number(code.trim_end_matches(['\t', ' ']));
            block.push(t.trim_start_matches(['\t', ' ']).to_string());
        }
        flush(&mut act, &mut block, &mut who, &mut select);
        finish_select(&mut act, &mut select);
        act
    }
}

fn finish_select(act: &mut Act, select: &mut Option<(usize, usize, Vec<Vec<String>>)>) {
    let Some((at, _, blocks)) = select.take() else {
        return;
    };
    if let Some(Line::Select {
        question, answers, ..
    }) = act.lines.get_mut(at)
    {
        let mut it = blocks.into_iter();
        *question = it.next().unwrap_or_default();
        for (a, text) in answers.iter_mut().zip(it) {
            a.0 = text.join("").trim().trim_start_matches('　').to_string();
        }
    }
}

/// The text of a script file: UTF-8 when it is valid UTF-8, else Shift_JIS.
pub fn text_of(b: &[u8]) -> String {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => nexas::sjis::decode(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_messages_and_choices() {
        let act = Act::parse(
            "***SC_A0000_00_A0100_00\t//start\n\nEF_BLACK\t//black\n\n\
             由香梨　（０４２１）[3]\n「ん、ふうっ\n　あん」\n\nnarration[4]\nmore\n\n\
             ***SS_A0100_00_02_10_20 // pick\n　どちらにしますか？\n\n　由香梨\n\n　水緒\n\n\
             ***SC_A0100_10_B0150_100\nLB_10\nEF_SKIP\n",
        );
        assert!(matches!(
            act.lines[0],
            Line::Scene {
                id: ('A', 0, 0),
                next: ('A', 100, 0)
            }
        ));
        assert!(matches!(&act.lines[1], Line::Command(c) if c == "EF_BLACK"));
        match &act.lines[2] {
            Line::Message { name, voice, text } => {
                assert_eq!(name.as_deref(), Some("由香梨"));
                assert_eq!(*voice, 421);
                assert_eq!(text, &["「ん、ふうっ", "　あん」"]);
            }
            l => panic!("{l:?}"),
        }
        match &act.lines[3] {
            Line::Message { name, text, .. } => {
                assert!(name.is_none());
                assert_eq!(text, &["narration", "more"]);
            }
            l => panic!("{l:?}"),
        }
        match &act.lines[4] {
            Line::Select {
                question, answers, ..
            } => {
                assert_eq!(question, &["　どちらにしますか？"]);
                assert_eq!(answers[0], ("由香梨".to_string(), ('A', 100, 10)));
                assert_eq!(answers[1], ("水緒".to_string(), ('A', 100, 20)));
            }
            l => panic!("{l:?}"),
        }
        assert_eq!(act.scenes[&('A', 100, 10)], 5);
        assert_eq!(act.local[&10], 6);
    }
}
