//! Enumerate system fonts containing Chinese glyphs and load the chosen one.
//!
//! Windows uses C:\Windows\Fonts + the per-user font folder, macOS the system
//! and user Fonts folders, Linux the fontconfig-style directories. Only the
//! sfnt/cmap headers are read to test U+4E2D; the selected file is memory-mapped
//! for egui.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};

#[derive(Clone)]
pub struct FontFace {
    pub name: String,
    pub path: PathBuf,
    pub index: u32,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    if o + 2 <= d.len() {
        Some(u16::from_be_bytes([d[o], d[o + 1]]))
    } else {
        None
    }
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    if o + 4 <= d.len() {
        Some(u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]))
    } else {
        None
    }
}

fn read_at(f: &mut File, offset: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Does the given face contain U+4E2D?
pub fn supports_chinese(path: &Path, face_index: u32) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    let Some(head) = read_at(&mut f, 0, 12) else { return false };
    let mut face_off = 0u32;
    if &head[0..4] == b"ttcf" {
        let Some(num) = u32_at(&head, 8) else { return false };
        if face_index >= num {
            return false;
        }
        let Some(off) = read_at(&mut f, 12 + 4 * face_index as u64, 4) else { return false };
        face_off = u32_at(&off, 0).unwrap_or(0);
    } else if face_index > 0 {
        return false;
    }

    let Some(face) = read_at(&mut f, face_off as u64, 12) else { return false };
    let Some(num_tables) = u16_at(&face, 4) else { return false };
    if num_tables == 0 || num_tables > 512 {
        return false;
    }
    let Some(records) = read_at(&mut f, face_off as u64 + 12, num_tables as usize * 16) else {
        return false;
    };
    let mut cmap_off = None;
    let mut cmap_len = 0usize;
    for i in 0..num_tables as usize {
        let rec = &records[i * 16..i * 16 + 16];
        if &rec[0..4] == b"cmap" {
            cmap_off = u32_at(rec, 8).map(|v| v as u64);
            cmap_len = u32_at(rec, 12).unwrap_or(0) as usize;
            break;
        }
    }
    let (Some(off), true) = (cmap_off, cmap_len > 0 && cmap_len < 2 * 1024 * 1024) else {
        return false;
    };
    let Some(cmap) = read_at(&mut f, off, cmap_len) else { return false };
    let Some(count) = u16_at(&cmap, 2) else { return false };
    let mut fmt4 = None;
    let mut fmt12 = None;
    for i in 0..count as usize {
        let Some(sub) = u32_at(&cmap, 4 + i * 8 + 4) else { continue };
        let Some(format) = u16_at(&cmap, sub as usize) else { continue };
        match format {
            4 if fmt4.is_none() => fmt4 = Some(sub as usize),
            12 if fmt12.is_none() => fmt12 = Some(sub as usize),
            _ => {}
        }
    }
    if let Some(sub) = fmt12 {
        if let Some(n) = u32_at(&cmap, sub + 12) {
            for g in 0..n.min(100_000) as usize {
                let base = sub + 16 + g * 12;
                let (Some(a), Some(b)) = (u32_at(&cmap, base), u32_at(&cmap, base + 4)) else {
                    break;
                };
                if (a..=b).contains(&0x4E2D) {
                    return true;
                }
                if a > 0x4E2D {
                    break;
                }
            }
        }
    }
    if let Some(sub) = fmt4 {
        let Some(seg_x2) = u16_at(&cmap, sub + 6) else { return false };
        let seg = seg_x2 as usize / 2;
        let end_base = sub + 14;
        let start_base = end_base + seg_x2 as usize + 2;
        let delta_base = start_base + seg_x2 as usize;
        let range_base = delta_base + seg_x2 as usize;
        for i in 0..seg {
            let (Some(end), Some(start)) = (u16_at(&cmap, end_base + 2 * i), u16_at(&cmap, start_base + 2 * i)) else {
                return false;
            };
            if !(start..=end).contains(&0x4E2Du16) {
                continue;
            }
            let Some(delta) = u16_at(&cmap, delta_base + 2 * i) else { return false };
            let Some(range) = u16_at(&cmap, range_base + 2 * i) else { return false };
            if range == 0 {
                return ((0x4E2D as i32 + delta as i16 as i32) & 0xFFFF) != 0;
            }
            let addr = range_base + 2 * i + range as usize + 2 * (0x4E2D - start) as usize;
            let Some(glyph) = u16_at(&cmap, addr) else { return false };
            if glyph == 0 {
                return false;
            }
            return (((glyph as i32) + delta as i16 as i32) & 0xFFFF) != 0;
        }
    }
    false
}

