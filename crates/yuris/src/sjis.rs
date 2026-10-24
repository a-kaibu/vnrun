//! Shift_JIS (CP932) text, the encoding of every name and string in a YU-RIS project.

pub fn decode(b: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode_without_bom_handling(b).0.into_owned()
}

pub fn encode(s: &str) -> Vec<u8> {
    encoding_rs::SHIFT_JIS.encode(s).0.into_owned()
}
