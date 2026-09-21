//! Settings window: CLI/git paths, plan defaults, quiz and heatmap options.

use crate::app::AiMentorApp;

pub fn show(app: &mut AiMentorApp, ctx: &egui::Context) {
    let mut open = app.show_settings;
    egui::Window::new("Settings")
        .open(&mut open)
        .resizable(false)
        .default_width(430.0)
        .show(ctx, |ui| {
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Claude CLI path");
                    ui.text_edit_singleline(&mut app.settings_form.claude_path);
                    ui.end_row();

                    ui.label("Prompt flag");
                    ui.text_edit_singleline(&mut app.settings_form.prompt_flag);
                    ui.end_row();

                    ui.label("Extra args");
                    ui.text_edit_singleline(&mut app.settings_form.extra_args);
                    ui.end_row();

                    ui.label("git path");
                    ui.text_edit_singleline(&mut app.settings_form.git_path);
                    ui.end_row();

                    ui.label("Target hours");
                    ui.add(egui::DragValue::new(&mut app.settings_form.target_hours).range(1..=200));
                    ui.end_row();

                    ui.label("Minutes per day");
                    ui.add(
                        egui::DragValue::new(&mut app.settings_form.minutes_per_day).range(10..=480),
                    );
                    ui.end_row();

                    ui.label("Quiz length");
                    ui.add(egui::DragValue::new(&mut app.settings_form.quiz_length).range(3..=25));
                    ui.end_row();

                    ui.label("Heatmap weeks");
                    ui.add(egui::DragValue::new(&mut app.settings_form.heatmap_weeks).range(4..=53));
                    ui.end_row();

                    ui.label("Roadmap unlock %");
                    ui.add(
                        egui::DragValue::new(&mut app.settings_form.unlock_threshold).range(10..=100),
                    );
                    ui.end_row();
                });

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!(
                    "Plans will run {} days ({} h at {} min/day).",
                    crate::mentor::day_count(
                        app.settings_form.target_hours,
                        app.settings_form.minutes_per_day
                    ),
                    app.settings_form.target_hours,
                    app.settings_form.minutes_per_day
                ))
                .small()
                .weak(),
            );

            if let Ok(path) = crate::db::Db::db_path() {
                ui.label(
                    egui::RichText::new(format!("Database: {}", path.display()))
                        .small()
                        .weak(),
                );
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    app.apply_settings();
                }
                if ui.button("Test Claude CLI").clicked() {
                    test_cli(app);
                }
            });
        });
    app.show_settings = open;
}

/// A cheap round-trip so a misconfigured path surfaces before a long job.
fn test_cli(app: &mut AiMentorApp) {
    let cfg = crate::mentor::CliConfig {
        claude_path: app.settings_form.claude_path.clone(),
        prompt_flag: app.settings_form.prompt_flag.clone(),
        extra_args: app.settings_form.extra_args.clone(),
        git_path: app.settings_form.git_path.clone(),
    };
    match crate::mentor::run_cli(&cfg, "Reply with the single word: ready", None) {
        Ok(out) => {
            app.status = format!("CLI replied: {}", out.trim().chars().take(60).collect::<String>());
            app.error = None;
        }
        Err(e) => app.error = Some(e.to_string()),
    }
}
