//! CXCOM settings (UTF-8 ini file next to the executable).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::win;

pub const MAX_QUICK: usize = 99;

/// Default serial-plot rule: captures `name: 12.3` / `name=12.3` pairs.
/// Both `Temp: 25.3, Hum: 60` and `温度H: 54 %` work.
pub const DEFAULT_PLOT_RULE: &str = r"(?P<label>[^\s:=,]+)\s*[:=]\s*(?P<value>-?\d+(?:\.\d+)?)";

#[derive(Clone, Default)]
pub struct Quick {
    pub order: u32,
    pub delay: u32,
    pub comment: String,
    pub data: String,
    pub hex: bool,
}

#[derive(Clone)]
pub struct Config {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: u8,
    pub flow: u8,
    pub dtr: bool,
    pub rts: bool,
    pub auto_open: bool,
    pub auto_reconnect: bool,

    pub net_mode: u8,
    pub remote_ip: String,
    pub remote_port: u16,
    pub local_port: u16,

    pub hex_display: bool,
    pub timestamps: bool,
    pub timeout_ms: u32,
    pub rx_limit: usize,
    pub encoding: u8,
    pub bg: [u8; 3],
    pub fg: [u8; 3],
    pub dark: bool,
    pub font_name: String,
    pub font_size: f32,
    pub plot_enabled: bool,
    pub plot_rule: String,
    pub plot_hover_size: f32,

    pub hex_send: bool,
    pub newline: bool,
    pub timed: bool,
    pub interval: u32,
    pub checksum: u8,
    pub checksum_start: u32,
    pub checksum_tail: u32,
    pub file_path: String,

    pub quick: Vec<Quick>,
    pub quick_w: f32,
    pub show_quick: bool,
    pub win_x: i32,
    pub win_y: i32,
    pub win_w: i32,
    pub win_h: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: "COM1".into(),
            baud: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: 0,
            flow: 0,
            dtr: true,
            rts: false,
            auto_open: false,
            auto_reconnect: true,
            net_mode: 0,
            remote_ip: "127.0.0.1".into(),
            remote_port: 8080,
            local_port: 777,
            hex_display: false,
            timestamps: true,
            timeout_ms: 20,
            rx_limit: 200_000,
            encoding: 2,
            bg: [255, 255, 255],
            fg: [0, 0, 0],
            dark: false,
            font_name: "宋体".into(),
            font_size: 12.0,
            plot_enabled: true,
            plot_rule: DEFAULT_PLOT_RULE.into(),
            plot_hover_size: 18.0,
            hex_send: false,
            newline: false,
            timed: false,
            interval: 1000,
            checksum: 0,
            checksum_start: 1,
            checksum_tail: 0,
            file_path: String::new(),
            quick: at_presets(),
            quick_w: 400.0,
            show_quick: false,
            win_x: 120,
            win_y: 80,
            win_w: 960,
            win_h: 700,
        }
    }
}

pub fn ini_path() -> PathBuf {
    // macOS .app：不要把配置写进应用包内部（会破坏签名，也不符合规范），
    // 放到 ~/Library/Application Support/CXCOM/CXCOM.ini，并迁移旧的同级配置。
    #[cfg(target_os = "macos")]
    if let (Ok(exe), Some(home)) = (std::env::current_exe(), std::env::var_os("HOME")) {
        if let Some(path) = macos_bundled_ini(&exe, std::path::Path::new(&home)) {
            return path;
        }
    }
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    dir.push("CXCOM.ini");
    dir
}

/// 在 .app 里运行时返回 `~/Library/Application Support/CXCOM/CXCOM.ini`：
/// 应用包内部不应写入用户数据。首次迁移可执行文件同目录的旧配置。
#[cfg(target_os = "macos")]
fn macos_bundled_ini(exe: &std::path::Path, home: &std::path::Path) -> Option<PathBuf> {
    if !exe.to_string_lossy().contains(".app/Contents/MacOS/") {
        return None;
    }
    let dir = home.join("Library").join("Application Support").join("CXCOM");
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join("CXCOM.ini");
    if !path.exists() {
        if let Some(legacy) = exe.parent().map(|p| p.join("CXCOM.ini")) {
            if legacy.exists() {
                let _ = fs::copy(&legacy, &path);
            }
        }
    }
    Some(path)
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some('r') => {
                it.next();
                out.push('\r');
            }
            Some('n') => {
                it.next();
                out.push('\n');
            }
            Some('t') => {
                it.next();
                out.push('\t');
            }
            Some('|') => {
                it.next();
                out.push('|');
            }
            Some('\\') => {
                it.next();
                out.push('\\');
            }
            _ => out.push('\\'),
        }
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

