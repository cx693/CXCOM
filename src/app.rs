//! CXCOM UI: serial/network debugger with GB2312 / UTF-8 switching.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, ComboBox, FontData, FontDefinitions, FontFamily, FontId, Layout,
    RichText, ScrollArea, Sense, TextEdit, TextStyle, Vec2,
};
use eframe::glow::HasContext;

use crate::buffer::{RenderOpts, Row, Rx};
use crate::checksum::{self, ChecksumKind};
use crate::config::{self, Config};
use crate::fonts;
use crate::text::{self, EncMode};
use crate::transport::{self, Cmd, Evt};
use crate::win;

const BAUDS: [u32; 12] = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 2000000];
const LIMITS: [(&str, usize); 7] = [
    ("200K", 200_000), ("500K", 500_000), ("1M", 1_000_000), ("2M", 2_000_000),
    ("5M", 5_000_000), ("10M", 10_000_000), ("20M", 20_000_000),
];
/// 行数组的安全上限：显示缓冲按字节封顶，但“全是换行”这类极端
/// 数据会产生数百万行，行本身也要有独立上限。
const VIEW_MAX_ROWS: usize = 60_000;

/// 在自动换行的底部栏里给 ComboBox 一个固定宽度的“格子”。
/// 直接放在 horizontal_wrapped 里时，ComboBox 如果正好落在换行边界，
/// 会被画到可视区域之外（看起来像凭空消失），套一层 allocate_ui 后
/// 整块控件会作为一个整体换到下一行。
fn combo_cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 20.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            // 选中文本超长时截断，保证每个下拉框宽度固定，排版才可预测
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            add(ui);
        },
    );
}

fn limit_label(limit: usize) -> &'static str {
    for (name, value) in LIMITS {
        if value == limit {
            return name;
        }
    }
    "自定义"
}

fn plot_color(i: usize) -> Color32 {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(230, 80, 70),
        Color32::from_rgb(60, 150, 230),
        Color32::from_rgb(50, 170, 90),
        Color32::from_rgb(240, 160, 40),
        Color32::from_rgb(170, 90, 220),
        Color32::from_rgb(40, 190, 190),
        Color32::from_rgb(220, 110, 170),
        Color32::from_rgb(140, 160, 70),
    ];
    COLORS[i % COLORS.len()]
}

