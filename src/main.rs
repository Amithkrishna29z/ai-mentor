//! AI Mentor — a 20-hour-rule study coach driven by the Claude CLI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analytics;
mod app;
mod db;
mod github;
mod mentor;
mod models;
mod quiz;
mod roadmap;
mod srs;
mod ui;
mod verify;

fn main() -> eframe::Result<()> {
    let database = match db::Db::open() {
        Ok(db) => db,
        Err(e) => {
            eprintln!("AI Mentor could not open its database: {e:#}");
            std::process::exit(1);
        }
    };

    // Restore the last window size before the window is created.
    let width = database.setting_i64("window_w", 1280) as f32;
    let height = database.setting_i64("window_h", 860) as f32;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width.max(800.0), height.max(560.0)])
            .with_min_inner_size([900.0, 560.0])
            .with_title("AI Mentor"),
        ..Default::default()
    };

    eframe::run_native(
        "AI Mentor",
        options,
        Box::new(|_cc| Ok(Box::new(app::AiMentorApp::new(database)))),
    )
}
