#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod export;
mod model;
mod ollama;
mod pdf;
mod updates;

use app::NovelQuillApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Novel Quill Studio")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Novel Quill Studio",
        options,
        Box::new(|cc| Ok(Box::new(NovelQuillApp::new(cc)))),
    )
}
