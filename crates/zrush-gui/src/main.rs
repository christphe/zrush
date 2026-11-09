//! The `zrush-gui` binary: the window, over every repository it had open.

use std::path::PathBuf;
use std::sync::Arc;

use zrush_core::config::{Config, Dirs};
use zrush_core::host::process::ProcessHost;
use zrush_gui::app::GuiApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("zrush")
            .with_app_id("zrush")
            .with_inner_size([1200.0, 800.0]),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "zrush",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let wake = Arc::new(move || ctx.request_repaint());
            Ok(match load() {
                Ok((dirs, cfg)) => {
                    let named = std::env::args_os().skip(1).map(PathBuf::from).collect();
                    let repos = zrush_gui::repos::initial(&cfg, named, std::env::current_dir);
                    Box::new(GuiApp::new(
                        dirs,
                        cfg,
                        repos,
                        Box::new(|cfg| Box::new(ProcessHost::new(cfg))),
                        wake,
                    ))
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
