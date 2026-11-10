//! 工具菜单里的三个小窗口：校验和计算器、进制转换器、ASCII 码表。
//!
//! 这些窗口只依赖本地输入，不碰串口状态；所有状态都放在 [`Tools`] 里，
//! 与 [`crate::app::App`] 的通讯逻辑解耦，避免把调试工具混进收发路径。

use eframe::egui::{self, Color32, ComboBox, RichText, ScrollArea, TextEdit, TextStyle};

use crate::checksum::{self, hex_line, ChecksumKind};

#[derive(Default)]
pub struct Tools {
    pub show_checksum: bool,
    pub show_base: bool,
    pub show_ascii: bool,
    // 校验和计算器
    cs_input: String,
    cs_hex: bool,
    cs_kind: u8,
    // 进制转换器
    base_input: String,
    base_mode: u8,
    // ASCII 码表
    ascii_filter: String,
}

impl Tools {
    /// 默认算法选 Modbus CRC16（串口调试里最常用）。
    pub fn new() -> Self {
        Self {
            cs_kind: 1,
            ..Default::default()
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        self.checksum_window(ctx);
        self.base_window(ctx);
        self.ascii_window(ctx);
    }

    fn checksum_data(&self) -> Result<Vec<u8>, String> {
        if self.cs_hex {
            checksum::parse_hex(&self.cs_input)
        } else {
            Ok(self.cs_input.as_bytes().to_vec())
        }
    }

    // -------------------------------------------------------- 校验和计算器

    fn checksum_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_checksum;
        egui::Window::new("校验和计算器")
            .open(&mut open)
            .default_width(560.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.cs_hex, "HEX 输入");
                    ui.label("算法:");
                    ComboBox::from_id_salt("cs_kind")
                        .width(150.0)
                        .selected_text(ChecksumKind::from_u8(self.cs_kind).name())
                        .show_ui(ui, |ui| {
                            for (v, n) in [(2u8, "ADD 累加和"), (3, "XOR 异或和"), (1, "Modbus CRC16")] {
                                ui.selectable_value(&mut self.cs_kind, v, n);
                            }
                        });
                    if ui.button("清空").clicked() {
                        self.cs_input.clear();
                    }
                });
                ui.add(
                    TextEdit::multiline(&mut self.cs_input)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY)
                        .font(TextStyle::Monospace)
                        .hint_text(if self.cs_hex {
                            "例如: 01 03 00 00 00 0A"
                        } else {
                            "输入要计算校验和的文本（按 UTF-8 字节）"
                        }),
                );
                ui.separator();
                let kind = ChecksumKind::from_u8(self.cs_kind);
                match self.checksum_data() {
                    Err(e) => {
                        ui.colored_label(Color32::from_rgb(190, 40, 40), e);
                    }
                    Ok(data) if data.is_empty() => {
                        ui.weak("输入数据后自动计算");
                    }
                    Ok(data) => {
                        let sum = checksum::value(&data, kind);
                        let mut frame = data.clone();
                        frame.extend_from_slice(&sum);
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(format!("数据 {} 字节", data.len()));
                            if ui.small_button("复制数据").clicked() {
                                ui.ctx().copy_text(hex_line(&data));
                            }
                            if ui.small_button("复制校验值").clicked() {
                                ui.ctx().copy_text(hex_line(&sum));
                            }
                            if ui.small_button("复制完整帧").clicked() {
                                ui.ctx().copy_text(hex_line(&frame));
                            }
                        });
                        ui.monospace(format!("数据: {}", hex_line(&data)));
                        let value_text = if sum.len() == 2 {
                            let v = u16::from_le_bytes([sum[0], sum[1]]);
                            format!("{}  (0x{v:04X} = {v})，低字节在前", hex_line(&sum))
                        } else {
                            let v = sum.first().copied().unwrap_or(0);
                            format!("{}  ({v})", hex_line(&sum))
                        };
                        ui.monospace(format!("校验值: {value_text}"));
                        ui.monospace(format!("完整帧: {}", hex_line(&frame)));
                        if kind == ChecksumKind::ModbusCrc16 {
                            ui.weak("Modbus CRC16 追加顺序：低字节在前、高字节在后。");
                        }
                    }
                }
            });
        self.show_checksum = open;
    }

    // ---------------------------------------------------------- 进制转换器

    fn base_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_base;
        egui::Window::new("进制转换器")
            .open(&mut open)
            .default_width(460.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("输入:");
                    ui.add(
                        TextEdit::singleline(&mut self.base_input)
                            .desired_width(f32::INFINITY)
                            .font(TextStyle::Monospace)
                            .hint_text("123 / 0xFF / 0b1010 / 0o777"),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("进制:");
                    ComboBox::from_id_salt("base_mode")
                        .width(100.0)
                        .selected_text(
                            ["自动", "二进制", "八进制", "十进制", "十六进制"]
                                [(self.base_mode as usize).min(4)],
                        )
                        .show_ui(ui, |ui| {
                            for (v, n) in [
                                (0u8, "自动"),
                                (1, "二进制"),
                                (2, "八进制"),
                                (3, "十进制"),
                                (4, "十六进制"),
                            ] {
                                ui.selectable_value(&mut self.base_mode, v, n);
                            }
                        });
                    ui.weak("自动：识别 0x / 0b / 0o 前缀，否则按十进制");
                });
                ui.separator();
                if self.base_input.trim().is_empty() {
                    ui.weak("输入数字后自动转换");
                } else {
                    match parse_number(&self.base_input, self.base_mode) {
                        Err(e) => {
                            ui.colored_label(Color32::from_rgb(190, 40, 40), e);
                        }
                        Ok((value, negative)) => base_output(ui, value, negative),
                    }
                }
            });
        self.show_base = open;
    }

    // ------------------------------------------------------------- ASCII 码表

    fn ascii_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_ascii;
        egui::Window::new("ASCII 码表")
            .open(&mut open)
            .default_size([560.0, 460.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("过滤:");
                    ui.add(
                        TextEdit::singleline(&mut self.ascii_filter)
                            .desired_width(220.0)
                            .hint_text("字符 / 65 / 0x41 / 0b1000001"),
                    );
                    if ui.button("清空").clicked() {
                        self.ascii_filter.clear();
                    }
                    ui.weak("范围 0–127");
                });
                ui.separator();
                ui.horizontal(|ui| {
                    for (w, s) in [(44.0, "DEC"), (44.0, "HEX"), (92.0, "BIN"), (44.0, "字符")] {
                        ui.add_sized([w, 18.0], egui::Label::new(RichText::new(s).weak()));
                    }
                    ui.add_sized([44.0, 18.0], egui::Label::new(RichText::new("复制").weak()));
                    ui.weak("说明");
                });
                let filter = self.ascii_filter.trim().to_ascii_lowercase();
                let rows: Vec<u8> = (0u8..=127).filter(|&c| ascii_matches(c, &filter)).collect();
                if rows.is_empty() {
                    ui.weak("没有匹配的字符");
                }
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, 20.0, rows.len(), |ui, range| {
                        for &code in &rows[range] {
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [44.0, 18.0],
                                    egui::Label::new(
                                        RichText::new(code.to_string()).monospace(),
                                    ),
                                );
                                ui.add_sized(
                                    [44.0, 18.0],
                                    egui::Label::new(
                                        RichText::new(format!("{code:02X}")).monospace(),
                                    ),
                                );
                                ui.add_sized(
                                    [92.0, 18.0],
                                    egui::Label::new(
                                        RichText::new(format!("{code:08b}")).monospace(),
                                    ),
                                );
                                ui.add_sized(
                                    [44.0, 18.0],
                                    egui::Label::new(RichText::new(ascii_char(code)).monospace()),
                                );
                                if ui.add_sized([44.0, 18.0], egui::Button::new("复制")).clicked() {
                                    ui.ctx().copy_text(ascii_copy(code));
                                }
                                ui.weak(ascii_name(code));
                            });
                        }
                    });
            });
        self.show_ascii = open;
    }
}

