//! Bounded receive buffer and the text-row renderer.

use std::collections::VecDeque;

use eframe::egui::Color32;

use crate::checksum::hex_line;
use crate::text::{Decoder, EncMode};

pub struct Chunk {
    pub t: f64,
    pub time: String,
    pub data: Vec<u8>,
    pub tx: bool,
}

pub struct Row {
    pub text: String,
    pub color: Color32,
}

pub struct RenderOpts {
    pub hex: bool,
    pub timestamps: bool,
    pub timeout_ms: u32,
    pub mode: EncMode,
    pub fg: Color32,
    pub tx: Color32,
    pub header: Color32,
    pub cap: usize,
}

pub struct Rx {
    chunks: VecDeque<Chunk>,
    total: usize,
    dropped: u64,
    limit: usize,
}

impl Rx {
    pub fn new(limit: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            total: 0,
            dropped: 0,
            limit: limit.max(16 * 1024),
        }
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn chunks(&self) -> &VecDeque<Chunk> {
        &self.chunks
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(16 * 1024);
        self.trim();
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.total = 0;
        self.dropped = 0;
    }

    pub fn add(&mut self, time: String, data: Vec<u8>) {
        self.add_dir(time, data, false);
    }

    pub fn add_tx(&mut self, time: String, data: Vec<u8>) {
        self.add_dir(time, data, true);
    }

    fn add_dir(&mut self, time: String, data: Vec<u8>, tx: bool) {
        if data.is_empty() {
            return;
        }
        let t = now_secs();
        self.total += data.len();
        self.chunks.push_back(Chunk { t, time, data, tx });
        self.trim();
    }

    fn trim(&mut self) {
        if self.total <= self.limit {
            return;
        }
        let target = (self.limit * 3 / 4).max(16 * 1024);
        while self.total > target {
            let Some(front) = self.chunks.front_mut() else {
                self.total = 0;
                break;
            };
            let excess = self.total - target;
            if front.data.len() <= excess {
                let n = front.data.len();
                self.total -= n;
                self.dropped += n as u64;
                self.chunks.pop_front();
            } else {
                front.data.drain(..excess);
                self.total -= excess;
                self.dropped += excess as u64;
                break;
            }
        }
    }

    pub fn write_raw(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        for c in &self.chunks {
            w.write_all(&c.data)?;
        }
        Ok(())
    }

    pub fn render(&self, o: &RenderOpts) -> (Vec<Row>, usize) {
        let cap = o.cap.max(4096);
        let tail_bytes = self.total.min(cap);
        let start = self.total - tail_bytes;
        let mut start_offset = self.dropped + start as u64;
        let mut skip = start;
        let mut rows = Vec::new();
        let mut rendered = 0usize;

        if o.timestamps {
            let mut decoder = Decoder::new(o.mode);
            let mut last_t = f64::NEG_INFINITY;
            let mut last_tx: Option<bool> = None;
            let mut group_text = String::new();
            for c in &self.chunks {
                let mut data = &c.data[..];
                if skip >= data.len() {
                    skip -= data.len();
                    start_offset += data.len() as u64;
                    continue;
                }
                if skip > 0 {
                    start_offset += skip as u64;
                    data = &data[skip..];
                    skip = 0;
                }
                rendered += data.len();
                let new_group = c.t - last_t > (o.timeout_ms as f64 / 1000.0)
                    || last_tx != Some(c.tx);
                if new_group {
                    // Flush the previous packet only when a new packet starts;
                    // chunks inside one packet stay on the same line.
                    if !group_text.is_empty() {
                        push_text_rows(&mut rows, &group_text, o.fg);
                        group_text.clear();
                    }
                    rows.push(Row {
                        text: format!(
                            "[{}] {}",
                            c.time,
                            if c.tx { "<--发-->" } else { "-->收<--" }
                        ),
                        color: o.header,
                    });
                    last_t = c.t;
                    last_tx = Some(c.tx);
                }
                if o.hex {
                    push_hex_rows(&mut rows, data, start_offset, o.fg);
                } else {
                    group_text.push_str(&decoder.decode(data));
                }
                start_offset += c.data.len() as u64;
            }
            if !o.hex {
                group_text.push_str(&decoder.flush());
            }
            if !group_text.is_empty() {
                push_text_rows(&mut rows, &group_text, o.fg);
            }
        } else {
            let mut all = Vec::with_capacity(tail_bytes);
            let mut skip2 = start;
            for c in &self.chunks {
                let mut data = &c.data[..];
                if skip2 >= data.len() {
                    skip2 -= data.len();
                    continue;
                }
                if skip2 > 0 {
                    data = &data[skip2..];
                    skip2 = 0;
                }
                all.extend_from_slice(data);
            }
            rendered = all.len();
            if o.hex {
                push_hex_rows(&mut rows, &all, start_offset, o.fg);
            } else {
                let mut d = Decoder::new(o.mode);
                let mut text = d.decode(&all);
                text.push_str(&d.flush());
                push_text_rows(&mut rows, &text, o.fg);
            }
            start_offset += all.len() as u64;
        }
        let _ = start_offset;
        (rows, rendered)
    }
}

fn now_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\t' => out.push_str("    "),
            c if c.is_control() && c != '\r' && c != '\n' => out.push('.'),
            c => out.push(c),
        }
    }
    out
}

fn push_text_rows(rows: &mut Vec<Row>, text: &str, color: Color32) {
    if text.is_empty() {
        return;
    }
    let mut line = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            rows.push(Row { text: sanitize(&line), color });
            line.clear();
        } else if ch != '\r' {
            line.push(ch);
        }
    }
    if !line.is_empty() {
        rows.push(Row { text: sanitize(&line), color });
    }
}

fn push_hex_rows(rows: &mut Vec<Row>, data: &[u8], start: u64, color: Color32) {
    for (i, part) in data.chunks(16).enumerate() {
        let off = start + (i * 16) as u64;
        let mut hex = hex_line(part);
        while hex.len() < 47 {
            hex.push(' ');
        }
        let ascii: String = part
            .iter()
            .map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' })
            .collect();
        rows.push(Row {
            text: format!("{off:08X}  {hex}  |{ascii}|"),
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::EncMode;

    #[test]
    fn renders_tx_header() {
        let mut rx = Rx::new(200_000);
        rx.add_tx("12:00:00.000".into(), b"1234\r\n".to_vec());
        let opts = RenderOpts {
            hex: false,
            timestamps: true,
            timeout_ms: 20,
            mode: EncMode::Utf8,
            fg: Color32::BLACK,
            tx: Color32::BLUE,
            header: Color32::GRAY,
            cap: 256 * 1024,
        };
        let (rows, _) = rx.render(&opts);
        assert!(rows.iter().any(|r| r.text.contains("<--发-->")));
        assert!(rows.iter().any(|r| r.text == "1234"));
    }

    #[test]
    fn renders_utf8_rows() {
        let mut rx = Rx::new(200_000);
        rx.add("12:00:00.000".into(), "UTF8 中文\r\n".as_bytes().to_vec());
        let opts = RenderOpts {
            hex: false,
            timestamps: true,
            timeout_ms: 20,
            mode: EncMode::Utf8,
            fg: Color32::BLACK,
            tx: Color32::BLUE,
            header: Color32::GRAY,
            cap: 256 * 1024,
        };
        let (rows, n) = rx.render(&opts);
        assert!(n > 0);
        assert!(rows.iter().any(|r| r.text.contains("UTF8 中文")));
    }
}
