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
mod timer;
mod ui;
mod update;
mod verify;

/// wgpu probes every backend it is built with when it starts up, and the WGL
/// (OpenGL) probe overflows its thread's stack on some Windows drivers - the
/// app dies before the window ever appears, roughly one launch in six. The
/// default backend set is `PRIMARY | GL`; Windows always has DX12, so drop the
/// GL probe. An explicit `WGPU_BACKEND` from the environment still wins.
#[cfg(windows)]
fn skip_fragile_gpu_backends() {
    if std::env::var_os("WGPU_BACKEND").is_none() {
        std::env::set_var("WGPU_BACKEND", "dx12,vulkan");
    }
}

#[cfg(not(windows))]
fn skip_fragile_gpu_backends() {}

/// Smallest window the layout still works in, in points. Shared with the
/// runtime monitor fit so the two can never disagree.
pub const MIN_INNER_SIZE: [f32; 2] = [900.0, 560.0];

fn main() -> eframe::Result<()> {
    skip_fragile_gpu_backends();

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
            .with_min_inner_size(MIN_INNER_SIZE)
            // A size saved on a bigger screen must not open off-display here.
            .with_clamp_size_to_monitor_size(true)
            .with_title("AI Mentor"),
        // eframe clamps the restored *size* to the monitor, but only clamps the
        // *position* when it has persisted window settings - and persistence is
        // off here. So the window opened wherever the OS put it, often far
        // enough right that its edge hung off the display. Centring it is what
        // actually keeps the whole window on screen.
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "AI Mentor",
        options,
        Box::new(|_cc| Ok(Box::new(app::AiMentorApp::new(database)))),
    )
}