// ---------------------------------------------------------------- 进制转换

/// 解析一个整数：`mode` 为 0 时自动识别前缀，否则按 1=2 进制 / 2=8 进制 /
/// 3=10 进制 / 4=16 进制解释。返回 (绝对值, 是否为负数)。
fn parse_number(input: &str, mode: u8) -> Result<(u128, bool), String> {
    let t = input.trim().replace('_', "");
    if t.is_empty() {
        return Err("请输入数字".into());
    }
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, t.strip_prefix('+').unwrap_or(&t).trim()),
    };
    if body.is_empty() {
        return Err("请输入数字".into());
    }
    let lower = body.to_ascii_lowercase();
    let (radix, digits): (u32, &str) = match mode {
        1 => (2, lower.strip_prefix("0b").unwrap_or(&lower)),
        2 => (8, lower.strip_prefix("0o").unwrap_or(&lower)),
        4 => (16, lower.strip_prefix("0x").unwrap_or(&lower)),
        3 => (10, lower.as_str()),
        _ => {
            if lower.starts_with("0x") || lower.starts_with("0b") || lower.starts_with("0o") {
                let radix = match &lower[0..2] {
                    "0x" => 16u32,
                    "0b" => 2,
                    _ => 8,
                };
                (radix, &lower[2..])
            } else {
                (10, lower.as_str())
            }
        }
    };
    let digits = digits.replace(' ', "");
    if digits.is_empty() {
        return Err("请输入数字".into());
    }
    let mut value: u128 = 0;
    for c in digits.chars() {
        let Some(d) = c.to_digit(radix) else {
            return Err(format!("字符 '{c}' 不是合法的 {radix} 进制数字"));
        };
        value = value
            .checked_mul(radix as u128)
            .and_then(|v| v.checked_add(d as u128))
            .ok_or("数值超出 128 位范围")?;
    }
    if negative && value == 0 {
        return Ok((0, false));
    }
    Ok((value, negative))
}

