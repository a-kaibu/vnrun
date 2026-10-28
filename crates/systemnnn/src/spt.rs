//! SPT scripts (XORed with 0xFF): little-endian words, positions counted in words. The header
//! (0x20 words, "SPTHEADER0" at word 12) gives the message count and table (words 4, 5), the
//! string count and table (6, 7), the film count and table (8, 9) and the table of big
//! constants (18). The tables hold word positions of NUL-terminated CP932 text. After the header
//! come records: their length (with itself), their type and code, then the arguments.

pub struct Script {
    pub name: String,
    pub w: Vec<u32>,
    bytes: Vec<u8>,
}

impl Script {
    pub fn parse(name: &str, raw: &[u8]) -> Result<Script, String> {
        let bytes: Vec<u8> = raw.iter().map(|b| b ^ 0xff).collect();
        let w: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        if w.len() < 0x20 || w[1] != 0x6666_0001 {
            return Err(format!("{name}: not an SPT script"));
        }
        Ok(Script {
            name: name.to_string(),
            w,
            bytes,
        })
    }

    pub fn word(&self, i: usize) -> u32 {
        self.w.get(i).copied().unwrap_or(0)
    }

    fn text_at(&self, table: usize, count: usize, i: usize) -> String {
        if i >= count {
            return String::new();
        }
        let at = self.word(table + i) as usize * 4;
        let Some(rest) = self.bytes.get(at..) else {
            return String::new();
        };
        let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        nexas::sjis::decode(&rest[..end])
    }

    pub fn string(&self, i: u32) -> String {
        self.text_at(self.word(7) as usize, self.word(6) as usize, i as usize)
    }

    pub fn message(&self, i: u32) -> String {
        self.text_at(self.word(5) as usize, self.word(4) as usize, i as usize)
    }

    pub fn films(&self) -> usize {
        self.word(8) as usize
    }

    /// Where film `i` starts.
    pub fn film(&self, i: u32) -> Option<usize> {
        if (i as usize) < self.films() {
            Some(self.word(self.word(9) as usize + i as usize) as usize)
        } else {
            None
        }
    }

    pub fn big(&self, i: u32) -> i32 {
        self.word(self.word(18) as usize + i as usize) as i32
    }
}
