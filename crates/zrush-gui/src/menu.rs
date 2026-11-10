//! What the app offers outside any tab: in the macOS menu bar there, in
//! the window's own bar elsewhere, and in the command palette everywhere.

/// Something asked of the whole app rather than of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    OpenRepository,
    Settings,
    Palette,
}

#[cfg(target_os = "macos")]
pub use mac::Menus;

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::mpsc::{Receiver, channel};

    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

    use super::Command;

    /// The menu bar, kept alive for as long as the app runs, and what its
    /// items were clicked for.
    pub struct Menus {
        _menu: Menu,
        ids: Vec<(MenuId, Command)>,
        rx: Receiver<MenuId>,
    }

    impl Menus {
        /// Put zrush's menus in the menu bar. A click wakes the window,
        /// which reads it at its next frame.
        pub fn install(ctx: &egui::Context) -> Option<Self> {
            let cmd = |text: &str, key: Code, command: Command| {
                let item = MenuItem::new(text, true, Some(Accelerator::new(Modifiers::META, key)));
                let id = item.id().clone();
                (item, (id, command))
            };
            let (settings, s) = cmd("Settings…", Code::Comma, Command::Settings);
            let (open, o) = cmd("Open Repository…", Code::KeyO, Command::OpenRepository);
            let (palette, p) = cmd("Command Palette…", Code::KeyK, Command::Palette);
            let sep = PredefinedMenuItem::separator;

            let app = Submenu::new("zrush", true);
            app.append_items(&[
                &PredefinedMenuItem::about(None, None),
                &sep(),
                &settings,
                &sep(),
                &PredefinedMenuItem::services(None),
                &sep(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &sep(),
                &PredefinedMenuItem::quit(None),
            ])
            .ok()?;
            let file = Submenu::new("File", true);
            file.append_items(&[&open, &sep(), &palette]).ok()?;
            // No Edit menu: its ⌘C and ⌘V would be taken by the menu and
            // never reach the text fields, which egui handles itself.
            let window = Submenu::new("Window", true);
            window
                .append_items(&[
                    &PredefinedMenuItem::minimize(None),
                    &PredefinedMenuItem::maximize(None),
                    &sep(),
                    &PredefinedMenuItem::fullscreen(None),
                    &sep(),
                    &PredefinedMenuItem::bring_all_to_front(None),
                ])
                .ok()?;
            let menu = Menu::new();
            menu.append_items(&[&app, &file, &window]).ok()?;
            menu.init_for_nsapp();
            window.set_as_windows_menu_for_nsapp();

            let (tx, rx) = channel();
            let ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let _ = tx.send(e.id);
                ctx.request_repaint();
            }));
            Some(Self {
                _menu: menu,
                ids: vec![s, o, p],
                rx,
            })
        }

        /// What was clicked since the last frame.
        pub fn commands(&self) -> Vec<Command> {
            self.rx
                .try_iter()
                .filter_map(|id| {
                    self.ids
                        .iter()
                        .find(|(known, _)| *known == id)
                        .map(|(_, c)| *c)
                })
                .collect()
        }
    }
}
