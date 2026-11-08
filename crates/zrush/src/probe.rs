//! One channel for everything the event loop waits on.
//!
//! Key presses and the loader's results (`zrush_core::loader`) arrive on
//! the same `mpsc::Receiver`, so the loop has a single place to block.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use zrush_core::loader::{Event, Loader, Sink};

#[derive(Debug)]
pub enum Msg {
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    /// A result from the loader.
    Core(Event),
}

impl Msg {
    /// The generation a result belongs to; input is never stale.
    pub fn generation(&self) -> Option<u64> {
        match self {
            Self::Key(_) | Self::Resize(..) => None,
            Self::Core(e) => e.generation(),
        }
    }
}

/// A loader whose results come back on `tx`, beside the keys.
pub fn loader(tx: Sender<Msg>) -> Loader {
    let sink: Sink = Arc::new(move |e| tx.send(Msg::Core(e)).is_ok());
    Loader::new(sink)
}

/// Read the keyboard forever, on its own thread, so nothing the loader
/// does can delay a key press.
pub fn spawn_input(tx: Sender<Msg>) {
    std::thread::spawn(move || {
        loop {
            match crossterm::event::read() {
                Ok(crossterm::event::Event::Key(k)) => {
                    if tx.send(Msg::Key(k)).is_err() {
                        return;
                    }
                }
                Ok(crossterm::event::Event::Resize(w, h)) => {
                    if tx.send(Msg::Resize(w, h)).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                // The terminal is gone; so is the reason to read it.
                Err(_) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_never_stale() {
        let k = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Esc);
        assert_eq!(Msg::Key(k).generation(), None);
        assert_eq!(Msg::Resize(80, 24).generation(), None);
    }
}
