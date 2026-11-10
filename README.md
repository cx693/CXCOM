# CXCOM

> 版本说明：当前为 **v001**（第一个初始版本）。

Rust + **egui** + **OpenGL (glow)** 实现的串口 / 网络调试器，支持 GB2312 与 UTF-8 收发、串口绘图、AT 指令预设和浅色/深色主题。

[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.32-blue.svg)](https://github.com/emilk/egui)
[![OpenGL](https://img.shields.io/badge/OpenGL-glow-green.svg)](https://github.com/grovesNL/glow)
[![License: CC BY-NC-ND 4.0](https://img.shields.io/badge/License-CC%20BY--NC--ND%204.0-lightgrey.svg)](https://creativecommons.org/licenses/by-nc-nd/4.0/)

![CXCOM 深色主题与串口绘图](docs/plot.png)

## 功能

- **串口 / TCP Client / TCP Server / UDP** 四种通讯方式
- **GB2312 / UTF-8 / 自动识别** 收发，同一编码同时用于接收和发送
- 流式解码：中文被拆到多次串口读取时不会乱码
- 接收区增量追加、尾部跟随；上滚暂停跟随，可一键回到底部
- 收发同窗显示：`-->收<--` / `<--发-->`，均带毫秒时间戳
- 字符级拖选，`Ctrl+C` 复制选中文字，右键复制全部 / 清空
- 发送按钮随内容变色：输入框有内容时绿底，发送后恢复默认
- HEX 显示与 HEX 发送、CRC16 / ADD / XOR 校验
- 定时发送、文件发送、接收数据写文件
- 99 条快捷字符串，内置 ESP-01S / ESP8266 AT 指令预设
- **串口绘图**：正则提取 `name:value`，实时曲线、自动缩放
- 系统注册字体枚举（Windows 读取 HKLM/HKCU 字体注册表，默认宋体），浅色/深色主题一键切换
- 串口 / TCP 故障自动重连
- 保存数据按当前显示导出 UTF-8 `.log`

## 编码

| 模式 | 说明 |
|---|---|
| GB2312 | GBK / CP936 超集，兼容常见中文串口设备 |
| UTF-8 | 标准 UTF-8，支持跨读取拆分字符 |
| 自动 | 优先按合法 UTF-8 解码，失败段回退 GBK；可混合显示 |

收发使用同一模式。接收解码器保留不完整字节，等下一包数据到达后继续拼接，不会把中文拆坏。

## 串口绘图

菜单 **工具 → 绘图** 打开绘图窗口。默认规则：

```regex
(?P<label>[^\s:=,]+)\s*[:=]\s*(?P<value>-?\d+(?:\.\d+)?)
```

兼容：

```text
Temp: 25.3, Hum: 60
温度T: 18.6 C, 湿度H: 54 %
DHT11 - 温度T: 22.4 C, 湿度H: 67 %
```

- 每行可匹配多个通道。
- 自定义规则可使用命名组 `label` / `value`；没有命名组时，第 1 组作通道名，最后一个数字捕获作值。
- 鼠标悬停显示十字竖线、各通道最近采样点和跟随数值框；数值字号可在绘图窗口用滑条 **12–24** 调节，默认 **18**。
- 默认显示最近 15 秒（-15~0 s），鼠标滚轮在绘图区可缩放时间刻度，范围 **1.5–15 秒**，跨度不会超过 15 秒；Y 轴自动缩放，最多 24 通道、每通道 3000 点。
- 数据只保留最近 35 秒；通道长时间无数据会自动回收，容量也会收缩。
- **关闭串口后曲线冻结保留**（时间轴停在关闭时刻，曲线不再向左滑走），方便事后分析；重新连接或收到新数据后自动恢复滚动。
- **关闭绘图窗口立即释放全部曲线缓存**，窗口未打开时不解析绘图数据。
- 规则错误会显示在窗口内，“清空”只清绘图数据。

## ESP-01S AT 预设

“工具 → 快捷指令”面板里点 **AT预设** 可载入：

- 基础：`AT`、`AT+RST`、`AT+GMR`、`AT+RESTORE`、`AT+UART_DEF`
- WiFi：`AT+CWMODE`、`AT+CWLAP`、`AT+CWJAP`、`AT+CWQAP`、`AT+CIFSR`
- TCP/UDP：`AT+CIPMUX`、`AT+CIPSTART`、`AT+CIPSEND`、`AT+CIPCLOSE`
- 透传：`AT+CIPMODE=1`、`AT+CIPSEND`、`+++`
- 其他：`AT+PING`、`AT+SLEEP=0`

字符串中的 `\r\n` 会转换成真正的 CRLF；`+++` 不带换行。

## 自动重连

串口写入/读取失败（例如 `os error 121`）或 TCP 断线时：

1. 关闭故障连接；
2. 状态栏提示“自动修复中”；
3. 每秒重试；
4. 恢复后自动继续接收和发送。

可在“串口设置”中关闭。等待期间收到的待发送数据会暂存，重连后补发。

## 主题

右上角胶囊按钮切换 **浅色 / 深色**，选择自动保存到 `CXCOM.ini` 的 `dark=0/1`。深色模式会同步调整菜单、控件、接收区和绘图背景。

## 构建

需要 Rust stable。Windows / Linux / macOS 使用同一套源码。

```bash
cargo build --release
```

产物：

```text
target/release/CXCOM.exe      # Windows
target/release/CXCOM          # Linux / macOS
```

Windows exe 图标使用 `icon/app.res` 预生成资源，**构建不需要 Windows SDK / rc.exe**。Linux 需要常见 winit/OpenGL 开发包，例如 Debian/Ubuntu：

```bash
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```

### macOS 打包（.app）

macOS 上 `cargo build --release` 得到的 `target/release/CXCOM` 是纯 Unix 可执行文件，Finder / Dock **只能显示系统固定的 `exec` 图标**，这是 macOS 的规则，不是打包失败。要让图标生效、像正常 App 一样启动，必须使用 `.app`。

```bash
cargo install cargo-bundle --locked   # 只需安装一次（务必加 --locked）
./build_app.sh                 # 或 ./build_app.sh --dmg
```

产物：

```text
target/release/CXCOM.app     # 与二进制同目录，直接双击（推荐）
dist/CXCOM.app               # 分发副本
dist/CXCOM-0.1.0.dmg         # 加 --dmg 时生成，内含“应用程序”快捷方式
                             # （hdiutil 受限时自动改生成同名 .zip）
```

> `cargo` 的 target 目录可能被 `CARGO_TARGET_DIR` 或 `.cargo/config.toml` 改到项目外；脚本会通过 `cargo metadata` 自动识别，不写死 `target/`。

脚本做的事：

1. 用 `icon/tu.png` 作为图标 —— cargo-bundle 自动转 `.icns` 并写入 `Info.plist`，**不需要手工准备 icns 或用在线转换工具**；
2. `cargo bundle --release --format osx` 生成 `.app`；
3. 自动 ad-hoc 签名（`codesign --sign -`），Apple Silicon 上可直接启动；
4. 把 `.app` 复制到原始二进制旁边（`target/release/CXCOM.app`）和 `dist/`，避免对着没有图标的二进制干瞪眼；
5. 可选生成 DMG，并打印版本、Bundle ID、图标、架构、大小。

常用参数：

| 参数 | 说明 |
|---|---|
| `--universal` | 同时构建 arm64 + x86_64 并合并为通用二进制（需 `rustup target add aarch64-apple-darwin x86_64-apple-darwin`） |
| `--dmg` | 额外生成 `dist/CXCOM-<版本>.dmg`，可直接分发给别人拖进“应用程序”（`hdiutil` 不可用时自动退回 zip） |
| `--no-sign` | 跳过签名 |
| `--clean` | 打包前先 `cargo clean` |
| `--open` | 打包完成后直接打开 App |
| `--reveal` | 在访达中定位生成的 `.app` |

打包成 `.app` 后，配置改存到 `~/Library/Application Support/CXCOM/CXCOM.ini`，不再写进应用包内部（避免破坏签名）；命令行直接运行时仍使用可执行文件同目录的 `CXCOM.ini`。

> 未做 Apple 公证（notarize）：第一次打开若提示“无法验证开发者”，在访达里右键 →「打开」；从网络下载的副本可执行 `xattr -dr com.apple.quarantine CXCOM.app`。图标缓存没刷新时执行 `killall Dock`。

### 跨平台说明

| 平台 | 字体 | GB2312 | 文件对话框 |
|---|---|---|---|
| Windows | `HKLM`/`HKCU` 字体注册表 + `C:\Windows\Fonts` + 用户字体目录 | Win32 `MultiByteToWideChar(936)` | 原生对话框 |
| macOS | `/System/Library/Fonts`、`/Library/Fonts`、`~/Library/Fonts`，以及字体册按需下载的 `/System/Library/AssetsV2/com_apple_MobileAsset_Font*` | `encoding_rs` GBK | 保存到当前目录 |
| Linux | `/usr/share/fonts`、`~/.local/share/fonts`、`~/.fonts` | `encoding_rs` GBK | 保存到当前目录 |

启动时扫描系统字体，通过 `cmap` 检测 `U+4E2D`，只列出能显示中文的字体；Windows 默认宋体，macOS 优先“苹方-简”（缺失时回退宋体-简 / 黑体-简），Linux 优先 Noto Sans CJK / 文泉驿。Windows 会读取 `HKLM/HKCU\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts`，再合并字体目录，逐个 face 枚举 `.ttc` 集合，并从 `name` 表取本地化家族名（如“宋体”“微软雅黑”）。macOS 会扫描 Font Book 按需下载的 `/System/Library/AssetsV2/com_apple_MobileAsset_Font*`，逐个 face 枚举 `.ttc` 集合，字体名取 `name` 表的中文家族名，与“字体册”显示一致。

## 使用

1. 选择串口或 TCP/UDP，设置参数后点击“打开连接”。
2. 选择编码（GB2312 / UTF-8 / 自动）。
3. 接收区显示数据；需要看曲线时点“绘图”。
4. 输入内容后点“发送”，`Ctrl+Enter` 也可发送。
5. 需要批量指令时打开“工具 → 快捷指令”，可载入 AT 预设或自行编辑。
6. “保存数据”按当前屏幕显示导出 `.log`（时间戳、HEX 格式与界面一致）。

参数保存在 exe 同目录的 `CXCOM.ini`；macOS 打包成 `.app` 后改存 `~/Library/Application Support/CXCOM/CXCOM.ini`。

## 体积与内存

Windows release 实测：

| 项目 | 数值 |
|---|---|
| exe | **约 3.19 MB**（含 regex 引擎与图标资源） |
| 私有工作集 | **约 45 MB** |
| 接收缓冲 | 200 KB – 20 MB 可调 |
| 渲染上限 | 只把末尾 256 KB 转成显示行 |

OpenGL 驱动/上下文占进程工作集的一大部分，属于渲染后端成本。

### macOS 内存占用明显偏高，需要重点优化

> **macOS 平台需要重点优化。** 同一份代码 Windows 约 45 MB，macOS 状态栏 `MEM`
> 常见 90–120 MB（前台），被遮挡后会回落到 35–44 MB。这不是泄漏：CXCOM 自己的堆
> 只有约 11 MB，大头来自 macOS 的 OpenGL 兼容层、IOKit/IOSurface 与窗口服务器。

状态栏 `MEM` 在两个平台的口径不同，不能直接横比：

| 项目 | macOS `phys_footprint` | Windows 私有工作集 |
|---|---|---|
| 私有脏页 | 计入 | 计入 |
| 压缩内存 | 计入 | 不计入 |
| GPU / OpenGL 表面与驱动内存 | 计入（IOKit / IOSurface） | 不计入（在 WDDM 内核态） |
| 共享框架页 | 基本不计入 | 不计入 |
| CXCOM 典型读数 | 90–120 MB（前台）/ 35–44 MB（遮挡后） | 约 45 MB |

macOS 取 `proc_pid_rusage` 的 `phys_footprint`（与“活动监视器 → 内存”一致），
Windows 取 `GetProcessMemoryInfo` 的私有工作集。

本机实测（macOS 15 / arm64，debug 构建）：

- 空 eframe + glow 窗口（不跑任何应用代码）：启动 footprint 峰值约 **96 MB**，
  静止后被遮挡可降到 **35–44 MB**；
- CXCOM 三次采样：**12.7 → 44.3 → 96.9 MB**，随窗口被合成到前台而上升；
- CXCOM 自身 malloc 堆约 **11 MB**（`vmmap`），其余是 GL 上下文、Cocoa/AppKit
  框架和窗口服务器表面。

可用“活动监视器”的“内存”列或 `vmmap <pid>` 复核上述数字。

优化方向（按性价比排序）：

1. **换掉 glow（OpenGL）后端**：macOS 已弃用 OpenGL，Apple 的 GL 实现在 Metal
   之上做兼容层，驱动与表面开销很大。改用 `wgpu`（Metal）后端是首要尝试方向，
   收益需实测确认。
2. **减少无谓重绘**：连接存在时 `poll()` 在每帧结尾调用 `request_repaint()`，
   空闲时也按满帧率重绘，窗口表面会持续被映射。建议改为由 I/O 线程在收到数据
   时调用 `ctx.request_repaint()` 唤醒 UI，空闲时用 `request_repaint_after`
   拉长间隔。
3. **不要为了刷新 `MEM` 做定时重绘**：1 Hz 定时重绘会让窗口表面一直保持映射，
   实测 footprint 停在 ~90 MB，反而更耗内存，不建议采用。
4. **收敛字体图集**：CJK 按需光栅化，避免多字号并存；切换字号/字体时复用图集。
5. **控制缓冲上限**：接收缓冲默认 200 KB（可调到 20 MB），绘图数据另算，必要时
   改用固定长度的环形缓冲。

Windows 的 `MEM` 仍按私有工作集显示，不受影响；Linux 暂未实现，显示 0.0 MB。

## 目录

```text
CXCOM/
├─ src/
│  ├─ app.rs        界面、绘图、主题与交互
│  ├─ text.rs       GB2312 / UTF-8 / 自动 流式编解码
│  ├─ buffer.rs     有界接收缓冲与行渲染
│  ├─ transport.rs  串口 / TCP / UDP 工作线程与重连
│  ├─ checksum.rs   HEX 解析与 CRC/ADD/XOR
│  ├─ config.rs     CXCOM.ini 与 AT 预设
│  ├─ fonts.rs      系统字体枚举与加载
│  └─ win.rs        文件对话框、GBK、内存统计（Windows / macOS）
├─ assets/          字体兜底
├─ icon/            tu.png / tu.ico / app.rc / app.res
├─ docs/            截图
├─ tools/           字体生成脚本
├─ build_app.sh     macOS .app 打包脚本（cargo-bundle）
├─ build.rs         链接预生成图标资源
└─ Cargo.toml
```

## 已知边界

- 未实现原工具的 ISP 烧写（STC / STM32）。
- `serialport` crate 不支持 1.5 停止位与 Mark/Space 校验，当前提供 1/2 停止位与 None/Odd/Even。
- macOS / Linux 未内置原生文件对话框，保存文件会写入当前目录；可在 Windows 使用完整对话框体验。
- macOS 的 `MEM` 为 `phys_footprint` 口径（含 GPU 与窗口服务器），明显高于 Windows 私有工作集；**macOS 平台需要重点优化**，详见“体积与内存”。
- macOS `.app` 仅做 ad-hoc 签名，未做 Apple 公证；分发给其他人时仍可能被 Gatekeeper 拦截。

## License

[![CC BY-NC-ND 4.0](https://img.shields.io/badge/License-CC%20BY--NC--ND%204.0-lightgrey.svg)](https://creativecommons.org/licenses/by-nc-nd/4.0/)

本项目采用 **Creative Commons Attribution-NonCommercial-NoDerivatives 4.0 International（CC BY-NC-ND 4.0）**：

- **署名**：分享时需注明来源 CXCOM 并附许可证链接。
- **非商业**：不得用于商业目的。
- **禁止演绎**：不得分享修改后的衍生版本。

完整法律文本见 [LICENSE](LICENSE)，或访问
<https://creativecommons.org/licenses/by-nc-nd/4.0/>。

如需商业授权或定制，欢迎加入 QQ 群 **947963288**（Rust 嵌入式探讨）。
