// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Christophe Opoix

//! On Windows, the icon goes inside zrush.exe. The window sets its own once
//! running, but a button pinned to the taskbar, or the file in Explorer,
//! shows the program's: without one, Windows draws a blank application.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        // A build without it would ship the blank button: better not build.
        if let Err(e) = res.compile() {
            panic!("could not put the icon into zrush.exe: {e}");
        }
    }
}
