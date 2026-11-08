//! The event loop.

use super::{App, Arc, Zrush, apply, draw, header, mpsc, preview_for, table, want_preview};

/// How often the spinner turns while a worker is out. Fast enough to read
/// as motion, slow enough that a long action is not a busy loop.
pub(super) const SPIN_EVERY: std::time::Duration = std::time::Duration::from_millis(90);

/// Run the interface until it is told to stop. What comes back is the
/// release to install, when that is why it stopped.
pub fn run(
    mut app: App,
    z: &Arc<Zrush>,
    check_updates: bool,
) -> zrush_core::error::Result<Option<String>> {
    let mut term = ratatui::init();
    let (tx, rx) = mpsc::channel();
    let probes = crate::probe::loader(tx.clone());
    crate::probe::spawn_input(tx);
    probes.refresh(z, false);
    if check_updates {
        probes.spawn_update_check(z.dirs().clone());
    }

    let out = loop {
        want_preview(&mut app, z, &probes);
        let prev = preview_for(&app);
        if let Err(e) = term.draw(|f| draw(f, &app, z, &prev)) {
            break Err(e.into());
        }
        let height = term
            .size()
            .map_or(10, |s| s.height.saturating_sub(header::HEIGHT + 3));
        app.offset = table::scroll_to(app.offset, app.cursor, height as usize);

        // Blocking recv while idle: a terminal that redraws for nothing
        // costs battery on a tool that sits open all day. The timeout
        // exists only to turn the spinner.
        let event = if app.busy.is_some() {
            match rx.recv_timeout(SPIN_EVERY) {
                Ok(e) => Some(e),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    app.spin = app.spin.wrapping_add(1);
                    None
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
            }
        } else {
            match rx.recv() {
                Ok(e) => Some(e),
                Err(_) => break Ok(()),
            }
        };
        if let Some(event) = event
            && !apply(&mut app, z, &probes, event)
        {
            break Ok(());
        }
    };
    ratatui::restore();
    out.map(|()| app.install)
}
