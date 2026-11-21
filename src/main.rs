// Off-Windows builds only exist for type-checking/tests; Windows-only code looks unused there.
#![cfg_attr(not(windows), allow(dead_code))]
// Hide the console window in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod apps;
mod calc;
mod clipboard;
mod clipdb;
mod clipimg;
mod config;
mod engine;
mod everything;
mod icon;
mod icons;
mod learn;
mod log;
mod pathnav;
mod py;
mod recent;
mod matcher;
mod platform;
mod settings_db;
mod types;
mod ui;
#[cfg(windows)]
mod winffi;

use std::sync::Arc;

fn main() -> eframe::Result {
    log::install_panic_hook();
    let background = std::env::args().any(|a| a == "--background");
    if !platform::acquire_single_instance() {
        return Ok(());
    }

    let cfg = config::Config::load();
    let shared = platform::Shared::new(&cfg.hotkey, &cfg.clipboard_hotkey, !background);
    if let Ok(mut g) = shared.saved_pos.lock() {
        *g = cfg.window_pos;
    }

    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title(platform::WINDOW_TITLE)
        .with_inner_size([ui::WIDTH, 96.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_visible(!background)
        .with_icon(Arc::new(eframe::egui::IconData {
            rgba: icon::rgba(64),
            width: 64,
            height: 64,
        }));

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        depth_buffer: 0,
        multisampling: 0,
        ..Default::default()
    };

    let result = eframe::run_native(
        platform::WINDOW_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, cfg, shared, background)))),
    );
    if let Err(e) = &result {
        // Typically a missing/old OpenGL driver (remote desktop, VM). Make it visible.
        log::info(&format!("window creation failed: {e}"));
        platform::error_box(&format!(
            "SwiftFind 无法创建窗口:\n{e}\n\n详情见 {}",
            log::log_path().display()
        ));
    }
    result
}