fn base_output(ui: &mut egui::Ui, value: u128, negative: bool) {
    let sign = if negative { "-" } else { "" };
    let mut rows: Vec<(&str, String)> = vec![
        ("十进制", format!("{sign}{value}")),
        ("十六进制", format!("{sign}0x{value:X}")),
        ("八进制", format!("{sign}0o{value:o}")),
        ("二进制", format!("{sign}0b{}", group_binary(value))),
    ];
    let bytes = be_bytes(value);
    let be: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    let mut le = bytes.clone();
    le.reverse();
    let le_s: Vec<String> = le.iter().map(|b| format!("{b:02X}")).collect();
    rows.push(("字节数", format!("{} 字节", bytes.len())));
    rows.push(("大端字节", be.join(" ")));
    rows.push(("小端字节", le_s.join(" ")));
    for (name, text) in rows {
        ui.horizontal(|ui| {
            ui.add_sized(
                [80.0, 18.0],
                egui::Label::new(RichText::new(name).weak()),
            );
            ui.monospace(text.clone());
            if ui.small_button("复制").clicked() {
                ui.ctx().copy_text(text);
            }
        });
    }
    let bits = if value == 0 {
        1
    } else {
        128 - value.leading_zeros() as usize
    };
    ui.weak(format!("有效位宽 {bits} 位（最大支持 128 位）"));
}