fn pretty(stem: &str) -> String {
    let lower = stem.to_ascii_lowercase();
    let known = match lower.as_str() {
        "simsun" => Some("宋体"),
        "simhei" => Some("黑体"),
        "simkai" => Some("楷体"),
        "simfang" => Some("仿宋"),
        "msyh" | "msyhbd" | "msyhl" => Some("微软雅黑"),
        "msjh" | "msjhbd" => Some("微软正黑体"),
        "deng" | "dengb" | "dengl" => Some("等线"),
        "mingliu" | "mingliub" => Some("细明体"),
        "stkaiti" | "stkaitib" => Some("华文楷体"),
        "stsong" | "stsongb" => Some("华文宋体"),
        "stfangso" => Some("华文仿宋"),
        "stxihei" => Some("华文细黑"),
        _ => None,
    };
    if let Some(name) = known {
        return name.to_string();
    }
    if lower.contains("pingfang") {
        return "PingFang SC".to_string();
    }
    if lower.contains("songti") {
        return "Songti SC".to_string();
    }
    if lower.contains("heiti") || lower.contains("stheiti") {
        return "Heiti SC".to_string();
    }
    if lower.contains("hiragino") {
        return "Hiragino Sans GB".to_string();
    }
    if lower.contains("notosanscjk") || lower.contains("notosanssc") {
        return "Noto Sans CJK".to_string();
    }
    if lower.contains("notoserifcjk") {
        return "Noto Serif CJK".to_string();
    }
    if lower.contains("sourcehansans") {
        return "思源黑体".to_string();
    }
    if lower.contains("sourcehanserif") {
        return "思源宋体".to_string();
    }
    if lower.contains("wqy") && lower.contains("zenhei") {
        return "文泉驿正黑".to_string();
    }
    if lower.contains("wqy") && lower.contains("microhei") {
        return "文泉驿微米黑".to_string();
    }
    if lower.contains("droidsansfallback") {
        return "Droid Sans Fallback".to_string();
    }
    stem.to_string()
}

