//! UTF-8 / GB2312 (GBK) streaming text codec.
//!
//! The decoder holds incomplete multi-byte sequences between reads, so a
//! Chinese character split across two serial packets still displays correctly.

use crate::win;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncMode {
    Gb2312 = 0,
    Utf8 = 1,
    Auto = 2,
}

impl EncMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Gb2312,
            1 => Self::Utf8,
            _ => Self::Auto,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gb2312 => "GB2312",
            Self::Utf8 => "UTF-8",
            Self::Auto => "自动",
        }
    }
}

pub struct Decoder {
    mode: EncMode,
    pending: Vec<u8>,
}

impl Decoder {
    pub fn new(mode: EncMode) -> Self {
        Self { mode, pending: Vec::new() }
    }

    pub fn mode(&self) -> EncMode {
        self.mode
    }

    pub fn decode(&mut self, data: &[u8]) -> String {
        self.pending.extend_from_slice(data);
        let mut out = String::new();
        match self.mode {
            EncMode::Gb2312 => self.drain_gbk(&mut out),
            EncMode::Utf8 => self.drain_utf8(&mut out),
            EncMode::Auto => self.drain_auto(&mut out),
        }
        out
    }

    pub fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let text = match self.mode {
            EncMode::Gb2312 => win::decode_gbk(&self.pending),
            _ => String::from_utf8_lossy(&self.pending).into_owned(),
        };
        self.pending.clear();
        text
    }

    fn drain_gbk(&mut self, out: &mut String) {
        let cut = gbk_complete_len(&self.pending);
        if cut > 0 {
            out.push_str(&win::decode_gbk(&self.pending[..cut]));
            self.pending.drain(..cut);
        }
    }

    fn drain_utf8(&mut self, out: &mut String) {
        let (valid, incomplete) = utf8_prefix(&self.pending);
        if valid > 0 {
            out.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
            self.pending.drain(..valid);
        }
        if !incomplete && !self.pending.is_empty() {
            out.push_str(&String::from_utf8_lossy(&self.pending));
            self.pending.clear();
        }
    }

    fn drain_auto(&mut self, out: &mut String) {
        loop {
            if self.pending.is_empty() {
                return;
            }
            let (valid, incomplete) = utf8_prefix(&self.pending);
            if valid == self.pending.len() {
                out.push_str(&String::from_utf8_lossy(&self.pending));
                self.pending.clear();
                return;
            }
            if valid > 0 {
                out.push_str(&String::from_utf8_lossy(&self.pending[..valid]));
                self.pending.drain(..valid);
                continue;
            }
            if incomplete {
                return;
            }
            let cut = gbk_complete_len(&self.pending);
            if cut == 0 {
                return;
            }
            out.push_str(&win::decode_gbk(&self.pending[..cut]));
            self.pending.drain(..cut);
        }
    }
}

/// Number of leading bytes that form complete ASCII/GBK characters.
fn gbk_complete_len(b: &[u8]) -> usize {
    let mut i = 0;
    while i < b.len() {
        if b[i] < 0x80 {
            i += 1;
            continue;
        }
        if i + 1 >= b.len() {
            break;
        }
        let t = b[i + 1];
        if (0x40..=0xFE).contains(&t) && t != 0x7F {
            i += 2;
        } else {
            i += 1; // invalid trail byte; consume the lead to avoid a stall
        }
    }
    i
}

/// (number of leading bytes that are valid UTF-8, ends with an incomplete sequence)
fn utf8_prefix(b: &[u8]) -> (usize, bool) {
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c <= 0x7F {
            i += 1;
            continue;
        }
        let need = if (0xC2..=0xDF).contains(&c) {
            1
        } else if (0xE0..=0xEF).contains(&c) {
            2
        } else if (0xF0..=0xF4).contains(&c) {
            3
        } else {
            return (i, false);
        };
        if i + need >= b.len() {
            return (i, true);
        }
        for k in 1..=need {
            if !(0x80..=0xBF).contains(&b[i + k]) {
                return (i, false);
            }
        }
        if c == 0xE0 && b[i + 1] < 0xA0 {
            return (i, false);
        }
        if c == 0xED && b[i + 1] > 0x9F {
            return (i, false);
        }
        if c == 0xF0 && b[i + 1] < 0x90 {
            return (i, false);
        }
        if c == 0xF4 && b[i + 1] > 0x8F {
            return (i, false);
        }
        i += need + 1;
    }
    (i, false)
}

pub fn decode_all(data: &[u8], mode: EncMode) -> String {
    let mut d = Decoder::new(mode);
    let mut s = d.decode(data);
    s.push_str(&d.flush());
    s
}

pub fn encode(text: &str, mode: EncMode) -> Vec<u8> {
    match mode {
        EncMode::Gb2312 => win::encode_gbk(text),
        _ => text.as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gb2312_roundtrip() {
        let bytes = win::encode_gbk("中文测试 GB2312");
        assert_eq!(&bytes[0..4], &[0xD6, 0xD0, 0xCE, 0xC4]);
        assert_eq!(decode_all(&bytes, EncMode::Gb2312), "中文测试 GB2312");
    }

    #[test]
    fn utf8_roundtrip() {
        let s = "中文 UTF-8 测试";
        assert_eq!(decode_all(s.as_bytes(), EncMode::Utf8), s);
    }

    #[test]
    fn auto_mixed_lines() {
        let mut data = String::from("UTF8 中文第一行\r\n").into_bytes();
        data.extend_from_slice(&win::encode_gbk("GB2312 中文第二行\r\n"));
        let got = decode_all(&data, EncMode::Auto);
        assert!(got.contains("UTF8 中文第一行"));
        assert!(got.contains("GB2312 中文第二行"));
    }

    #[test]
    fn utf8_split_across_reads() {
        let bytes = "中文".as_bytes();
        let mut d = Decoder::new(EncMode::Utf8);
        let a = d.decode(&bytes[..2]);
        let b = d.decode(&bytes[2..]);
        assert_eq!(format!("{a}{b}"), "中文");
    }

    #[test]
    fn gb_split_across_reads() {
        let bytes = win::encode_gbk("中文");
        let mut d = Decoder::new(EncMode::Gb2312);
        let a = d.decode(&bytes[..1]);
        let b = d.decode(&bytes[1..]);
        assert_eq!(format!("{a}{b}"), "中文");
    }
}