/// 从右往左每 4 位插入下划线，最高位组去掉多余前导零。
fn group_binary(v: u128) -> String {
    if v == 0 {
        return "0".into();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut n = v;
    while n > 0 {
        parts.push(format!("{:04b}", n & 0xF));
        n >>= 4;
    }
    parts.reverse();
    if let Some(first) = parts.first_mut() {
        let trimmed = first.trim_start_matches('0').to_string();
        *first = if trimmed.is_empty() { "0".to_string() } else { trimmed };
    }
    parts.join("_")
}

fn be_bytes(v: u128) -> Vec<u8> {
    if v == 0 {
        return vec![0];
    }
    let mut n = v;
    let mut out = Vec::new();
    while n > 0 {
        out.push((n & 0xFF) as u8);
        n >>= 8;
    }
    out.reverse();
    out
}

// ---------------------------------------------------------------- ASCII 码表

fn ascii_name(code: u8) -> &'static str {
    match code {
        0 => "NUL 空字符",
        1 => "SOH 标题开始",
        2 => "STX 正文开始",
        3 => "ETX 正文结束",
        4 => "EOT 传输结束",
        5 => "ENQ 询问",
        6 => "ACK 确认",
        7 => "BEL 响铃",
        8 => "BS 退格",
        9 => "HT 水平制表",
        10 => "LF 换行",
        11 => "VT 垂直制表",
        12 => "FF 换页",
        13 => "CR 回车",
        14 => "SO 移出",
        15 => "SI 移入",
        16 => "DLE 数据链路转义",
        17 => "DC1 设备控制 1",
        18 => "DC2 设备控制 2",
        19 => "DC3 设备控制 3",
        20 => "DC4 设备控制 4",
        21 => "NAK 否定",
        22 => "SYN 同步空闲",
        23 => "ETB 块传输结束",
        24 => "CAN 取消",
        25 => "EM 介质结束",
        26 => "SUB 替换",
        27 => "ESC 转义",
        28 => "FS 文件分隔",
        29 => "GS 组分隔",
        30 => "RS 记录分隔",
        31 => "US 单元分隔",
        32 => "SPACE 空格",
        127 => "DEL 删除",
        _ => "",
    }
}

fn ascii_char(code: u8) -> String {
    match code {
        32..=126 => (code as char).to_string(),
        0..=31 => format!("^{}", (b'@' + code) as char),
        127 => "^?".into(),
        _ => "·".into(),
    }
}

fn ascii_copy(code: u8) -> String {
    match code {
        32..=126 => (code as char).to_string(),
        _ => ascii_char(code),
    }
}

fn ascii_matches(code: u8, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let dec = code.to_string();
    let hex = format!("{code:02x}");
    let bin = format!("{code:08b}");
    let ch = ascii_char(code).to_ascii_lowercase();
    let name = ascii_name(code).to_ascii_lowercase();
    filter.split_whitespace().all(|tok| {
        let t = tok
            .trim_start_matches("0x")
            .trim_start_matches("0b");
        if t.is_empty() {
            return true;
        }
        dec.contains(t) || hex.contains(t) || bin.contains(t) || ch.contains(t) || name.contains(t)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_number_with_prefixes() {
        assert_eq!(parse_number("0xFF", 0), Ok((255, false)));
        assert_eq!(parse_number("0b1010", 0), Ok((10, false)));
        assert_eq!(parse_number("0o777", 0), Ok((511, false)));
        assert_eq!(parse_number("123", 0), Ok((123, false)));
        assert_eq!(parse_number("-42", 0), Ok((42, true)));
        assert_eq!(parse_number("FF", 4), Ok((255, false)));
        assert_eq!(parse_number("0x1_000", 0), Ok((4096, false)));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_number("", 0).is_err());
        assert!(parse_number("zz", 4).is_err());
        assert!(parse_number("2", 1).is_err());
        assert!(parse_number("-", 0).is_err());
    }

    #[test]
    fn binary_grouping() {
        assert_eq!(group_binary(0), "0");
        assert_eq!(group_binary(1), "1");
        assert_eq!(group_binary(0xFF), "1111_1111");
        assert_eq!(group_binary(0x1234), "1_0010_0011_0100");
    }

    #[test]
    fn checksum_value_matches_apply() {
        let data = [1u8, 3, 0, 0, 0, 10];
        for kind in [
            ChecksumKind::ModbusCrc16,
            ChecksumKind::Add,
            ChecksumKind::Xor,
        ] {
            let sum = checksum::value(&data, kind);
            let framed = checksum::apply(&data, kind, 1, 0);
            assert_eq!(&framed[data.len()..], &sum[..]);
        }
    }

    #[test]
    fn ascii_filter_matches() {
        assert!(ascii_matches(65, ""));
        assert!(ascii_matches(65, "a"));
        assert!(ascii_matches(65, "65"));
        assert!(ascii_matches(65, "0x41"));
        assert!(ascii_matches(65, "0b1000001"));
        assert!(ascii_matches(10, "lf"));
        assert!(!ascii_matches(66, "65"));
    }
}