fn scan_dir(dir: &Path, out: &mut Vec<FontFace>, depth: u8) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 {
                scan_dir(&path, out, depth + 1);
            }
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "ttf" && ext != "ttc" && ext != "otf" {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.is_empty() || stem.starts_with('@') {
            continue;
        }
        let mut index = 0u32;
        let mut ok = supports_chinese(&path, 0);
        if !ok && ext == "ttc" {
            for i in 1..4 {
                if supports_chinese(&path, i) {
                    index = i;
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        out.push(FontFace { name: pretty(stem), path: path.clone(), index });
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// All installed fonts with a Chinese glyph, common names first.
pub fn catalog() -> Vec<FontFace> {
    let mut fonts = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            scan_dir(&PathBuf::from(windir).join("Fonts"), &mut fonts, 0);
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            scan_dir(&PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"), &mut fonts, 0);
        }
    }
    #[cfg(target_os = "macos")]
    {
        scan_dir(Path::new("/System/Library/Fonts"), &mut fonts, 0);
        scan_dir(Path::new("/Library/Fonts"), &mut fonts, 0);
        if let Some(home) = home_dir() {
            scan_dir(&home.join("Library").join("Fonts"), &mut fonts, 0);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        scan_dir(Path::new("/usr/share/fonts"), &mut fonts, 0);
        scan_dir(Path::new("/usr/local/share/fonts"), &mut fonts, 0);
        if let Some(home) = home_dir() {
            scan_dir(&home.join(".local").join("share").join("fonts"), &mut fonts, 0);
            scan_dir(&home.join(".fonts"), &mut fonts, 0);
        }
    }
    let mut seen = std::collections::HashSet::new();
    fonts.retain(|f| seen.insert(f.name.to_ascii_lowercase()));
    let rank = |name: &str| -> u8 {
        match name {
            "宋体" => 0,
            "微软雅黑" => 1,
            "黑体" => 2,
            "PingFang SC" | "Noto Sans CJK" => 1,
            "Songti SC" | "文泉驿正黑" => 2,
            _ => 10,
        }
    };
    fonts.sort_by(|a, b| rank(&a.name).cmp(&rank(&b.name)).then(a.name.cmp(&b.name)));
    fonts
}

pub fn default_name(fonts: &[FontFace], wanted: &str) -> String {
    if fonts.iter().any(|f| f.name == wanted) {
        return wanted.to_owned();
    }
    let preferred: &[&str] = if cfg!(windows) {
        &["宋体", "微软雅黑", "黑体"]
    } else if cfg!(target_os = "macos") {
        &["PingFang SC", "Songti SC", "Heiti SC", "宋体"]
    } else {
        &["Noto Sans CJK", "文泉驿正黑", "思源黑体", "Droid Sans Fallback"]
    };
    for name in preferred {
        if fonts.iter().any(|f| &f.name == name) {
            return (*name).to_owned();
        }
    }
    fonts.first().map(|f| f.name.clone()).unwrap_or_else(|| "内置".to_owned())
}

pub fn install(ctx: &eframe::egui::Context, fonts: &[FontFace], wanted: &str) -> String {
    let mut defs = FontDefinitions::default();
    let mut used = String::new();
    let mut candidates: Vec<&FontFace> = Vec::new();
    if let Some(f) = fonts.iter().find(|f| f.name == wanted) {
        candidates.push(f);
    }
    for fallback in ["宋体", "微软雅黑", "黑体", "Noto Sans CJK", "PingFang SC"] {
        if let Some(f) = fonts.iter().find(|f| f.name == fallback && f.name != wanted) {
            candidates.push(f);
        }
    }
    if candidates.is_empty() {
        if let Some(f) = fonts.first() {
            candidates.push(f);
        }
    }

    if let Some(face) = candidates.first() {
        if let Ok(file) = File::open(&face.path) {
            if let Ok(mmap) = unsafe { memmap2::Mmap::map(&file) } {
                if mmap.len() >= 1024 {
                    let leaked: &'static memmap2::Mmap = Box::leak(Box::new(mmap));
                    let mut data = FontData::from_static(&leaked[..]);
                    data.index = face.index;
                    defs.font_data.insert("system".to_owned(), Arc::new(data));
                    used = face.name.clone();
                }
            }
        }
    }
    if !used.is_empty() {
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, "system".to_owned());
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "system".to_owned());
    }

    defs.font_data.insert(
        "fallback".to_owned(),
        Arc::new(FontData::from_static(include_bytes!("../assets/ui_cjk.ttf"))),
    );
    defs.font_data.insert(
        "mono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!("../assets/ui_mono.ttf"))),
    );
    defs.families.entry(FontFamily::Proportional).or_default().push("fallback".to_owned());
    let mono = defs.families.entry(FontFamily::Monospace).or_default();
    mono.push("mono".to_owned());
    mono.push("fallback".to_owned());
    ctx.set_fonts(defs);
    if used.is_empty() { "内置".to_owned() } else { used }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_finds_chinese_fonts() {
        let fonts = catalog();
        assert!(fonts.len() >= 1, "expected at least one Chinese font");
        if cfg!(windows) {
            assert!(fonts.iter().any(|f| f.name == "宋体"), "SimSun should be detected");
        }
        assert!(!default_name(&fonts, "不存在的字体").is_empty());
    }
}
