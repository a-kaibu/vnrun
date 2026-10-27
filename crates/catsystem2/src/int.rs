//! CatSystem2 archives (.int, "KIF"), after GARbro's ArcINT.cs (MIT).
//!
//! ```text
//! "KIF\0" count:u32  count x { name[0x40] offset:u32 size:u32 }
//! ```
//!
//! Encrypted archives start with an entry "__key__.dat" whose size is a seed: the first number of
//! a Mersenne twister seeded with it is the Blowfish key (4 bytes, little end first) of the
//! offsets, sizes and file data. Each further entry i's offset+i and size are one Blowfish block;
//! its name is shifted through an alphabet by a twister seeded with game_key + i. The game key is
//! a CRC of a passphrase kept in the exe's resources ([`crate::exe::passphrase`]).

use std::collections::HashMap;

use blowfish::Blowfish;
use blowfish::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use nexas::files::Files;

type Cipher = Blowfish<byteorder::LE>;

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub offset: u32,
    pub size: u32,
}

pub struct Archive {
    pub file: String,
    pub entries: HashMap<String, Entry>,
    pub names: Vec<String>,
    cipher: Option<Cipher>,
}

/// The CRC-32 (not reflected) of a passphrase in CP932, as the key of the names.
pub fn game_key(pass: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = (i as u32) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04c1_1db7
            } else {
                c << 1
            };
        }
        *t = c;
    }
    let mut key = 0xffff_ffffu32;
    for &c in pass {
        key = !table[((key >> 24) ^ c as u32) as usize] ^ (key << 8);
    }
    key
}

/// MT19937 with the old (1998) seeding by the 69069 generator.
pub struct Twister {
    mt: [u32; 624],
    i: usize,
}

impl Twister {
    pub fn new(mut seed: u32) -> Twister {
        let mut mt = [0u32; 624];
        for m in mt.iter_mut() {
            let upper = seed & 0xffff_0000;
            seed = seed.wrapping_mul(69069).wrapping_add(1);
            *m = upper | (seed & 0xffff_0000) >> 16;
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        Twister { mt, i: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % 624] & 0x7fff_ffff);
                let mut v = self.mt[(k + 397) % 624] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908_b0df;
                }
                self.mt[k] = v;
            }
            self.i = 0;
        }
        let mut y = self.mt[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }
}

fn decipher_name(raw: &[u8], key: u32) -> Vec<u8> {
    const ABC: &[u8] = b"zyxwvutsrqponmlkjihgfedcbaZYXWVUTSRQPONMLKJIHGFEDCBA";
    let k = (key >> 24)
        .wrapping_add(key >> 16)
        .wrapping_add(key >> 8)
        .wrapping_add(key) as u8 as i32;
    let mut out = Vec::new();
    for (n, &c) in raw.iter().take_while(|&&c| c != 0).enumerate() {
        let k = k + n as i32;
        let mut c = c;
        if let Some(j) = ABC.iter().position(|&a| a == c) {
            let mut j = j as i32 - k % 0x34;
            if j < 0 {
                j += 0x34;
            }
            c = ABC[(0x33 - j) as usize];
        }
        out.push(c);
    }
    out
}

/// Blowfish over whole 8-byte blocks (a tail shorter than a block stays as it is).
pub fn decrypt(cipher: &Cipher, data: &mut [u8]) {
    for block in data.chunks_exact_mut(8) {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
}

/// How names are looked up.
pub fn key(name: &str) -> String {
    name.to_lowercase()
}

impl Archive {
    /// `game_key`: the key of encrypted archives, if the game has one.
    pub fn open(
        files: &mut dyn Files,
        file: &str,
        game_key: Option<u32>,
    ) -> Result<Archive, String> {
        let head = files.read_range(file, 0, 8).ok_or("cannot read")?;
        if &head[..4] != b"KIF\0" {
            return Err("not a KIF archive".into());
        }
        let count = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        if count > 1 << 20 {
            return Err("bad entry count".into());
        }
        let index = files
            .read_range(file, 8, count * 0x48)
            .ok_or("index is cut short")?;
        let rec = |i: usize| &index[i * 0x48..(i + 1) * 0x48];
        let u32_at = |r: &[u8], at: usize| u32::from_le_bytes(r[at..at + 4].try_into().unwrap());
        let mut a = Archive {
            file: file.to_string(),
            entries: HashMap::new(),
            names: Vec::new(),
            cipher: None,
        };
        let encrypted = count > 0 && rec(0).starts_with(b"__key__.dat\0");
        if !encrypted {
            for i in 0..count {
                let r = rec(i);
                let len = r[..0x40].iter().position(|&c| c == 0).unwrap_or(0x40);
                let name = crate::sjis(&r[..len]);
                a.add(name, u32_at(r, 0x40), u32_at(r, 0x44));
            }
            return Ok(a);
        }
        let main = game_key.ok_or("encrypted archive and no key")?;
        let seed = u32_at(rec(0), 0x44);
        let bf = Cipher::new_from_slice(&Twister::new(seed).next_u32().to_le_bytes())
            .map_err(|e| e.to_string())?;
        for i in 1..count {
            let r = rec(i);
            let mut block = [0u8; 8];
            block[..4].copy_from_slice(&u32_at(r, 0x40).wrapping_add(i as u32).to_le_bytes());
            block[4..].copy_from_slice(&r[0x44..0x48]);
            decrypt(&bf, &mut block);
            let name_key = Twister::new(main.wrapping_add(i as u32)).next_u32();
            let name = crate::sjis(&decipher_name(&r[..0x40], name_key));
            a.add(name, u32_at(&block, 0), u32_at(&block, 4));
        }
        a.cipher = Some(bf);
        Ok(a)
    }

    fn add(&mut self, name: String, offset: u32, size: u32) {
        self.entries.insert(key(&name), Entry { offset, size });
        self.names.push(name);
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&key(name))?;
        let mut data = files.read_range(&self.file, e.offset as u64, e.size as usize)?;
        if let Some(c) = &self.cipher {
            decrypt(c, &mut data);
        }
        Some(data)
    }
}
