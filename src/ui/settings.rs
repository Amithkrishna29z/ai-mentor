//! Settings window: CLI/git paths, plan defaults, quiz and heatmap options.

use crate::app::AiMentorApp;
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ctx: &egui::Context) {
    let mut open = app.show_settings;
    egui::Window::new("Settings")
        .open(&mut open)
        .resizable(false)
        .default_width(430.0)
        .show(ctx, |ui| {
            theme::section_label(ui, "Claude CLI & plan defaults");
            ui.add_space(6.0);
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

                    ui.label("Concurrent CLI jobs");
                    let mut concurrent = app.max_concurrent as i64;
                    if ui
                        .add(egui::DragValue::new(&mut concurrent).range(1..=4))
                        .on_hover_text(
                            "How many Claude CLI processes may run at once. \
                             1 keeps the course fetching steadily in the background.",
                        )
                        .changed()
                    {
                        app.max_concurrent = concurrent.clamp(1, 4) as usize;
                        let _ = app
                            .db
                            .set_setting("max_concurrent_cli", &app.max_concurrent.to_string());
                        app.pump_queue();
                    }
                    ui.end_row();

                    ui.label("Fetch lessons automatically");
                    let mut auto = app.auto_fetch_lessons;
                    if ui
                        .checkbox(&mut auto, "")
                        .on_hover_text(
                            "After a course is generated, pull every day's lesson from the \
                             Claude CLI so it is ready to read in the app.",
                        )
                        .changed()
                    {
                        app.auto_fetch_lessons = auto;
                        let _ = app
                            .db
                            .set_setting("auto_fetch_lessons", if auto { "1" } else { "0" });
                    }
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

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let t = theme::current(ui);
                let save = egui::Button::new(
                    egui::RichText::new("Save").color(egui::Color32::WHITE).strong(),
                )
                .fill(t.accent)
                .corner_radius(egui::CornerRadius::same(8));
                if ui.add(save).clicked() {
                    app.apply_settings();
                }
                if ui
                    .add_enabled(!app.testing_cli, egui::Button::new("Test Claude CLI"))
                    .clicked()
                {
                    app.test_cli(crate::mentor::CliConfig {
                        claude_path: app.settings_form.claude_path.clone(),
                        prompt_flag: app.settings_form.prompt_flag.clone(),
                        extra_args: app.settings_form.extra_args.clone(),
                        git_path: app.settings_form.git_path.clone(),
                    });
                }
                if app.testing_cli {
                    ui.spinner();
                }
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);
            updates_section(app, ui);
        });
    app.show_settings = open;
}

/// Version, update check and one-click install.
fn updates_section(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let t = theme::current(ui);
    theme::section_label(ui, "Updates");
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label("Installed version");
        theme::chip(ui, format!("v{}", crate::update::current_version()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!app.checking_update, egui::Button::new("Check now"))
                .clicked()
            {
                app.check_for_update();
            }
            if app.checking_update {
                ui.spinner();
            }
        });
    });

    ui.add_space(6.0);
    let mut on_start = app.db.setting_or("check_updates_on_start", "1") == "1";
    if ui
        .checkbox(&mut on_start, "Check for updates when the app starts")
        .changed()
    {
        let _ = app
            .db
            .set_setting("check_updates_on_start", if on_start { "1" } else { "0" });
    }

    ui.add_space(8.0);
    if let Some(version) = app.update_installed.clone() {
        ui.horizontal(|ui| {
            theme::pill(ui, "installed", t.good);
            ui.label(format!("v{version} is in place \u{2014} restart to use it."));
        });
        return;
    }

    match app.update_available.clone() {
        Some(release) => {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::pill(ui, format!("v{}", release.version), t.good);
                    ui.label(egui::RichText::new(&release.name).strong());
                    ui.label(
                        egui::RichText::new(&release.date).size(11.0).color(t.text_muted),
                    );
                });
                if !release.notes.trim().is_empty() {
                    ui.add_space(4.0);
                    let notes: String = release.notes.lines().take(8).collect::<Vec<_>>().join("\n");
                    ui.label(egui::RichText::new(notes).size(11.5).color(t.text_weak));
                }
                ui.add_space(8.0);
                let install = egui::Button::new(
                    egui::RichText::new("Download & install")
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(t.good)
                .corner_radius(egui::CornerRadius::same(8));
                if ui.add(install).clicked() {
                    app.install_update();
                }
                ui.label(
                    egui::RichText::new(
                        "The running app is replaced in place; the new version starts on the next launch.",
                    )
                    .size(11.0)
                    .color(t.text_muted),
                );
            });
        }
        None => {
            ui.label(
                egui::RichText::new("No newer release found.")
                    .size(11.5)
                    .color(t.text_muted),
            );
        }
    }
}

