#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod buffer;
mod checksum;
mod config;
mod fonts;
mod text;
mod transport;
mod win;

fn load_icon() -> Option<eframe::egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../icon/tu_256.png")).ok()?.to_rgba8();
    let (width, height) = (img.width(), img.height());
    Some(eframe::egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

fn main() -> eframe::Result<()> {
    let cfg = config::Config::load();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("CXCOM 串口/网络调试器")
        .with_inner_size([cfg.win_w.max(820) as f32, cfg.win_h.max(560) as f32])
        .with_min_inner_size([820.0, 560.0])
        .with_position(eframe::egui::pos2(cfg.win_x as f32, cfg.win_y as f32));
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        vsync: true,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        ..Default::default()
    };
    eframe::run_native(
        "CXCOM",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg)))),
    )
}
