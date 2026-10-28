//! The sounds: Ogg Vorbis whose first page marker is "Tink" and whose first 0xe1f bytes after
//! it are XORed with a text the program carries (a GUID and a copyright line, with its
//! closing zero); music has 12 more bytes in front.

/// The text of the 2018 trials, used when the program's own is not found.
pub const DEFAULT_KEY: &[u8] =
    b"DBB3206F-F171-4885-A131-EC7FBA6FF491 Copyright 2004 Cyberworks \"TinkerBell\"., all rights reserved.";

/// Finds the key in the program: a GUID followed by " Copyright".
pub fn key_from_exe(b: &[u8]) -> Option<Vec<u8>> {
    let mut from = 0;
    while let Some(p) = find(&b[from..], b" Copyright ") {
        let at = from + p;
        if at >= 36 {
            let guid = &b[at - 36..at];
            let ok = guid.iter().enumerate().all(|(i, &c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c == b'-'
                } else {
                    c.is_ascii_hexdigit()
                }
            });
            if ok {
                let end = b[at..].iter().position(|&c| c == 0)?;
                return Some(b[at - 36..at + end].to_vec());
            }
        }
        from = at + 1;
    }
    None
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// The playable Ogg of a sound entry.
pub fn decode(mut d: Vec<u8>, key: &[u8]) -> Vec<u8> {
    let start = [0usize, 12]
        .into_iter()
        .find(|&s| d.get(s..s + 4) == Some(b"Tink"))
        .or_else(|| {
            [0usize, 12]
                .into_iter()
                .find(|&s| d.get(s..s + 4) == Some(b"OggS"))
        });
    let Some(start) = start else {
        return d;
    };
    d.drain(..start);
    if d.starts_with(b"Tink") && !key.is_empty() {
        // the key's index runs 0..=len (its closing zero), then 1..=len again and again
        let mut k = 0usize;
        let end = d.len().min(0xe1f);
        for b in d.iter_mut().take(end).skip(4) {
            *b ^= key.get(k).copied().unwrap_or(0);
            if k == key.len() {
                k = 0;
            }
            k += 1;
        }
        d[..4].copy_from_slice(b"OggS");
    }
    d
}
