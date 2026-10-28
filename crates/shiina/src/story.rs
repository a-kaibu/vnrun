//! The story files (`A001.TXT` ... in the scenario archive): Shift-JIS text that the game's
//! .SCN code reads a line at a time. `;` starts a comment (`;￥ジャンプ＜a002＞へ` names the
//! next file), `$NAME,arg,...` is a command, `【name】` names the speaker of the page that
//! follows, and a run of other lines is one page of text.

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Cmd(String, Vec<String>),
    /// the speaker, then the page's lines
    Page(Option<String>, Vec<String>),
    /// the next story file
    Jump(String),
}

pub fn parse(b: &[u8]) -> Vec<Item> {
    let text = encoding_rs::SHIFT_JIS.decode(b).0;
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut page: Vec<String> = Vec::new();
    let flush = |out: &mut Vec<Item>, name: &mut Option<String>, page: &mut Vec<String>| {
        if !page.is_empty() {
            out.push(Item::Page(name.take(), std::mem::take(page)));
        }
    };
    for raw in text.lines() {
        let line = raw.trim_end_matches(['\r', ' ']);
        if let Some(c) = line.strip_prefix(';') {
            if let Some(rest) = c.split_once("ジャンプ＜").map(|x| x.1)
                && let Some(n) = rest.split('＞').next()
            {
                flush(&mut out, &mut name, &mut page);
                out.push(Item::Jump(n.trim().to_lowercase()));
            }
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut out, &mut name, &mut page);
            continue;
        }
        if let Some(c) = line.strip_prefix('$') {
            flush(&mut out, &mut name, &mut page);
            let mut parts = c.split(',').map(|s| s.trim().to_string());
            let cmd = parts.next().unwrap_or_default().to_uppercase();
            out.push(Item::Cmd(cmd, parts.collect()));
            continue;
        }
        if let Some(n) = line.strip_prefix('【').and_then(|s| s.strip_suffix('】')) {
            flush(&mut out, &mut name, &mut page);
            name = Some(n.to_string());
            continue;
        }
        page.push(line.to_string());
    }
    flush(&mut out, &mut name, &mut page);
    out
}

/// `bg\bg00b.s25` -> `bg00b.s25`
pub fn base_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .trim()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_and_commands() {
        let src = ";c\r\n$L_BG,bg\\bg00b.s25,0\r\n【男】\r\n「おのこか」\r\n\r\nあ\r\nい\r\n;￥ジャンプ＜a002＞へ\r\n";
        let b = encoding_rs::SHIFT_JIS.encode(src).0;
        let items = parse(&b);
        assert_eq!(
            items,
            vec![
                Item::Cmd("L_BG".into(), vec!["bg\\bg00b.s25".into(), "0".into()]),
                Item::Page(Some("男".into()), vec!["「おのこか」".into()]),
                Item::Page(None, vec!["あ".into(), "い".into()]),
                Item::Jump("a002".into()),
            ]
        );
        assert_eq!(base_name("bg\\BG00B.S25"), "bg00b.s25");
    }
}