struct FileJob {
    total: usize,
    sent: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

struct PlotChannel {
    name: String,
    color: Color32,
    points: VecDeque<(f64, f64)>,
    last: f64,
}

pub struct App {
    cfg: Config,
    ctx: egui::Context,
    fonts: Vec<fonts::FontFace>,
    conn: Option<(std::sync::mpsc::Sender<Cmd>, std::sync::mpsc::Receiver<Evt>)>,
    state: u8, // 0 closed, 1 opening, 2 open
    status: String,
    error: bool,
    ports: Vec<String>,
    rx: Rx,
    rows: Vec<Row>,
    dirty: bool,
    force: bool,
    last_view: Instant,
    follow_tail: bool,
    view_h: f32,
    display_h: f32,
    view_cursor: usize,
    view_decoder: text::Decoder,
    view_line: String,
    view_open_row: bool,
    view_last_t: f64,
    view_last_tx: Option<bool>,
    view_dropped: u64,
    view_key: (bool, u8, u32),
    send_text: String,
    send_just_sent: bool,
    tx_bytes: u64,
    rx_bytes: u64,
    recv_file: Option<File>,
    recv_to_file: bool,
    file_job: Option<FileJob>,
    last_file_sent: usize,
    last_timed: Instant,
    loop_on: bool,
    loop_current: Option<usize>,
    loop_next: Instant,
    stats: (f32, f32),
    last_stats: Instant,
    gl: String,
    viewport: Option<egui::Rect>,
    show_quick: bool,
    show_serial: bool,
    show_display: bool,
    show_about: bool,
    show_help: bool,
    show_plot: bool,
    plot_channels: Vec<PlotChannel>,
    plot_decoder: text::Decoder,
    plot_mode: u8,
    plot_buf: String,
    plot_re: Option<regex::Regex>,
    plot_rule_compiled: String,
    plot_error: Option<String>,
    plot_t0: Instant,
    plot_last_prune: Instant,
}

const FONT_CHOICES: [&str; 6] = ["宋体", "黑体", "微软雅黑", "楷体", "仿宋", "Consolas"];

const QQ_GROUP_URL: &str = r"https://qun.qq.com/universal-share/share?ac=1&authKey=Md3YpJnHlArQugW1HGukAUEnEXE33uO9X5pEpI81U1z%2F9ZTrxoNag6mWra8b4vM6&busi_data=eyJncm91cENvZGUiOiI5NDc5NjMyODgiLCJ0b2tlbiI6Im8wRnFiZTJMeFovMGV3ZmorVExUUjdaMHFiYWNtMUhpaENXUGJpaDFDZStxNHdEa2RNWGh4K2hOY2JXR1IwSC8iLCJ1aW4iOiIxMjY5MjQ5MSJ9&data=gh7wTAMS5_JIsk9X8fI2Oha64ItGZzskgv5JqS1P8ViCv3yEQKbPauIWimCthRqX7viAd668OlO7xV54dhCmsQ&svctype=4&tempid=h5_group_info";

fn font_files(name: &str) -> Vec<(&'static str, u32)> {
    match name {
        "宋体" => vec![(r"C:\Windows\Fonts\simsun.ttc", 0)],
        "黑体" => vec![(r"C:\Windows\Fonts\simhei.ttf", 0)],
        "微软雅黑" => vec![(r"C:\Windows\Fonts\msyh.ttc", 0)],
        "楷体" => vec![(r"C:\Windows\Fonts\simkai.ttf", 0)],
        "仿宋" => vec![(r"C:\Windows\Fonts\simfang.ttf", 0)],
        "Consolas" => vec![(r"C:\Windows\Fonts\consola.ttf", 0)],
        _ => vec![],
    }
}

/// Load the selected font from the operating system's registered fonts.
/// The embedded subset is only a fallback for the UI if no system font is
/// readable (for example on a stripped-down system).
fn install_fonts(ctx: &egui::Context, wanted: &str) -> String {
    let mut fonts = FontDefinitions::default();
    let mut used = String::new();
    let mut order: Vec<&str> = vec![wanted];
    for fallback in ["宋体", "微软雅黑", "黑体"] {
        if fallback != wanted {
            order.push(fallback);
        }
    }
    'outer: for name in order {
        for (path, index) in font_files(name) {
            // Memory-map the system font: its glyph tables are read on demand
            // and the pages stay file-backed instead of being copied into the
            // process heap (SimSun is ~17 MB).
            let Ok(file) = File::open(path) else { continue };
            let Ok(mmap) = (unsafe { memmap2::Mmap::map(&file) }) else { continue };
            if mmap.len() < 1024 {
                continue;
            }
            let leaked: &'static memmap2::Mmap = Box::leak(Box::new(mmap));
            let mut data = FontData::from_static(&leaked[..]);
            data.index = index;
            fonts.font_data.insert("system".to_owned(), Arc::new(data));
            used = name.to_owned();
            break 'outer;
        }
    }
    if !used.is_empty() {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "system".to_owned());
    }

    fonts.font_data.insert(
        "fallback".to_owned(),
        Arc::new(FontData::from_static(include_bytes!("../assets/ui_cjk.ttf"))),
    );
    fonts.font_data.insert(
        "mono".to_owned(),
        Arc::new(FontData::from_static(include_bytes!("../assets/ui_mono.ttf"))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("fallback".to_owned());
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.push("mono".to_owned());
    mono.push("fallback".to_owned());

    ctx.set_fonts(fonts);
    if used.is_empty() {
        "内置".to_owned()
    } else {
        used
    }
}

fn theme_visuals(dark: bool) -> egui::Visuals {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    if dark {
        v.panel_fill = Color32::from_rgb(34, 36, 40);
        v.extreme_bg_color = Color32::from_rgb(24, 25, 28);
        v.window_fill = Color32::from_rgb(38, 40, 45);
    } else {
        v.panel_fill = Color32::from_rgb(240, 240, 240);
        v.extreme_bg_color = Color32::WHITE;
    }
    v
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut st = (*ctx.style()).clone();
    st.visuals = theme_visuals(dark);
    ctx.set_style(st);
}

fn install_style(ctx: &egui::Context) {
    let mut st = (*ctx.style()).clone();
    st.text_styles = [
        (TextStyle::Heading, FontId::proportional(16.0)),
        (TextStyle::Body, FontId::proportional(12.0)),
        (TextStyle::Button, FontId::proportional(12.0)),
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ].into_iter().collect();
    st.spacing.item_spacing = Vec2::new(4.0, 4.0);
    st.spacing.button_padding = Vec2::new(6.0, 3.0);
    st.spacing.interact_size.y = 20.0;
    st.visuals = theme_visuals(false);
    ctx.set_style(st);
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: Config) -> Self {
        let font_catalog = fonts::catalog();
        let selected = fonts::default_name(&font_catalog, &cfg.font_name);
        fonts::install(&cc.egui_ctx, &font_catalog, &selected);
        cfg.font_name = selected;
        install_style(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, cfg.dark);
        if cfg.dark {
            if cfg.bg == [255, 255, 255] {
                cfg.bg = [30, 30, 30];
                cfg.fg = [220, 220, 220];
            }
        } else if cfg.bg == [30, 30, 30] {
            cfg.bg = [255, 255, 255];
            cfg.fg = [0, 0, 0];
        }
        let gl = cc.gl.as_ref().map(|g| unsafe { g.get_parameter_string(eframe::glow::VERSION) })
            .unwrap_or_else(|| "OpenGL".into());
        let mut app = Self {
            send_text: String::new(),
            send_just_sent: false,
            ctx: cc.egui_ctx.clone(),
            fonts: font_catalog,
            show_quick: cfg.show_quick,
            rx: Rx::new(cfg.rx_limit),
            rows: Vec::new(),
            dirty: true,
            force: true,
            last_view: Instant::now(),
            follow_tail: true,
            view_h: 300.0,
            display_h: 0.0,
            view_cursor: 0,
            view_decoder: text::Decoder::new(EncMode::from_u8(cfg.encoding)),
            view_line: String::new(),
            view_open_row: false,
            view_last_t: f64::NEG_INFINITY,
            view_last_tx: None,
            view_dropped: 0,
            view_key: (cfg.timestamps, cfg.encoding, cfg.timeout_ms),
            status: String::from("就绪"),
            error: false,
            state: 0,
            conn: None,
            ports: Vec::new(),
            tx_bytes: 0,
            rx_bytes: 0,
            recv_file: None,
            recv_to_file: false,
            file_job: None,
            last_file_sent: 0,
            last_timed: Instant::now(),
            loop_on: false,
            loop_current: None,
            loop_next: Instant::now(),
            stats: win::memory_mb(),
            last_stats: Instant::now(),
            gl,
            viewport: None,
            show_serial: false,
            show_display: false,
            show_about: false,
            show_help: false,
            show_plot: false,
            plot_channels: Vec::new(),
            plot_decoder: text::Decoder::new(EncMode::from_u8(cfg.encoding)),
            plot_mode: cfg.encoding,
            plot_buf: String::new(),
            plot_re: None,
            plot_rule_compiled: String::new(),
            plot_error: None,
            plot_t0: Instant::now(),
            plot_last_prune: Instant::now(),
            cfg,
        };
        app.refresh_ports();
        app.plot_compile();
        if app.cfg.auto_open {
            app.open();
            cc.egui_ctx.request_repaint();
        }
        app
    }

    fn status(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }

    fn refresh_ports(&mut self) {
        self.ports = serialport::available_ports()
            .map(|v| v.into_iter().map(|p| p.port_name).collect())
            .unwrap_or_default();
        self.ports.sort();
        if !self.ports.is_empty() && !self.ports.iter().any(|p| p == &self.cfg.port) {
            self.cfg.port = self.ports[0].clone();
        }
    }

    fn open(&mut self) {
        if self.conn.is_some() {
            return;
        }
        if self.cfg.net_mode == 0 && self.cfg.port.trim().is_empty() {
            self.status("没有可用串口", true);
            return;
        }
        let (tx, rx) = transport::spawn(&self.cfg);
        self.conn = Some((tx, rx));
        self.state = 1;
        self.status("正在打开...", false);
    }

    fn close(&mut self) {
        if let Some((tx, _)) = self.conn.take() {
            let _ = tx.send(Cmd::Stop);
        }
        self.state = 0;
        self.status("已关闭", false);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some((tx, rx)) = self.conn.take() else { return };
        let mut keep = true;
        loop {
            match rx.try_recv() {
                Ok(Evt::Opened(s)) => {
                    self.state = 2;
                    self.status(s, false);
                    self.last_timed = Instant::now();
                }
                Ok(Evt::Data(d)) => {
                    self.plot_feed(&d);
                    self.rx_bytes += d.len() as u64;
                    if let Some(f) = self.recv_file.as_mut() {
                        let _ = f.write_all(&d);
                    }
                    self.rx.add(config::timestamp_ms(), d);
                    self.dirty = true;
                    ctx.request_repaint();
                }
                Ok(Evt::Info(s)) => self.status(s, false),
                Ok(Evt::Reconnecting(s)) => {
                    self.state = 1;
                    self.status(s, true);
                }
                Ok(Evt::Error(e)) => {
                    self.status(e, true);
                    self.state = 0;
                    keep = false;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    keep = false;
                    self.state = 0;
                    break;
                }
            }
        }
        if keep {
            self.conn = Some((tx, rx));
        }
        ctx.request_repaint();
    }

    fn write_data(&mut self, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }
        let Some((tx, _)) = self.conn.as_ref() else {
            self.status("未连接，无法发送", true);
            return;
        };
        if self.state != 2 {
            self.status("未连接，无法发送", true);
            return;
        }
        let n = data.len() as u64;
        let echo = data.clone();
        if tx.send(Cmd::Write(data)).is_err() {
            self.status("发送失败", true);
            return;
        }
        self.tx_bytes += n;
        // Echo the transmitted bytes into the same timeline as RX, marked as TX.
        self.rx.add_tx(config::timestamp_ms(), echo);
        self.dirty = true;
    }

    fn send_current(&mut self) {
        if self.send_text.is_empty() {
            return;
        }
        let mode = EncMode::from_u8(self.cfg.encoding);
        let raw = if self.cfg.hex_send {
            match checksum::parse_hex(&self.send_text) {
                Ok(v) => v,
                Err(e) => {
                    self.status(format!("HEX 解析失败: {e}"), true);
                    return;
                }
            }
        } else {
            text::encode(&self.send_text, mode)
        };
        let mut data = checksum::apply(
            &raw,
            ChecksumKind::from_u8(self.cfg.checksum),
            self.cfg.checksum_start,
            self.cfg.checksum_tail,
        );
        if self.cfg.newline {
            data.extend_from_slice(b"\r\n");
        }
        self.write_data(data);
        self.send_just_sent = true;
        self.last_timed = Instant::now();
    }

    fn send_quick(&mut self, idx: usize) {
        let Some(q) = self.cfg.quick.get(idx) else { return };
        let mode = EncMode::from_u8(self.cfg.encoding);
        let result = if q.hex {
            checksum::parse_hex(&q.data)
        } else {
            Ok(text::encode(&config::unescape(&q.data), mode))
        };
        match result {
            Ok(d) => self.write_data(d),
            Err(e) => self.status(format!("第 {} 条: {e}", idx + 1), true),
        }
    }

    fn next_quick(&self) -> Option<usize> {
        let mut items: Vec<(u32, usize)> = self.cfg.quick.iter().enumerate()
            .filter(|(_, q)| q.order > 0).map(|(i, q)| (q.order, i)).collect();
        if items.is_empty() {
            return None;
        }
        items.sort_by_key(|(o, _)| *o);
        if let Some(cur) = self.loop_current {
            let o = self.cfg.quick.get(cur).map(|q| q.order).unwrap_or(0);
            if let Some((_, i)) = items.iter().find(|(x, _)| *x > o) {
                return Some(*i);
            }
        }
        Some(items[0].1)
    }

    fn start_file(&mut self) {
        if self.cfg.file_path.is_empty() || !std::path::Path::new(&self.cfg.file_path).exists() {
            self.status("请先选择要发送的文件", true);
            return;
        }
        if self.state != 2 {
            self.status("请先打开串口/网络", true);
            return;
        }
        let Some((tx, _)) = self.conn.as_ref() else { return };
        let tx = tx.clone();
        let path = self.cfg.file_path.clone();
        let total = std::fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
        if total == 0 {
            self.status("文件为空", true);
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let (stop2, sent2, done2) = (stop.clone(), sent.clone(), done.clone());
        let delay = self.cfg.interval.max(1) as u64;
        let _ = std::thread::Builder::new().name("cxcom-file".into()).spawn(move || {
            if let Ok(mut f) = File::open(&path) {
                let mut buf = [0u8; 256];
                loop {
                    if stop2.load(Ordering::Relaxed) {
                        break;
                    }
                    let n = std::io::Read::read(&mut f, &mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    if tx.send(Cmd::Write(buf[..n].to_vec())).is_err() {
                        break;
                    }
                    sent2.fetch_add(n, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(delay.min(200)));
                }
            }
            done2.store(true, Ordering::Relaxed);
        });
        self.last_file_sent = 0;
        self.file_job = Some(FileJob { total, sent, stop, done });
        self.status("正在发送文件...", false);
    }

    fn stop_all(&mut self) {
        self.cfg.timed = false;
        self.loop_on = false;
        if let Some(j) = &self.file_job {
            j.stop.store(true, Ordering::Relaxed);
        }
        self.status("已停止发送", false);
    }

    fn dirty(&mut self) {
        self.dirty = true;
        self.force = true;
    }

    fn rebuild_all(&mut self) {
        if !self.force && self.last_view.elapsed() < Duration::from_millis(40) {
            return;
        }
        let mode = EncMode::from_u8(self.cfg.encoding);
        let opts = RenderOpts {
            hex: self.cfg.hex_display,
            timestamps: self.cfg.timestamps,
            timeout_ms: self.cfg.timeout_ms,
            mode,
            fg: Color32::from_rgb(self.cfg.fg[0], self.cfg.fg[1], self.cfg.fg[2]),
            tx: Color32::from_rgb(0, 80, 200),
            header: Color32::from_rgb(100, 100, 100),
            cap: self.rx.limit(),
            max_rows: VIEW_MAX_ROWS,
        };
        self.rows = self.rx.render(&opts).0;
        self.display_h = 0.0;
        self.view_cursor = self.rx.chunks().len();
        self.view_decoder = text::Decoder::new(mode);
        self.view_line.clear();
        self.view_open_row = false;
        self.view_last_t = self.rx.chunks().back().map(|c| c.t).unwrap_or(f64::NEG_INFINITY);
        self.view_last_tx = self.rx.chunks().back().map(|c| c.tx);
        self.view_dropped = self.rx.dropped();
        self.view_key = (self.cfg.timestamps, self.cfg.encoding, self.cfg.timeout_ms);
        self.dirty = false;
        self.force = false;
        self.last_view = Instant::now();
    }

    /// Append only the newly received chunks. The view behaves like a memo:
    /// the tail grows at the bottom and the viewport follows it.
    fn refresh_view(&mut self) {
        if !self.dirty && !self.force {
            return;
        }
        let key = (self.cfg.timestamps, self.cfg.encoding, self.cfg.timeout_ms);
        if self.force
            || self.cfg.hex_display
            || key != self.view_key
            || self.rx.dropped() != self.view_dropped
            || self.view_cursor > self.rx.chunks().len()
        {
            self.rebuild_all();
            return;
        }

        let count = self.rx.chunks().len();
        if self.view_cursor >= count {
            self.dirty = false;
            return;
        }

        let fg = Color32::from_rgb(self.cfg.fg[0], self.cfg.fg[1], self.cfg.fg[2]);
        let header = Color32::from_rgb(100, 100, 100);
        let timeout_s = self.cfg.timeout_ms as f64 / 1000.0;
        let timestamps = self.cfg.timestamps;
        let mut needs_rebuild = false;

        {
            let chunks = self.rx.chunks();
            for i in self.view_cursor..count {
                let c = &chunks[i];
                if timestamps {
                    let new_group = c.t - self.view_last_t > timeout_s
                        || self.view_last_tx != Some(c.tx);
                    if new_group {
                        self.rows.push(Row {
                            text: format!(
                                "[{}] {}",
                                crate::buffer::fmt_time(c.time),
                                if c.tx { "<--发-->" } else { "-->收<--" }
                            ),
                            color: header,
                        });
                        self.view_last_t = c.t;
                        self.view_last_tx = Some(c.tx);
                        self.view_line.clear();
                        self.view_open_row = false;
                    }
                }

                let text = self.view_decoder.decode(&c.data);
                for ch in text.chars() {
                    if ch == '\n' {
                        // Finish the current line in place; a following chunk
                        // in the same packet will start a new row.
                        if self.view_open_row && !self.rows.is_empty() {
                            if let Some(last) = self.rows.last_mut() {
                                last.text = crate::buffer::sanitize(&self.view_line);
                            }
                        } else {
                            self.rows.push(Row {
                                text: crate::buffer::sanitize(&self.view_line),
                                color: fg,
                            });
                        }
                        self.view_line.clear();
                        self.view_open_row = false;
                    } else if ch != '\r' {
                        self.view_line.push(ch);
                    }
                }

                if !self.view_line.is_empty() {
                    // Keep one live partial line. This is the key for data
                    // without newlines that arrives split over several reads:
                    // append to the same row instead of starting a new line.
                    if self.view_open_row && !self.rows.is_empty() {
                        if let Some(last) = self.rows.last_mut() {
                            last.text = crate::buffer::sanitize(&self.view_line);
                        }
                    } else {
                        self.rows.push(Row {
                            text: crate::buffer::sanitize(&self.view_line),
                            color: fg,
                        });
                        self.view_open_row = true;
                    }
                }
            }
            self.view_cursor = count;
            if self.rows.len() > 20_000 {
                needs_rebuild = true;
            }
        }

        if needs_rebuild {
            self.force = true;
            self.rebuild_all();
        } else {
            self.dirty = false;
            self.last_view = Instant::now();
        }
    }

    /// 按需把所有行拼成一段文本（复制全部 / 保存数据时）。
    /// 平时只保留逐行数据，避免同时驻留两份大文本。
    fn display_text_all(&self) -> String {
        let cap: usize = self.rows.iter().map(|r| r.text.len() + 1).sum();
        let mut s = String::with_capacity(cap);
        for (i, r) in self.rows.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&r.text);
        }
        s
    }

    fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.cfg.dark = !self.cfg.dark;
        apply_theme(ctx, self.cfg.dark);
        if self.cfg.dark {
            self.cfg.bg = [30, 30, 30];
            self.cfg.fg = [220, 220, 220];
        } else {
            self.cfg.bg = [255, 255, 255];
            self.cfg.fg = [0, 0, 0];
        }
        let _ = self.cfg.save();
        self.status(if self.cfg.dark { "已切换深色主题" } else { "已切换浅色主题" }, false);
        ctx.request_repaint();
    }

    fn plot_compile(&mut self) {
        self.plot_rule_compiled = self.cfg.plot_rule.clone();
        match regex::Regex::new(&self.cfg.plot_rule) {
            Ok(re) => {
                self.plot_re = Some(re);
                self.plot_error = None;
            }
            Err(e) => {
                self.plot_re = None;
                self.plot_error = Some(format!("正则错误: {e}"));
            }
        }
    }

    fn plot_feed(&mut self, data: &[u8]) {
        if !self.cfg.plot_enabled || !self.show_plot {
            return;
        }
        if self.plot_mode != self.cfg.encoding {
            self.plot_mode = self.cfg.encoding;
            self.plot_decoder = text::Decoder::new(EncMode::from_u8(self.cfg.encoding));
            self.plot_buf.clear();
        }
        let text = self.plot_decoder.decode(data);
        self.plot_buf.push_str(&text);
        if self.plot_buf.len() > 64 * 1024 {
            let cut = self.plot_buf.len() - 32 * 1024;
            self.plot_buf.drain(..cut);
        }
        while let Some(pos) = self.plot_buf.find('\n') {
            let line = self.plot_buf[..pos].trim_end_matches('\r').to_string();
            self.plot_buf.drain(..=pos);
            self.plot_parse_line(&line);
        }
    }

    fn plot_prune(&mut self) {
        let now = self.plot_t0.elapsed().as_secs_f64();
        let cutoff = now - 35.0;
        for ch in &mut self.plot_channels {
            while let Some(&(x, _)) = ch.points.front() {
                if x < cutoff || ch.points.len() > 3000 {
                    ch.points.pop_front();
                } else {
                    break;
                }
            }
            if ch.points.capacity() > ch.points.len() * 2 + 64 {
                ch.points.shrink_to_fit();
            }
        }
        // Drop channels that have been silent for the whole window. This also
        // releases the VecDeque backing allocation instead of keeping it.
        self.plot_channels.retain(|c| !c.points.is_empty());
        if self.plot_buf.capacity() > 64 * 1024 {
            self.plot_buf.shrink_to_fit();
        }
    }

    fn plot_parse_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        if self.plot_re.is_none() || self.plot_rule_compiled != self.cfg.plot_rule {
            self.plot_compile();
        }
        let Some(re) = self.plot_re.as_ref() else { return };
        let mut found: Vec<(String, f64)> = Vec::new();
        for caps in re.captures_iter(line) {
            let value = caps
                .name("value")
                .and_then(|m| m.as_str().trim().parse::<f64>().ok())
                .or_else(|| {
                    caps.iter()
                        .skip(1)
                        .flatten()
                        .filter_map(|m| m.as_str().trim().parse::<f64>().ok())
                        .last()
                });
            let Some(value) = value else { continue };
            let mut label = caps.name("label").map(|m| m.as_str().trim().to_string());
            if label.is_none() {
                if let Some(m) = caps.get(1) {
                    let s = m.as_str().trim();
                    if s.parse::<f64>().is_err() && !s.is_empty() {
                        label = Some(s.to_string());
                    }
                }
            }
            let label = label
                .filter(|s| s.chars().count() <= 24)
                .unwrap_or_else(|| "值".to_string());
            found.push((label, value));
        }
        if found.is_empty() {
            return;
        }
        let now = self.plot_t0.elapsed().as_secs_f64();
        let cutoff = now - 35.0;
        for (name, value) in found {
            if let Some(ch) = self.plot_channels.iter_mut().find(|c| c.name == name) {
                ch.last = value;
                ch.points.push_back((now, value));
                while ch.points.len() > 3000 {
                    ch.points.pop_front();
                }
                while let Some(&(x, _)) = ch.points.front() {
                    if x < cutoff {
                        ch.points.pop_front();
                    } else {
                        break;
                    }
                }
            } else {
                if self.plot_channels.len() >= 24 {
                    // Replace the least recently updated channel instead of
                    // refusing new data and keeping stale allocations forever.
                    if let Some(pos) = self
                        .plot_channels
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| {
                            let ax = a.points.back().map(|p| p.0).unwrap_or(0.0);
                            let bx = b.points.back().map(|p| p.0).unwrap_or(0.0);
                            ax.total_cmp(&bx)
                        })
                        .map(|(i, _)| i)
                    {
                        self.plot_channels.remove(pos);
                    }
                }
                let idx = self.plot_channels.len();
                let mut points = VecDeque::new();
                points.push_back((now, value));
                self.plot_channels.push(PlotChannel {
                    name,
                    color: plot_color(idx),
                    points,
                    last: value,
                });
            }
        }
        if self.plot_last_prune.elapsed() >= Duration::from_secs(1) {
            self.plot_prune();
            self.plot_last_prune = Instant::now();
        }
    }

    fn plot_reset(&mut self) {
        self.plot_channels = Vec::new();
        self.plot_buf = String::new();
        self.plot_last_prune = Instant::now();
    }

    fn plot_window(&mut self, ctx: &egui::Context) {
        if !self.show_plot {
            return;
        }
        let mut open = self.show_plot;
        let mut clear = false;
        egui::Window::new("串口绘图")
            .open(&mut open)
            .default_size([700.0, 420.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cfg.plot_enabled, "启用解析");
                    if ui.button("清空").clicked() {
                        clear = true;
                    }
                    ui.label(format!("{} 通道", self.plot_channels.len()));
                    ui.weak("每行可匹配多个 name:value");
                    ui.add(
                        egui::Slider::new(&mut self.cfg.plot_hover_size, 12.0..=24.0)
                            .text("悬停字号")
                            .fixed_decimals(0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("规则:");
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.cfg.plot_rule)
                            .desired_width(f32::INFINITY)
                            .font(TextStyle::Monospace),
                    );
                    if resp.changed() {
                        self.plot_rule_compiled.clear();
                        self.plot_error = None;
                    }
                });
                if let Some(err) = &self.plot_error {
                    ui.colored_label(Color32::from_rgb(190, 40, 40), err);
                }
                ui.weak("默认匹配 Temp: 25.3, Hum: 60；可选命名组 label/value");
                ui.separator();
                self.draw_plot(ui);
            });
        if clear {
            self.plot_reset();
        }
        self.show_plot = open;
        if !open {
            self.plot_reset();
        }
    }

    fn draw_plot(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for ch in &self.plot_channels {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, ch.color);
                ui.label(format!("{} {:.2}", ch.name, ch.last));
            }
        });
        let height = ui.available_height().max(160.0);
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), height),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        let dark = ui.visuals().dark_mode;
        let bg = if dark { Color32::from_rgb(18, 20, 24) } else { Color32::from_rgb(250, 251, 253) };
        let grid = if dark { Color32::from_gray(62) } else { Color32::from_gray(220) };
        let text = ui.visuals().text_color();
        painter.rect_filled(rect, 4.0, bg);
        if self.plot_channels.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "等待匹配的数据...",
                FontId::proportional(14.0),
                Color32::from_gray(140),
            );
            return;
        }
        let plot = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 52.0, rect.top() + 8.0),
            egui::pos2(rect.right() - 10.0, rect.bottom() - 22.0),
        );
        let now = self.plot_t0.elapsed().as_secs_f64();
        let xmax = now.max(5.0);
        let xmin = (xmax - 30.0).max(0.0);
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        for ch in &self.plot_channels {
            for &(x, y) in ch.points.iter().filter(|(x, _)| *x >= xmin) {
                ymin = ymin.min(y);
                ymax = ymax.max(y);
            }
        }
        if !ymin.is_finite() || !ymax.is_finite() {
            ymin = -1.0;
            ymax = 1.0;
        }
        if (ymax - ymin).abs() < 1e-9 {
            ymin -= 1.0;
            ymax += 1.0;
        }
        let pad = (ymax - ymin) * 0.1;
        ymin -= pad;
        ymax += pad;
        let map = |x: f64, y: f64| {
            egui::pos2(
                plot.left() + ((x - xmin) / (xmax - xmin)) as f32 * plot.width(),
                plot.bottom() - ((y - ymin) / (ymax - ymin)) as f32 * plot.height(),
            )
        };
        for i in 0..=4 {
            let t = i as f64 / 4.0;
            let y = plot.bottom() - t as f32 * plot.height();
            painter.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                egui::Stroke::new(1.0_f32, grid),
            );
            painter.text(
                egui::pos2(plot.left() - 5.0, y),
                Align2::RIGHT_CENTER,
                format!("{:.2}", ymin + t * (ymax - ymin)),
                FontId::monospace(9.0),
                text,
            );
        }
        for i in 0..=5 {
            let t = i as f64 / 5.0;
            let x = plot.left() + t as f32 * plot.width();
            painter.line_segment(
                [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
                egui::Stroke::new(1.0_f32, grid),
            );
            let secs = xmin + t * (xmax - xmin);
            painter.text(
                egui::pos2(x, plot.bottom() + 3.0),
                Align2::CENTER_TOP,
                format!("{:.0}s", secs - xmax),
                FontId::monospace(9.0),
                text,
            );
        }
        let clipped = painter.with_clip_rect(plot);
        for ch in &self.plot_channels {
            let pts: Vec<egui::Pos2> = ch
                .points
                .iter()
                .filter(|(x, _)| *x >= xmin)
                .map(|&(x, y)| map(x, y))
                .collect();
            if pts.len() == 1 {
                clipped.circle_filled(pts[0], 2.5, ch.color);
            } else if pts.len() > 1 {
                clipped.add(egui::Shape::line(pts, egui::Stroke::new(1.6_f32, ch.color)));
            }
        }

        // Hover readout: vertical guide + nearest sample of each channel.
        if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
            if plot.contains(pointer) {
                let target = xmin
                    + ((pointer.x - plot.left()) / plot.width().max(1.0)) as f64
                        * (xmax - xmin);
                let cx = pointer.x.clamp(plot.left(), plot.right());
                clipped.line_segment(
                    [
                        egui::pos2(cx, plot.top()),
                        egui::pos2(cx, plot.bottom()),
                    ],
                    egui::Stroke::new(1.0_f32, Color32::from_gray(150)),
                );
                let mut hovered: Vec<(String, f64, Color32)> = Vec::new();
                for ch in &self.plot_channels {
                    if let Some((x, y)) = ch
                        .points
                        .iter()
                        .min_by(|a, b| {
                            (a.0 - target).abs().total_cmp(&(b.0 - target).abs())
                        })
                    {
                        clipped.circle_filled(map(*x, *y), 3.0, ch.color);
                        hovered.push((ch.name.clone(), *y, ch.color));
                    }
                }
                if !hovered.is_empty() {
                    // Configurable 12..24 readout size (default 18).
                    let tip_size = self.cfg.plot_hover_size.clamp(12.0, 24.0);
                    let line_h = tip_size + 6.0;
                    let box_w = (tip_size * 12.0)
                        .clamp(240.0, (plot.width() - 4.0).max(200.0));
                    let max_rows = (((plot.height() - line_h - 12.0) / line_h)
                        .floor() as usize)
                        .clamp(1, 8);
                    let shown = hovered.len().min(max_rows);
                    let more = hovered.len() > shown;
                    let box_h = 10.0
                        + line_h
                        + shown as f32 * line_h
                        + if more { line_h * 0.8 } else { 0.0 };
                    let min_bx = plot.left() + 2.0;
                    let max_bx = (plot.right() - box_w - 2.0).max(min_bx);
                    let bx = if pointer.x + 12.0 + box_w < plot.right() {
                        pointer.x + 12.0
                    } else {
                        pointer.x - box_w - 12.0
                    }
                    .clamp(min_bx, max_bx);
                    let min_by = plot.top() + 2.0;
                    let max_by = (plot.bottom() - box_h - 2.0).max(min_by);
                    let by = (pointer.y + 12.0).clamp(min_by, max_by);
                    let tip = egui::Rect::from_min_size(
                        egui::pos2(bx, by),
                        Vec2::new(box_w, box_h),
                    );
                    let tip_bg = if dark {
                        Color32::from_rgba_unmultiplied(26, 29, 34, 242)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 246)
                    };
                    painter.rect_filled(tip, 5.0, tip_bg);
                    painter.text(
                        egui::pos2(bx + 10.0, by + 6.0),
                        Align2::LEFT_TOP,
                        format!("{:.2}s", target - xmax),
                        FontId::monospace(tip_size * 0.8),
                        text,
                    );
                    let rows_y = by + 8.0 + line_h;
                    for (i, (name, value, color)) in hovered.iter().take(shown).enumerate() {
                        let y = rows_y + i as f32 * line_h;
                        let dot = tip_size * 0.45;
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(bx + 10.0, y + (line_h - dot) * 0.5),
                                Vec2::splat(dot),
                            ),
                            1.5,
                            *color,
                        );
                        let name: String = name.chars().take(12).collect();
                        painter.text(
                            egui::pos2(bx + 10.0 + dot + 8.0, y + 1.0),
                            Align2::LEFT_TOP,
                            format!("{name}: {value:.2}"),
                            FontId::monospace(tip_size),
                            text,
                        );
                    }
                    if more {
                        painter.text(
                            egui::pos2(bx + 10.0, rows_y + shown as f32 * line_h),
                            Align2::LEFT_TOP,
                            format!("... +{} 通道", hovered.len() - shown),
                            FontId::monospace(tip_size * 0.75),
                            Color32::from_gray(150),
                        );
                    }
                }
            }
        }
    }

    fn save_cfg(&mut self) {
        if let Some(r) = self.viewport {
            self.cfg.win_x = r.min.x as i32;
            self.cfg.win_y = r.min.y as i32;
            self.cfg.win_w = r.width() as i32;
            self.cfg.win_h = r.height() as i32;
        }
        self.cfg.show_quick = self.show_quick;
        match self.cfg.save() {
            Ok(_) => self.status("参数已保存", false),
            Err(e) => self.status(e, true),
        }
    }

    fn toggle_conn(&mut self, ctx: &egui::Context) {
        if self.state == 0 {
            self.open();
        } else {
            self.close();
        }
        ctx.request_repaint();
    }

    // ------------------------------------------------------------ menus

    fn menus(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("通讯端口", |ui| {
                    if ui.button("刷新串口列表").clicked() {
                        self.refresh_ports();
                        ui.close();
                    }
                    ui.separator();
                    for (m, n) in [(1u8, "TCP Client"), (2, "TCP Server"), (3, "UDP")] {
                        if ui.selectable_label(self.cfg.net_mode == m, n).clicked() {
                            self.close();
                            self.cfg.net_mode = m;
                            ui.close();
                        }
                    }
                    if ui.selectable_label(self.cfg.net_mode == 0, "串口").clicked() {
                        self.close();
                        self.cfg.net_mode = 0;
                        ui.close();
                    }
                    ui.separator();
                    let ports = self.ports.clone();
                    if ports.is_empty() {
                        ui.weak("(没有检测到串口)");
                    }
                    for p in ports {
                        if ui.selectable_label(self.cfg.port == p, &p).clicked() {
                            self.close();
                            self.cfg.port = p;
                            ui.close();
                        }
                    }
                });
                ui.menu_button("串口设置", |ui| {
                    if ui.button("更多串口设置...").clicked() {
                        self.show_serial = true;
                        ui.close();
                    }
                    ui.checkbox(&mut self.cfg.auto_open, "启动时自动打开");
                    ui.checkbox(&mut self.cfg.auto_reconnect, "故障自动重连");
                });
                ui.menu_button("显示", |ui| {
                    if ui.button("字体/颜色/大小...").clicked() {
                        self.show_display = true;
                        ui.close();
                    }
                    ui.menu_button("显示缓冲上限", |ui| {
                        for (n, v) in LIMITS {
                            if ui.selectable_label(self.cfg.rx_limit == v, n).clicked() {
                                self.cfg.rx_limit = v;
                                self.rx.set_limit(v);
                                self.dirty();
                                ui.close();
                            }
                        }
                    });
                    if ui.checkbox(&mut self.cfg.hex_display, "HEX 显示").changed() {
                        self.dirty();
                    }
                    if ui.checkbox(&mut self.cfg.timestamps, "时间戳和分包显示").changed() {
                        self.dirty();
                    }
                });
                ui.menu_button("发送", |ui| {
                    ui.checkbox(&mut self.cfg.hex_send, "HEX 发送");
                    ui.checkbox(&mut self.cfg.newline, "加回车换行");
                    ui.checkbox(&mut self.cfg.timed, "定时发送");
                    ui.separator();
                    if ui.button("发送文件").clicked() {
                        self.start_file();
                    }
                    if ui.button("停止发送").clicked() {
                        self.stop_all();
                    }
                });
                ui.menu_button("工具", |ui| {
                    if ui.checkbox(&mut self.show_quick, "快捷指令").changed() {
                        self.cfg.show_quick = self.show_quick;
                    }
                    if ui.button("绘图").clicked() {
                        self.show_plot = !self.show_plot;
                        if !self.show_plot {
                            self.plot_reset();
                        }
                        ui.close();
                    }
                    ui.add_enabled(false, egui::Button::new("开发中..."));
                });
                ui.menu_button("帮助", |ui| {
                    if ui.button("加入我们（QQ 群）").clicked() {
                        win::open_url(QQ_GROUP_URL);
                        self.status("正在打开 QQ 群链接...", false);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("使用说明").clicked() {
                        self.show_help = true;
                        ui.close();
                    }
                    if ui.button("关于CXCOM").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dark = self.cfg.dark;
                    let (label, fill, text) = if dark {
                        ("浅色", Color32::from_rgb(210, 214, 222), Color32::from_rgb(30, 32, 36))
                    } else {
                        ("深色", Color32::from_rgb(52, 56, 64), Color32::from_rgb(240, 242, 246))
                    };
                    let button = egui::Button::new(
                        egui::RichText::new(label).size(12.0).color(text),
                    )
                    .fill(fill)
                    .corner_radius(11)
                    .min_size(Vec2::new(58.0, 22.0));
                    if ui.add(button).on_hover_text("切换浅色 / 深色主题").clicked() {
                        self.toggle_theme(ctx);
                    }
                });
            });
        });
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("清除窗口").clicked() {
                    self.clear_display();
                    ctx.request_repaint();
                }
                if ui.button("打开文件").clicked() {
                    if let Some(p) = win::open_file_dialog(
                        "选择要发送的文件",
                        &[("所有文件", "*.*")],
                        &self.cfg.file_path,
                    ) {
                        self.cfg.file_path = p.display().to_string();
                    }
                }
                ui.add(TextEdit::singleline(&mut self.cfg.file_path).desired_width(190.0));
                if ui.button("发送文件").clicked() {
                    self.start_file();
                }
                if ui.button("停止").clicked() {
                    self.stop_all();
                }
                if ui.button("清发送区").clicked() {
                    self.send_text.clear();
                    self.send_just_sent = false;
                }
                if ui.button("保存参数").clicked() {
                    self.save_cfg();
                }
            });
        });
    }

    /// 端口号下拉框。
    fn ctl_port(&mut self, ui: &mut egui::Ui) {
        ui.label("端口号:");
        ui.add_enabled_ui(self.state == 0 && self.cfg.net_mode == 0, |ui| {
            combo_cell(ui, 170.0, |ui| {
                ComboBox::from_id_salt("port").width(170.0)
                    .selected_text(&self.cfg.port)
                    .show_ui(ui, |ui| {
                        let ports = self.ports.clone();
                        for p in ports {
                            ui.selectable_value(&mut self.cfg.port, p.clone(), p);
                        }
                    });
            });
        });
    }

    /// 打开/关闭连接按钮与 DTR / RTS。
    fn ctl_conn(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (label, color) = match self.state {
            2 => ("关闭连接", Color32::from_rgb(170, 40, 40)),
            1 => ("取消", Color32::from_rgb(180, 120, 0)),
            _ => ("打开连接", Color32::from_rgb(0, 120, 60)),
        };
        if ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(color)).clicked() {
            self.toggle_conn(ctx);
        }
        if self.cfg.net_mode == 0 {
            if ui.checkbox(&mut self.cfg.dtr, "DTR").changed() {
                if let Some((tx, _)) = self.conn.as_ref() {
                    let _ = tx.send(Cmd::Dtr(self.cfg.dtr));
                }
            }
            if ui.checkbox(&mut self.cfg.rts, "RTS").changed() {
                if let Some((tx, _)) = self.conn.as_ref() {
                    let _ = tx.send(Cmd::Rts(self.cfg.rts));
                }
            }
        }
    }

    /// 波特率下拉框（仅串口模式）。
    fn ctl_baud(&mut self, ui: &mut egui::Ui) {
        if self.cfg.net_mode == 0 {
            ui.label("波特率:");
            ui.add_enabled_ui(self.state == 0, |ui| {
                combo_cell(ui, 90.0, |ui| {
                    ComboBox::from_id_salt("baud").width(90.0)
                        .selected_text(self.cfg.baud.to_string())
                        .show_ui(ui, |ui| {
                            for b in BAUDS {
                                ui.selectable_value(&mut self.cfg.baud, b, b.to_string());
                            }
                        });
                });
            });
        }
    }

    /// 编码切换与“更多串口设置”入口。
    fn ctl_enc(&mut self, ui: &mut egui::Ui) {
        ui.label("编码:");
        let mut enc = self.cfg.encoding;
        combo_cell(ui, 80.0, |ui| {
            ComboBox::from_id_salt("enc").width(80.0)
                .selected_text(EncMode::from_u8(enc).name())
                .show_ui(ui, |ui| {
                    for (v, n) in [(0u8, "GB2312"), (1, "UTF-8"), (2, "自动")] {
                        ui.selectable_value(&mut enc, v, n);
                    }
                });
        });
        if enc != self.cfg.encoding {
            self.cfg.encoding = enc;
            self.dirty();
        }
        if ui.button("更多串口设置").clicked() {
            self.show_serial = true;
        }
    }

    /// 显示相关：HEX 显示、保存、接收存文件、时间戳。
    fn ctl_display_opts(&mut self, ui: &mut egui::Ui) {
        if ui.checkbox(&mut self.cfg.hex_display, "HEX 显示").changed() {
            self.dirty();
        }
        if ui.button("保存数据").clicked() {
            self.save_data();
        }
        let mut want = self.recv_to_file;
        if ui.checkbox(&mut want, "接收数据到文件").changed() {
            if want {
                if let Some(p) = win::save_file_dialog(
                    "接收数据保存到文件",
                    &[("二进制文件", "*.bin"), ("所有文件", "*.*")],
                    "",
                    "bin",
                ) {
                    match OpenOptions::new().create(true).append(true).open(&p) {
                        Ok(f) => {
                            self.recv_file = Some(f);
                            self.recv_to_file = true;
                        }
                        Err(e) => self.status(format!("无法打开文件: {e}"), true),
                    }
                }
            } else {
                self.recv_file = None;
                self.recv_to_file = false;
            }
        }
        if ui.checkbox(&mut self.cfg.timestamps, "时间戳和分包显示").changed() {
            self.dirty();
        }
    }

    /// 分包超时与显示缓冲上限。
    fn ctl_limit(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::DragValue::new(&mut self.cfg.timeout_ms).range(1..=5000).suffix(" ms"));
        ui.label("缓冲上限:");
        combo_cell(ui, 70.0, |ui| {
            ComboBox::from_id_salt("limit").width(70.0)
                .selected_text(limit_label(self.cfg.rx_limit))
                .show_ui(ui, |ui| {
                    for (n, v) in LIMITS {
                        if ui.selectable_value(&mut self.cfg.rx_limit, v, n).changed() {
                            self.rx.set_limit(v);
                            self.dirty();
                        }
                    }
                });
        });
    }

    /// 发送选项：HEX、回车换行、定时发送与间隔。
    fn ctl_send_opts(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.cfg.hex_send, "HEX 发送");
        ui.checkbox(&mut self.cfg.newline, "加回车换行");
        if ui.checkbox(&mut self.cfg.timed, "定时发送").changed() {
            self.last_timed = Instant::now();
        }
        ui.add(egui::DragValue::new(&mut self.cfg.interval).range(10..=600000).suffix(" ms/次"));
    }

    /// 校验范围与校验方式。
    fn ctl_checksum(&mut self, ui: &mut egui::Ui) {
        ui.label("第");
        ui.add(egui::DragValue::new(&mut self.cfg.checksum_start).range(1..=99999));
        ui.label("字节至");
        combo_cell(ui, 120.0, |ui| {
            ComboBox::from_id_salt("tail").width(120.0)
                .selected_text(["末尾(无帧尾)", "第-1", "第-2", "第-3", "第-4"][self.cfg.checksum_tail.min(4) as usize])
                .show_ui(ui, |ui| {
                    for (i, t) in ["末尾(无帧尾)", "第-1", "第-2", "第-3", "第-4"].iter().enumerate() {
                        ui.selectable_value(&mut self.cfg.checksum_tail, i as u32, *t);
                    }
                });
        });
        ui.label("加校验");
        combo_cell(ui, 105.0, |ui| {
            ComboBox::from_id_salt("sum").width(105.0)
                .selected_text(ChecksumKind::from_u8(self.cfg.checksum).name())
                .show_ui(ui, |ui| {
                    for (v, n) in [(0u8, "None"), (1, "ModbusCRC16"), (2, "ADD"), (3, "XOR")] {
                        ui.selectable_value(&mut self.cfg.checksum, v, n);
                    }
                });
        });
    }

    fn controls(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            // 先按当前文字量估算每个分组的宽度，再按可用宽度贪心排版：
            // 放得下就并排，放不下就整组换行。这样既不会把下拉框挤到窗口外，
            // 也不会在某些宽度下让某一行只剩一个控件、右边留出大片空白。
            let sp = ui.spacing().item_spacing.x;
            let pad = ui.spacing().button_padding.x;
            let icon = ui.spacing().icon_width + ui.spacing().icon_spacing;
            let tw = |s: &str| {
                ui.fonts(|f| {
                    f.layout_no_wrap(s.to_owned(), FontId::proportional(12.0), Color32::WHITE)
                        .size()
                        .x
                })
            };
            let btn = |s: &str| tw(s) + 2.0 * pad + 4.0;
            let chk = |s: &str| icon + tw(s) + 6.0;
            let mut widths = [0.0f32; 8];
            widths[0] = tw("端口号:") + 170.0 + sp + 12.0;
            widths[1] = btn("关闭连接") + chk("DTR") + chk("RTS") + 2.0 * sp + 12.0;
            widths[2] = if self.cfg.net_mode == 0 {
                tw("波特率:") + 90.0 + sp + 12.0
            } else {
                0.0
            };
            widths[3] = tw("编码:") + 80.0 + btn("更多串口设置") + 2.0 * sp + 12.0;
            widths[4] = chk("HEX 显示")
                + btn("保存数据")
                + chk("接收数据到文件")
                + chk("时间戳和分包显示")
                + 4.0 * sp
                + 12.0;
            widths[5] = 80.0 + tw("缓冲上限:") + 70.0 + 2.0 * sp + 12.0;
            widths[6] =
                chk("HEX 发送") + chk("加回车换行") + chk("定时发送") + 90.0 + 4.0 * sp + 12.0;
            widths[7] = tw("第")
                + 40.0
                + tw("字节至")
                + 120.0
                + tw("加校验")
                + 105.0
                + 6.0 * sp
                + 12.0;

            // 行数尽量少，并且在同样行数下让最宽的一行尽可能窄（各行更均衡，
            // 不会出现最后一行只剩一个控件、右边空一大片的情况）。
            let avail = ui.available_width();
            let groups: Vec<(usize, f32)> = widths
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, w)| *w > 0.0)
                .collect();
            let n = groups.len();
            let mut best: Option<(usize, f32, Vec<Vec<usize>>)> = None;
            for mask in 0u32..(1u32 << (n - 1)) {
                let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
                for i in 0..n {
                    if i > 0 && (mask >> (i - 1)) & 1 == 1 {
                        rows.push(Vec::new());
                    }
                    rows.last_mut().unwrap().push(groups[i].0);
                }
                let mut max_w: f32 = 0.0;
                let mut fits = true;
                for row in &rows {
                    let w: f32 = row
                        .iter()
                        .map(|&gi| widths[gi])
                        .sum::<f32>()
                        + sp * (row.len() as f32 - 1.0);
                    max_w = max_w.max(w);
                    if w > avail {
                        fits = false;
                        break;
                    }
                }
                if !fits {
                    continue;
                }
                let count = rows.len();
                let better = match &best {
                    None => true,
                    Some((bc, bw, _)) => count < *bc || (count == *bc && max_w < *bw),
                };
                if better {
                    best = Some((count, max_w, rows));
                }
            }
            let rows = best.map(|(_, _, r)| r).unwrap_or_else(|| {
                vec![groups.iter().map(|&(gi, _)| gi).collect()]
            });
            for row in rows {
                ui.horizontal(|ui| {
                    for i in row {
                        match i {
                            0 => self.ctl_port(ui),
                            1 => self.ctl_conn(ui, ctx),
                            2 => self.ctl_baud(ui),
                            3 => self.ctl_enc(ui),
                            4 => self.ctl_display_opts(ui),
                            5 => self.ctl_limit(ui),
                            6 => self.ctl_send_opts(ui),
                            _ => self.ctl_checksum(ui),
                        }
                    }
                });
            }

            ui.horizontal(|ui| {
                let w = (ui.available_width() - 90.0).max(200.0);
                let r = ui.add_sized(
                    [w, 56.0],
                    TextEdit::multiline(&mut self.send_text)
                        .font(TextStyle::Monospace)
                        .hint_text("输入要发送的内容（Ctrl+Enter 发送）"),
                );
                if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl) {
                    self.send_current();
                }
                if r.changed() {
                    self.send_just_sent = false;
                }
                let has_data = !self.send_text.trim().is_empty() && !self.send_just_sent;
                let button = if has_data {
                    egui::Button::new(
                        RichText::new("发 送")
                            .size(16.0)
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(42, 148, 74))
                } else {
                    egui::Button::new(RichText::new("发 送").size(16.0))
                };
                if ui.add_sized([82.0, 56.0], button).clicked() {
                    self.send_current();
                }
            });
            if let Some(j) = &self.file_job {
                ui.horizontal(|ui| {
                    let p = j.sent.load(Ordering::Relaxed) as f32 / j.total.max(1) as f32;
                    ui.add(egui::ProgressBar::new(p).desired_width(140.0).text(format!("{}/{}", j.sent.load(Ordering::Relaxed), j.total)));
                });
            }
        });
    }

    fn quick_panel(&mut self, ctx: &egui::Context) {
        if !self.show_quick {
            return;
        }
        let out = egui::SidePanel::right("quick")
            .resizable(true)
            .default_width(self.cfg.quick_w)
            .width_range(280.0..=720.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.weak("拖动加宽");
                    ui.checkbox(&mut self.loop_on, "循环发送");
                    if ui.button("AT预设").clicked() {
                        self.cfg.quick = config::at_presets();
                        self.status("已载入 ESP-01S / ESP8266 AT 指令", false);
                    }
                    if ui.button("说明").clicked() {
                        self.show_help = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.weak("顺序"); ui.weak("延时"); ui.weak("HEX");
                    ui.weak("注释 / 字符串");
                });
                ui.separator();
                let mut to_send = None;
                let len = self.cfg.quick.len();
                ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 22.0, len, |ui, range| {
                    for i in range {
                        let q = &mut self.cfg.quick[i];
                        ui.horizontal(|ui| {
                            ui.add_sized([34.0, 20.0], egui::DragValue::new(&mut q.order).range(0..=99));
                            ui.add_sized([50.0, 20.0], egui::DragValue::new(&mut q.delay).range(0..=600000).speed(10.0));
                            ui.checkbox(&mut q.hex, "");
                            ui.add_sized([65.0, 20.0], TextEdit::singleline(&mut q.comment).hint_text("注释").font(TextStyle::Small));
                            ui.add_sized([130.0, 20.0], TextEdit::singleline(&mut q.data).hint_text("数据").font(TextStyle::Monospace));
                            if ui.small_button("发送").clicked() {
                                to_send = Some(i);
                            }
                        });
                    }
                });
                if let Some(i) = to_send {
                    self.send_quick(i);
                }
            });
        self.cfg.quick_w = out.response.rect.width();
    }

    fn clear_display(&mut self) {
        self.rx.clear();
        self.rows.clear();
        self.view_line.clear();
        self.view_open_row = false;
        self.display_h = 0.0;
        self.view_cursor = 0;
        self.view_last_t = f64::NEG_INFINITY;
        self.view_last_tx = None;
        self.view_dropped = 0;
        self.rx_bytes = 0;
        self.tx_bytes = 0;
        self.dirty = false;
        self.follow_tail = true;
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let bg = Color32::from_rgb(self.cfg.bg[0], self.cfg.bg[1], self.cfg.bg[2]);
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(bg).inner_margin(4))
            .show(ctx, |ui| {
                self.refresh_view();
                let size = self.cfg.font_size;
                let follow = self.follow_tail;
                let tail_offset = (self.display_h - self.view_h).max(0.0);
                let mut copy_all = false;
                let mut clear = false;
                let mut to_bottom = false;

                // 关键：用 show_rows 按行虚拟化。每帧只对可视范围内的
                // 几十行做布局/栅格化，而不是把整个显示缓冲当一个
                // Label 一次性布局——否则 egui 的 galley 布局缓存和
                // tessellation 中间缓冲会随接收数据量线性增长。
                let row_h = ui.fonts(|f| f.row_height(&FontId::monospace(size)));
                let total = self.rows.len();
                let out = {
                    let mut area = ScrollArea::both().auto_shrink([false, false]);
                    if follow {
                        area = area.vertical_scroll_offset(tail_offset);
                    }
                    area.show_rows(ui, row_h, total, |ui, range| {
                        for i in range {
                            let row = &self.rows[i];
                            ui.add(
                                egui::Label::new(
                                    RichText::new(row.text.as_str())
                                        .font(FontId::monospace(size))
                                        .color(row.color),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .selectable(true),
                            );
                        }
                        // 覆盖全部可见行的响应，用于承载右键菜单。
                        ui.response()
                    })
                };

                out.inner.context_menu(|ui| {
                    if ui.button("复制全部").clicked() {
                        copy_all = true;
                        ui.close();
                    }
                    if ui.button("清空窗口").clicked() {
                        clear = true;
                        ui.close();
                    }
                    if ui.button("回到底部").clicked() {
                        to_bottom = true;
                        ui.close();
                    }
                });

                if copy_all {
                    let text = self.display_text_all();
                    if !text.is_empty() {
                        ctx.copy_text(text);
                        self.status("已复制全部显示内容", false);
                    }
                }
                if clear {
                    self.clear_display();
                    ctx.request_repaint();
                }
                if to_bottom {
                    self.follow_tail = true;
                    ctx.request_repaint();
                }

                self.display_h = out.content_size.y;
                self.view_h = out.inner_rect.height();
                let max_offset = (out.content_size.y - out.inner_rect.height()).max(0.0);
                let at_bottom = out.state.offset.y >= max_offset - 2.0;
                let hovered = ui.input(|i| i.pointer.hover_pos())
                    .map(|p| out.inner_rect.contains(p))
                    .unwrap_or(false);
                if hovered && ui.input(|i| i.raw_scroll_delta.y > 0.0) {
                    self.follow_tail = false;
                } else if at_bottom {
                    self.follow_tail = true;
                }

                if self.dirty {
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
            });
    }

    fn save_data(&mut self) {
        self.refresh_view();
        if self.rows.is_empty() {
            self.status("没有可保存的显示内容", true);
            return;
        }
        let stamp = config::timestamp().replace(':', "").replace('.', "");
        let name = format!("CXCOM_{stamp}.log");
        if let Some(p) = win::save_file_dialog(
            "保存显示内容",
            &[("日志文件", "*.log"), ("文本文件", "*.txt"), ("所有文件", "*.*")],
            &name,
            "log",
        ) {
            // Save exactly what is on screen: timestamps and HEX formatting
            // included when they are enabled, absent when they are disabled.
            let mut text = self.display_text_all().replace('\n', "\r\n");
            if !text.ends_with("\r\n") {
                text.push_str("\r\n");
            }
            let mut bytes = Vec::with_capacity(text.len() + 3);
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]); // UTF-8 BOM for Notepad
            bytes.extend_from_slice(text.as_bytes());
            match std::fs::write(&p, bytes) {
                Ok(_) => self.status(format!("已保存显示内容 {} 行到 {}", self.rows.len(), p.display()), false),
                Err(e) => self.status(format!("保存失败: {e}"), true),
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let mut open = self.show_serial;
        egui::Window::new("串口设置").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("数据位:");
                ComboBox::from_id_salt("db").selected_text(self.cfg.data_bits.to_string()).show_ui(ui, |ui| {
                    for v in [5u8, 6, 7, 8] {
                        ui.selectable_value(&mut self.cfg.data_bits, v, v.to_string());
                    }
                });
                ui.label("停止位:");
                ComboBox::from_id_salt("sb").selected_text(self.cfg.stop_bits.to_string()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cfg.stop_bits, 1, "1");
                    ui.selectable_value(&mut self.cfg.stop_bits, 2, "2");
                });
            });
            ui.horizontal(|ui| {
                ui.label("校验:");
                ComboBox::from_id_salt("pa").selected_text(["None", "Odd", "Even"][self.cfg.parity.min(2) as usize]).show_ui(ui, |ui| {
                    for (v, n) in [(0u8, "None"), (1, "Odd"), (2, "Even")] {
                        ui.selectable_value(&mut self.cfg.parity, v, n);
                    }
                });
                ui.label("流控:");
                ComboBox::from_id_salt("fl").selected_text(["None", "Xon/Xoff", "RTS/CTS"][self.cfg.flow.min(2) as usize]).show_ui(ui, |ui| {
                    for (v, n) in [(0u8, "None"), (1, "Xon/Xoff"), (2, "RTS/CTS")] {
                        ui.selectable_value(&mut self.cfg.flow, v, n);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("波特率:");
                ui.add(egui::DragValue::new(&mut self.cfg.baud).range(300..=6_000_000).speed(100));
            });
            ui.checkbox(&mut self.cfg.auto_open, "启动时自动打开");
            ui.checkbox(&mut self.cfg.auto_reconnect, "故障自动重连（串口/网络，每 1 秒重试）");
            ui.weak("修改后需重新打开串口生效。");
            if ui.button("保存参数").clicked() {
                self.save_cfg();
            }
        });
        self.show_serial = open;

        let mut open = self.show_display;
        egui::Window::new("字体 / 颜色").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("字体:");
                let mut choice = self.cfg.font_name.clone();
                ComboBox::from_id_salt("font_choice").width(120.0)
                    .selected_text(&choice)
                    .show_ui(ui, |ui| {
                        for face in &self.fonts {
                            ui.selectable_value(&mut choice, face.name.clone(), &face.name);
                        }
                    });
                if choice != self.cfg.font_name {
                    self.cfg.font_name = choice.clone();
                    let used = fonts::install(&self.ctx, &self.fonts, &choice);
                    self.status(format!("字体: {used}"), false);
                    self.ctx.request_repaint();
                }
            });
            let mut changed = false;
            changed |= ui.add(egui::Slider::new(&mut self.cfg.font_size, 8.0..=36.0).text("字号")).changed();
            ui.horizontal(|ui| {
                ui.label("文字:");
                changed |= ui.color_edit_button_srgb(&mut self.cfg.fg).changed();
                ui.label("背景:");
                changed |= ui.color_edit_button_srgb(&mut self.cfg.bg).changed();
            });
            if changed {
                self.dirty();
            }
        });
        self.show_display = open;

        let mut open = self.show_about;
        egui::Window::new("关于CXCOM").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.heading("CXCOM");
            ui.label("Rust + egui + OpenGL 串口/网络调试器");
            ui.label("支持 GB2312 / UTF-8 收发切换");
            ui.label(format!("渲染: {}", self.gl));
            ui.label(format!("内存: 私有 {:.1} MB / 提交 {:.1} MB", self.stats.0, self.stats.1));
            ui.separator();
            ui.label("如果感兴趣，可以加入 QQ 群：947963288");
            if ui.link("点击加入群聊【Rust嵌入式探讨】").clicked() {
                win::open_url(QQ_GROUP_URL);
            }
            ui.separator();
            ui.weak("独立实现；参数保存在 CXCOM.ini。");
        });
        self.show_about = open;

        let mut open = self.show_help;
        egui::Window::new("使用说明").open(&mut open).resizable(true).show(ctx, |ui| {
            ui.label("1. 通讯端口：选择串口或 TCP/UDP 模式。");
            ui.label("2. 编码：GB2312 / UTF-8 / 自动；收和发使用同一编码。");
            ui.label("3. HEX 显示/发送、时间戳分包、校验、定时发送、文件发送。");
            ui.label("   接收窗口同时显示收发：-->收<-- 与 <--发-->，发送内容会回显。");
            ui.label("4. “工具 → 快捷指令”打开快捷指令面板；内置 ESP-01S/ESP8266 AT 预设（AT、WiFi、TCP/UDP、透传、Ping 等）。");
            ui.label("   字符串中的 \\r \\n 会作为回车换行发送；AT 预设默认已带 \\r\\n。");
            ui.label("5. 接收区每行可单独用鼠标拖选，Ctrl+C 复制选中文字；右键可复制全部。");
            ui.label("6. Ctrl+Enter 发送。");
            ui.label("7. 串口/TCP 故障会自动关闭并每秒重连，可在串口设置中关闭。");
            ui.label("8. 右上角胶囊按钮切换浅色/深色主题，选择会自动保存。");
            ui.label("9. “工具 → 绘图”打开串口绘图，按正则提取 name:value；默认规则兼容 Temp: 25.3, Hum: 60。");
            ui.separator();
            ui.label("感兴趣可以加入 QQ 群：947963288");
            if ui.link("点击加入群聊【Rust嵌入式探讨】").clicked() {
                win::open_url(QQ_GROUP_URL);
            }
        });
        self.show_help = open;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.viewport = ctx.input(|i| i.viewport().outer_rect);
        self.poll(ctx);

        let now = Instant::now();
        if self.cfg.timed && self.state == 2 && !self.send_text.is_empty() {
            let interval = Duration::from_millis(self.cfg.interval.max(10) as u64);
            if now.duration_since(self.last_timed) >= interval {
                self.send_current();
            }
            ctx.request_repaint_after(interval.saturating_sub(self.last_timed.elapsed()));
        }
        if self.loop_on && self.state == 2 && now >= self.loop_next {
            if let Some(i) = self.next_quick() {
                self.send_quick(i);
                self.loop_current = Some(i);
                self.loop_next = now + Duration::from_millis(self.cfg.quick[i].delay.max(1) as u64);
            } else {
                self.loop_on = false;
                self.status("没有可循环发送的字符串（顺序需大于 0）", true);
            }
        }
        if let Some(mut j) = self.file_job.take() {
            let sent = j.sent.load(Ordering::Relaxed);
            if sent > self.last_file_sent {
                self.tx_bytes += (sent - self.last_file_sent) as u64;
                self.last_file_sent = sent;
            }
            if j.done.load(Ordering::Relaxed) {
                self.status(format!("发送完毕: {} 字节", j.total), false);
            } else {
                ctx.request_repaint_after(Duration::from_millis(80));
                self.file_job = Some(j);
            }
        }
        if self.last_stats.elapsed() >= Duration::from_secs(1) {
            self.stats = win::memory_mb();
            self.last_stats = Instant::now();
        }



        self.menus(ctx);
        self.toolbar(ctx);
        egui::TopBottomPanel::bottom("status").exact_height(22.0).show(ctx, |ui| {
            // 状态栏只有一行：窄窗口时按优先级丢弃次要信息，避免左右两段互相挤压
            let w = ui.available_width();
            ui.horizontal(|ui| {
                ui.label(format!("S:{}  R:{}", self.tx_bytes, self.rx_bytes));
                ui.label("｜");
                ui.label(format!("显示缓冲 {} 字节", self.rx.total()));
                if w >= 460.0 {
                    ui.separator();
                    ui.label(format!("编码: {}", EncMode::from_u8(self.cfg.encoding).name()));
                }
                ui.separator();
                let color = if self.error { Color32::from_rgb(190, 30, 30) } else { Color32::from_rgb(0, 90, 20) };
                ui.add(egui::Label::new(RichText::new(&self.status).color(color)).truncate());
                if !self.follow_tail {
                    if ui.small_button("↓ 回到底部").clicked() {
                        self.follow_tail = true;
                        ctx.request_repaint();
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("MEM {:.1}MB", self.stats.0));
                    if w >= 560.0 {
                        ui.label(format!("RX {}/{}", self.rx.total(), limit_label(self.rx.limit())));
                    }
                });
            });
        });
        self.controls(ctx);
        self.quick_panel(ctx);
        self.plot_window(ctx);
        self.receive(ctx);
        self.dialogs(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_cfg();
    }
}
