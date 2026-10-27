//! Skins: the XML (system.inf in a skin archive such as tk_title.noa) that lists a skin's
//! pictures, the styles of its items (buttons, texts) and its pages of items.

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

impl Node {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    /// An integer attribute ("12", "-3", "00FFFFFFH"); `def` when absent or empty.
    pub fn int(&self, k: &str, def: i32) -> i32 {
        match self.attr(k) {
            Some(v) if !v.trim().is_empty() => crate::value::parse_int(v),
            _ => def,
        }
    }

    pub fn flag(&self, k: &str) -> bool {
        self.attr(k) == Some("true")
    }

    pub fn kid(&self, tag: &str) -> Option<&Node> {
        self.kids.iter().find(|n| n.tag == tag)
    }
}

/// Parses an XML document (already decoded to text).
pub fn parse_xml(text: &str) -> Result<Node, String> {
    // roxmltree does not take a declaration naming another encoding
    let body = match text.find("?>") {
        Some(i) if text.trim_start().starts_with("<?xml") => &text[i + 2..],
        _ => text,
    };
    let doc = roxmltree::Document::parse(body).map_err(|e| e.to_string())?;
    fn conv(n: roxmltree::Node) -> Node {
        Node {
            tag: n.tag_name().name().to_string(),
            attrs: n
                .attributes()
                .map(|a| (a.name().to_string(), a.value().to_string()))
                .collect(),
            kids: n.children().filter(|c| c.is_element()).map(conv).collect(),
        }
    }
    Ok(conv(doc.root_element()))
}

/// Text that may be Shift-JIS or UTF-8 (with a byte order mark).
pub fn decode_text(b: &[u8]) -> String {
    if let Some(r) = b.strip_prefix(b"\xef\xbb\xbf") {
        return String::from_utf8_lossy(r).into_owned();
    }
    if let Some(r) = b.strip_prefix(b"\xff\xfe") {
        let u: Vec<u16> = r
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u);
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(b).0.into_owned(),
    }
}

pub struct Skin {
    /// the file it was loaded from
    pub src: String,
    /// where its pictures are: "tk_title.noa/"
    pub prefix: String,
    /// picture id → file
    pub images: HashMap<String, String>,
    pub styles: HashMap<String, Node>,
    /// window and page nodes by id
    pub pages: HashMap<String, Node>,
}

impl Skin {
    pub fn parse(prefix: &str, root: &Node) -> Skin {
        let mut s = Skin {
            src: String::new(),
            prefix: prefix.to_string(),
            images: HashMap::new(),
            styles: HashMap::new(),
            pages: HashMap::new(),
        };
        for n in &root.kids {
            match n.tag.as_str() {
                "resource" => {
                    for r in &n.kids {
                        if let (Some(id), Some(src)) = (r.attr("id"), r.attr("src")) {
                            s.images.insert(id.to_string(), src.to_string());
                        }
                    }
                }
                "declare_style" => {
                    for st in &n.kids {
                        if let Some(id) = st.attr("id") {
                            s.styles.insert(id.to_string(), st.clone());
                        }
                    }
                }
                _ => {
                    if let Some(id) = n.attr("id") {
                        s.pages.insert(id.to_string(), n.clone());
                    }
                }
            }
        }
        s
    }

    /// The file of a picture id.
    pub fn image_file(&self, id: &str) -> Option<String> {
        self.images.get(id).map(|f| format!("{}{f}", self.prefix))
    }
}

/// "ID_BTN:RECT(0,67,188,67)" as the picture id and its part (x y w h).
pub fn image_ref(s: &str) -> (String, Option<[i32; 4]>) {
    match s.split_once(':') {
        Some((id, rest)) => {
            let r = rest
                .trim()
                .strip_prefix("RECT(")
                .and_then(|r| r.strip_suffix(')'))
                .map(|r| {
                    let v: Vec<i32> = r.split(',').map(crate::value::parse_int).collect();
                    [
                        v.first().copied().unwrap_or(0),
                        v.get(1).copied().unwrap_or(0),
                        v.get(2).copied().unwrap_or(0),
                        v.get(3).copied().unwrap_or(0),
                    ]
                });
            (id.to_string(), r)
        }
        None => (s.to_string(), None),
    }
}
