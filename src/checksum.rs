//! HEX parsing and checksum modes.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChecksumKind {
    None,
    ModbusCrc16,
    Add,
    Xor,
}

impl ChecksumKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::ModbusCrc16,
            2 => Self::Add,
            3 => Self::Xor,
            _ => Self::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::ModbusCrc16 => "ModbusCRC16",
            Self::Add => "ADD",
            Self::Xor => "XOR",
        }
    }
}

pub fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut tok = String::new();
    for c in input.chars() {
        if c.is_ascii_hexdigit() || c == 'x' || c == 'X' || c == '$' {
            tok.push(c);
        } else if c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '|') {
            flush(&mut tok, &mut out)?;
        } else {
            return Err(format!("无效字符 '{c}'"));
        }
    }
    flush(&mut tok, &mut out)?;
    Ok(out)
}

fn flush(tok: &mut String, out: &mut Vec<u8>) -> Result<(), String> {
    if tok.is_empty() {
        return Ok(());
    }
    let mut t = tok.as_str();
    if t.starts_with("0x") || t.starts_with("0X") {
        t = &t[2..];
    } else if let Some(rest) = t.strip_prefix('$') {
        t = rest;
    }
    if t.is_empty() {
        tok.clear();
        return Ok(());
    }
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("无效十六进制片段: {t}"));
    }
    if t.len() % 2 != 0 {
        return Err(format!("十六进制片段长度必须为偶数: {t}"));
    }
    let b = t.as_bytes();
    for p in b.chunks(2) {
        let hi = (p[0] as char).to_digit(16).unwrap() as u8;
        let lo = (p[1] as char).to_digit(16).unwrap() as u8;
        out.push((hi << 4) | lo);
    }
    tok.clear();
    Ok(())
}

pub fn apply(data: &[u8], kind: ChecksumKind, start: u32, tail: u32) -> Vec<u8> {
    if data.is_empty() || kind == ChecksumKind::None {
        return data.to_vec();
    }
    let tail = (tail as usize).min(data.len());
    let end = data.len() - tail;
    let first = ((start.max(1) as usize) - 1).min(end);
    if first >= end {
        return data.to_vec();
    }
    let body = &data[first..end];
    let sum: Vec<u8> = match kind {
        ChecksumKind::ModbusCrc16 => {
            let mut crc: u16 = 0xFFFF;
            for &b in body {
                crc ^= b as u16;
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
                }
            }
            crc.to_le_bytes().to_vec()
        }
        ChecksumKind::Add => vec![body.iter().fold(0u8, |a, &b| a.wrapping_add(b))],
        ChecksumKind::Xor => vec![body.iter().fold(0u8, |a, &b| a ^ b)],
        ChecksumKind::None => Vec::new(),
    };
    let mut out = Vec::with_capacity(data.len() + sum.len());
    out.extend_from_slice(&data[..end]);
    out.extend_from_slice(&sum);
    out.extend_from_slice(&data[end..]);
    out
}

pub fn hex_line(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}
