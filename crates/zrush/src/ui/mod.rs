//! The terminal interface.

pub mod app;
pub mod filter;
pub mod header;
pub mod modal;
pub mod preview;
pub mod screen;
pub mod table;
pub mod theme;

#[cfg(test)]
pub mod tests_support {
    use ratatui::backend::TestBackend;

    /// The whole frame as one string, for asserting that something is on
    /// screen without caring where.
    pub fn flatten(backend: &TestBackend) -> String {
        let buf = backend.buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }
}
