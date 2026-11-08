//! The `zrush-gui` binary.

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "zrush",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(Empty))),
    )
}

struct Empty;

impl eframe::App for Empty {
    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}
}