fn parse_quick(v: &str) -> Quick {
    let mut q = Quick { delay: 1000, ..Default::default() };
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut esc = false;
    for c in v.chars() {
        if esc {
            cur.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    if let Some(x) = parts.first() {
        q.order = x.trim().parse().unwrap_or(0);
    }
    if let Some(x) = parts.get(1) {
        q.delay = x.trim().parse().unwrap_or(1000);
    }
    if let Some(x) = parts.get(2) {
        q.hex = x.eq_ignore_ascii_case("H");
    }
    if let Some(x) = parts.get(3) {
        // Keep the escaped text; only send_quick turns \r\n into CR/LF.
        // Unescaping here would lose the backslash on the next save.
        q.comment = x.clone();
    }
    if let Some(x) = parts.get(4) {
        q.data = x.clone();
    }
    q
}

fn num<T: std::str::FromStr>(map: &HashMap<String, String>, key: &str, dflt: T) -> T {
    map.get(key).and_then(|v| v.trim().parse().ok()).unwrap_or(dflt)
}

fn flag(map: &HashMap<String, String>, key: &str, dflt: bool) -> bool {
    map.get(key)
        .map(|v| {
            let v = v.trim().to_ascii_uppercase();
            v == "1" || v == "Y" || v == "TRUE" || v == "ON"
        })
        .unwrap_or(dflt)
}

fn color(map: &HashMap<String, String>, key: &str, dflt: [u8; 3]) -> [u8; 3] {
    let Some(v) = map.get(key) else { return dflt };
    let p: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if p.len() == 3 { [p[0], p[1], p[2]] } else { dflt }
}

pub fn at_presets() -> Vec<Quick> {
    let defs: [(&str, &str); 26] = [
        ("AT 测试", "AT\\r\\n"),
        ("重启模块", "AT+RST\\r\\n"),
        ("版本信息", "AT+GMR\\r\\n"),
        ("恢复出厂", "AT+RESTORE\\r\\n"),
        ("设置波特率 115200", "AT+UART_DEF=115200,8,1,0,0\\r\\n"),
        ("STA 模式", "AT+CWMODE=1\\r\\n"),
        ("AP 模式", "AT+CWMODE=2\\r\\n"),
        ("AP+STA 模式", "AT+CWMODE=3\\r\\n"),
        ("查询 WiFi 模式", "AT+CWMODE?\\r\\n"),
        ("扫描附近 WiFi", "AT+CWLAP\\r\\n"),
        ("连接 WiFi（改 SSID/密码）", "AT+CWJAP=\"SSID\",\"PASSWORD\"\\r\\n"),
        ("断开 WiFi", "AT+CWQAP\\r\\n"),
        ("查询本机 IP", "AT+CIFSR\\r\\n"),
        ("单连接模式", "AT+CIPMUX=0\\r\\n"),
        ("多连接模式", "AT+CIPMUX=1\\r\\n"),
        ("建立 TCP 连接（改 IP/端口）", "AT+CIPSTART=\"TCP\",\"192.168.1.100\",8080\\r\\n"),
        ("建立 UDP 连接（改 IP/端口）", "AT+CIPSTART=\"UDP\",\"192.168.1.100\",8080\\r\\n"),
        ("普通发送（指定长度）", "AT+CIPSEND=4\\r\\n"),
        ("开启透传模式", "AT+CIPMODE=1\\r\\n"),
        ("进入透传发送", "AT+CIPSEND\\r\\n"),
        ("退出透传（不要加换行）", "+++"),
        ("关闭当前连接", "AT+CIPCLOSE\\r\\n"),
        ("查询连接状态", "AT+CIPSTATUS\\r\\n"),
        ("Ping 测试", "AT+PING=\"www.baidu.com\"\\r\\n"),
        ("设置主机名", "AT+CWHOSTNAME=\"ESP01S\"\\r\\n"),
        ("关闭休眠", "AT+SLEEP=0\\r\\n"),
    ];
    let mut quick = vec![Quick::default(); MAX_QUICK];
    for (i, (comment, data)) in defs.iter().enumerate() {
        quick[i] = Quick {
            order: (i + 1) as u32,
            delay: 1000,
            comment: (*comment).to_string(),
            data: (*data).to_string(),
            hex: false,
        };
    }
    quick
}

fn repair_at_presets(quick: &mut [Quick]) {
    let presets = at_presets();
    for q in quick.iter_mut() {
        let Some(p) = presets.iter().find(|p| p.comment == q.comment) else { continue };
        // Older builds unescaped \r\n while loading, then dropped \r when
        // saving, leaving e.g. "ATn". Restore only that exact broken form.
        let broken = p.data.replace("\\r", "").replace('\\', "");
        if q.data == broken || q.data.contains('\r') || q.data.contains('\n') {
            q.data = p.data.clone();
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let mut cfg = Self::default();
        let Ok(bytes) = fs::read(ini_path()) else { return cfg };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut map = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        cfg.port = map.get("port").cloned().unwrap_or(cfg.port);
        cfg.baud = num(&map, "baud", cfg.baud);
        cfg.data_bits = num(&map, "databits", cfg.data_bits);
        cfg.stop_bits = num(&map, "stopbits", cfg.stop_bits);
        cfg.parity = num(&map, "parity", cfg.parity);
        cfg.flow = num(&map, "flow", cfg.flow);
        cfg.dtr = flag(&map, "dtr", cfg.dtr);
        cfg.rts = flag(&map, "rts", cfg.rts);
        cfg.auto_open = flag(&map, "autoopen", cfg.auto_open);
        cfg.auto_reconnect = flag(&map, "autoreconnect", cfg.auto_reconnect);
        cfg.net_mode = num(&map, "netmode", cfg.net_mode);
        cfg.remote_ip = map.get("remoteip").cloned().unwrap_or(cfg.remote_ip);
        cfg.remote_port = num(&map, "remoteport", cfg.remote_port);
        cfg.local_port = num(&map, "localport", cfg.local_port);
        cfg.hex_display = flag(&map, "hexdisplay", cfg.hex_display);
        cfg.timestamps = flag(&map, "timestamps", cfg.timestamps);
        cfg.timeout_ms = num(&map, "timeout", cfg.timeout_ms);
        cfg.rx_limit = num(&map, "rxlimit", cfg.rx_limit);
        cfg.encoding = num(&map, "encoding", cfg.encoding);
        cfg.bg = color(&map, "bg", cfg.bg);
        cfg.fg = color(&map, "fg", cfg.fg);
        cfg.dark = flag(&map, "dark", cfg.dark);
        cfg.font_name = map.get("fontname").cloned().unwrap_or(cfg.font_name);
        cfg.font_size = num(&map, "fontsize", cfg.font_size);
        cfg.plot_enabled = flag(&map, "plotenabled", cfg.plot_enabled);
        cfg.plot_rule = map.get("plotrule").cloned().unwrap_or(cfg.plot_rule);
        cfg.plot_hover_size = num(&map, "plothoversize", cfg.plot_hover_size).clamp(12.0, 24.0);
        cfg.hex_send = flag(&map, "hexsend", cfg.hex_send);
        cfg.newline = flag(&map, "newline", cfg.newline);
        cfg.timed = flag(&map, "timed", cfg.timed);
        cfg.interval = num(&map, "interval", cfg.interval);
        cfg.checksum = num(&map, "checksum", cfg.checksum);
        cfg.checksum_start = num(&map, "checksumstart", cfg.checksum_start);
        cfg.checksum_tail = num(&map, "checksumtail", cfg.checksum_tail);
        cfg.file_path = map.get("filepath").cloned().unwrap_or_default();
        cfg.quick_w = num(&map, "quickw", cfg.quick_w);
        cfg.show_quick = flag(&map, "showquick", cfg.show_quick);
        cfg.win_x = num(&map, "winx", cfg.win_x);
        cfg.win_y = num(&map, "winy", cfg.win_y);
        cfg.win_w = num(&map, "winw", cfg.win_w);
        cfg.win_h = num(&map, "winh", cfg.win_h);
        for i in 1..=MAX_QUICK {
            if let Some(v) = map.get(&format!("q{i}")) {
                cfg.quick[i - 1] = parse_quick(v);
            }
        }
        repair_at_presets(&mut cfg.quick);
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let mut s = String::new();
        s.push_str("; CXCOM settings (UTF-8)\n");
        let mut w = |k: &str, v: String| {
            s.push_str(&format!("{k}={v}\n"));
        };
        w("port", self.port.clone());
        w("baud", self.baud.to_string());
        w("databits", self.data_bits.to_string());
        w("stopbits", self.stop_bits.to_string());
        w("parity", self.parity.to_string());
        w("flow", self.flow.to_string());
        w("dtr", (self.dtr as u8).to_string());
        w("rts", (self.rts as u8).to_string());
        w("autoopen", (self.auto_open as u8).to_string());
        w("autoreconnect", (self.auto_reconnect as u8).to_string());
        w("netmode", self.net_mode.to_string());
        w("remoteip", self.remote_ip.clone());
        w("remoteport", self.remote_port.to_string());
        w("localport", self.local_port.to_string());
        w("hexdisplay", (self.hex_display as u8).to_string());
        w("timestamps", (self.timestamps as u8).to_string());
        w("timeout", self.timeout_ms.to_string());
        w("rxlimit", self.rx_limit.to_string());
        w("encoding", self.encoding.to_string());
        w("bg", format!("{},{},{}", self.bg[0], self.bg[1], self.bg[2]));
        w("fg", format!("{},{},{}", self.fg[0], self.fg[1], self.fg[2]));
        w("dark", (self.dark as u8).to_string());
        w("fontname", self.font_name.clone());
        w("fontsize", format!("{:.1}", self.font_size));
        w("plotenabled", (self.plot_enabled as u8).to_string());
        w("plotrule", self.plot_rule.clone());
        w("plothoversize", format!("{:.1}", self.plot_hover_size));
        w("hexsend", (self.hex_send as u8).to_string());
        w("newline", (self.newline as u8).to_string());
        w("timed", (self.timed as u8).to_string());
        w("interval", self.interval.to_string());
        w("checksum", self.checksum.to_string());
        w("checksumstart", self.checksum_start.to_string());
        w("checksumtail", self.checksum_tail.to_string());
        w("filepath", self.file_path.clone());
        w("quickw", format!("{:.0}", self.quick_w));
        w("showquick", (self.show_quick as u8).to_string());
        w("winx", self.win_x.to_string());
        w("winy", self.win_y.to_string());
        w("winw", self.win_w.to_string());
        w("winh", self.win_h.to_string());
        for (i, q) in self.quick.iter().enumerate() {
            s.push_str(&format!(
                "q{}={}|{}|{}|{}|{}\n",
                i + 1,
                q.order,
                q.delay,
                if q.hex { "H" } else { "A" },
                escape(&q.comment),
                escape(&q.data)
            ));
        }
        let path = ini_path();
        fs::write(&path, s).map_err(|e| format!("保存配置失败: {e}"))
    }
}

pub fn timestamp() -> String {
    if let Some((h, m, s, ms)) = win::local_time() {
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    } else {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let s = d.as_secs();
        format!("{:02}:{:02}:{:02}.{:03}", (s / 3600) % 24, (s / 60) % 60, s % 60, d.subsec_millis())
    }
}

/// 午夜以来的毫秒数（0..86_400_000）。接收缓冲用它当紧凑时间戳，
/// 显示时才通过 `buffer::fmt_time` 格式化。
pub fn timestamp_ms() -> u32 {
    match win::local_time() {
        Some((h, m, s, ms)) => {
            (h as u32 * 3_600_000 + m as u32 * 60_000 + s as u32 * 1_000 + ms as u32) % 86_400_000
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bundle_config_moves_out_of_app() {
        let root = std::env::temp_dir().join(format!("cxcom_ini_test_{}", std::process::id()));
        let exe_dir = root.join("CXCOM.app").join("Contents").join("MacOS");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::write(exe_dir.join("CXCOM.ini"), "fontsize=18\n").unwrap();
        let home = root.join("home");
        let path = macos_bundled_ini(&exe_dir.join("CXCOM"), &home).expect("bundled ini path");
        assert!(path.starts_with(home.join("Library").join("Application Support").join("CXCOM")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fontsize=18\n");
        // 不在 .app 内运行时保持原行为（exe 同目录）
        assert!(macos_bundled_ini(&root.join("CXCOM"), &home).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn at_presets_are_available_by_default() {        let cfg = Config::default();
        assert_eq!(cfg.quick.len(), MAX_QUICK);
        assert_eq!(cfg.quick[0].comment, "AT 测试");
        // The source literal has two backslashes (`\\r\\n`); the runtime string
        // therefore contains one backslash before r/n, and unescape turns it
        // into a real CR/LF before sending.
        assert_eq!(cfg.quick[0].data, "AT\\r\\n");
        assert_eq!(unescape(&cfg.quick[0].data), "AT\r\n");
        assert!(cfg.quick.iter().any(|q| q.data.contains("AT+CWJAP")));
        assert!(cfg.quick.iter().any(|q| q.data.contains("AT+CIPSTART")));
    }

    #[test]
    fn quick_strings_survive_save_and_load() {        let cfg = Config::default();
        for q in cfg.quick.iter().take(26) {
            let saved = format!(
                "{}|{}|{}|{}|{}",
                q.order,
                q.delay,
                if q.hex { "A" } else { "H" },
                escape(&q.comment),
                escape(&q.data)
            );
            let parsed = parse_quick(&saved);
            assert_eq!(parsed.data, q.data, "round-trip failed for {}", q.comment);
        }
    }

    #[test]
    fn repairs_legacy_broken_presets() {
        let mut quick = at_presets();
        quick[0].data = "ATn".into();
        repair_at_presets(&mut quick);
        assert_eq!(quick[0].data, "AT\\r\\n");
    }
}
