//! The `zrush-gui` binary: the window, over every repository it had open.

// On Windows, a window of its own and no console behind it. Debug builds
// keep the console, where a panic says what happened.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use zrush_core::config::{Config, Dirs};
use zrush_core::host::process::ProcessHost;
use zrush_gui::app::GuiApp;

fn main() -> eframe::Result<()> {
    zrush_gui::login_path::adopt();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("zrush")
        .with_app_id("zrush")
        .with_inner_size([1200.0, 800.0]);
    // The content runs under a see-through title bar: the top bar is the
    // title bar, its window buttons over the room it leaves for them.
    if cfg!(target_os = "macos") {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    }
    // A broken icon is no reason not to open.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(zrush_gui::chrome::ICON) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "zrush",
        options,
        Box::new(|cc| {
            zrush_gui::fonts::install(&cc.egui_ctx);
            #[cfg(target_os = "macos")]
            let menus = zrush_gui::menu::Menus::install(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let wake = Arc::new(move || ctx.request_repaint());
            Ok(match load() {
                Ok((dirs, cfg)) => {
                    let named = std::env::args_os().skip(1).map(PathBuf::from).collect();
                    let repos = zrush_gui::repos::initial(&cfg, named, std::env::current_dir);
                    #[allow(unused_mut)]
                    let mut app = GuiApp::new(
                        dirs,
                        cfg,
                        repos,
                        Box::new(|cfg| Box::new(ProcessHost::new(cfg))),
                        wake,
                    )
                    .with_titlebar_tabs();
                    #[cfg(target_os = "macos")]
                    if let Some(m) = menus {
                        app.set_menus(m);
                    }
                    Box::new(app)
                }
                Err(e) => Box::new(Broken(e)),
            })
        }),
    )
}

fn load() -> Result<(Dirs, Config), String> {
    let dirs = Dirs::from_env().map_err(|e| e.to_string())?;
    let cfg = Config::load(&dirs).map_err(|e| e.to_string())?;
    Ok((dirs, cfg))
}

/// The configuration could not be read: say so in the window rather than
/// vanishing with a message nobody sees.
struct Broken(String);

impl eframe::App for Broken {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("zrush could not start");
            ui.label(&self.0);
        });
    }
}
