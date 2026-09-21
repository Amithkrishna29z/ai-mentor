//! Study > Day: lesson content, the hands-on gate, interview drill and logging.

use egui_commonmark::CommonMarkViewer;

use crate::app::AiMentorApp;
use crate::models::DayStatus;
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let Some(day) = app.current_day().cloned() else {
        ui.label("Pick a day from the Overview.");
        return;
    };

    navigation(app, ui, day.day_number);
    ui.add_space(4.0);

    let t = theme::current(ui);
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            theme::pill(
                ui,
                format!("difficulty {}/5", day.difficulty),
                theme::difficulty_color(day.difficulty, t.dark),
            );
            ui.label(
                egui::RichText::new(format!("Day {} \u{2014} {}", day.day_number, day.title))
                    .size(18.0)
                    .strong(),
            );
        });
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(format!(
                "{} \u{00b7} {} min",
                day.subskill, day.est_minutes
            ))
            .color(t.text_weak)
            .size(12.5),
        );

        if !day.builds_on.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("BUILDS ON")
                        .size(10.0)
                        .color(t.text_muted)
                        .strong(),
                );
                for chip in &day.builds_on {
                    theme::chip(ui, chip);
                }
            });
        }
    });
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .id_salt("day_scroll")
        .show(ui, |ui| {
            if !day.objectives.is_empty() {
                theme::titled_card(ui, "Objectives", |ui| {
                    for objective in &day.objectives {
                        ui.label(format!("\u{2610}  {objective}"));
                    }
                });
                ui.add_space(8.0);
            }

            theme::card(ui, |ui| content_section(app, ui, &day));
            ui.add_space(8.0);
            hands_on_section(app, ui, &day);
            ui.add_space(8.0);
            theme::card(ui, |ui| interview_section(app, ui, &day));
            ui.add_space(8.0);
            theme::card(ui, |ui| {
                practice_log(app, ui, &day);
                ui.add_space(6.0);
                notes(app, ui, &day);
            });
        });
}

fn navigation(app: &mut AiMentorApp, ui: &mut egui::Ui, day_number: i64) {
    ui.horizontal(|ui| {
        let ids: Vec<i64> = app.days.iter().map(|d| d.id).collect();
        let idx = app
            .selected_day
            .and_then(|id| ids.iter().position(|d| *d == id))
            .unwrap_or(0);

        if ui
            .add_enabled(idx > 0, egui::Button::new("◀ Previous"))
            .clicked()
        {
            app.selected_day = Some(ids[idx - 1]);
            app.sync_day_buffers();
        }
        ui.label(format!("Day {} of {}", day_number, ids.len()));
        if ui
            .add_enabled(idx + 1 < ids.len(), egui::Button::new("Next ▶"))
            .clicked()
        {
            app.selected_day = Some(ids[idx + 1]);
            app.sync_day_buffers();
        }

        ui.separator();
        let mut status = app.current_day().map(|d| d.status).unwrap_or(DayStatus::NotStarted);
        let hands_on_done = app.current_day().map(|d| d.hands_on_done).unwrap_or(false);
        let before = status;
        egui::ComboBox::from_id_salt("day_status")
            .selected_text(status.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut status, DayStatus::NotStarted, "Not started");
                ui.selectable_value(&mut status, DayStatus::InProgress, "In progress");
                // Reading alone never completes a day.
                ui.add_enabled_ui(hands_on_done, |ui| {
                    ui.selectable_value(&mut status, DayStatus::Done, "Done")
                        .on_disabled_hover_text("Tick the hands-on task first");
                });
            });
        if status != before {
            if let Some(id) = app.selected_day {
                if let Err(e) = app.db.set_day_status(id, status) {
                    app.error = Some(e.to_string());
                }
                app.reload_subject();
            }
        }
    });
}

fn content_section(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    ui.horizontal(|ui| {
        theme::section_label(ui, "Lesson");
        if day.content_md.trim().is_empty() {
            if ui.button("Fetch content").clicked() {
                app.fetch_day_content(day.id);
            }
        } else if ui
            .button("Regenerate")
            .on_hover_text("Replaces the stored content, including your edits")
            .clicked()
        {
            app.fetch_day_content(day.id);
        }

        let mut edit = app.edit_mode;
        if ui.toggle_value(&mut edit, "Edit").clicked() {
            app.edit_mode = edit;
            if edit {
                app.edit_buffer = day.content_md.clone();
            }
        }
        if app.edit_mode && ui.button("Save").clicked() {
            let buffer = app.edit_buffer.clone();
            if let Err(e) = app.db.set_day_content(day.id, &buffer, true) {
                app.error = Some(e.to_string());
            } else {
                app.status = "Your edits were saved.".to_string();
                app.edit_mode = false;
                app.reload_subject();
            }
        }
        if day.content_edited {
            ui.label(
                egui::RichText::new("edited — protected from auto-overwrite")
                    .small()
                    .weak(),
            );
        }
    });

    ui.add_space(4.0);
    if app.edit_mode {
        ui.add(
            egui::TextEdit::multiline(&mut app.edit_buffer)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(22),
        );
    } else if day.content_md.trim().is_empty() {
        ui.label(
            egui::RichText::new(
                "No content yet. Fetch it from the Claude CLI, or switch to Edit and write it yourself.",
            )
            .weak(),
        );
    } else {
        let markdown = day.content_md.clone();
        CommonMarkViewer::new().show(ui, &mut app.md_cache, &markdown);
    }
}

fn hands_on_section(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    let t = theme::current(ui);
    egui::Frame::new()
        .fill(t.surface)
        .stroke(egui::Stroke::new(
            1.0,
            if day.hands_on_done { t.good } else { t.border },
        ))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
        ui.horizontal(|ui| {
            theme::section_label(ui, "Hands-on task");
            if day.hands_on_done {
                theme::pill(ui, "done", t.good);
            } else {
                theme::pill(ui, "required to finish the day", t.warning);
            }
        });
        ui.add_space(6.0);
        ui.label(if day.practice_task.trim().is_empty() {
            "(no task recorded for this day)".to_string()
        } else {
            day.practice_task.clone()
        });
        ui.add_space(6.0);
        let mut done = day.hands_on_done;
        if ui.checkbox(&mut done, "Task completed").changed() {
            if let Err(e) = app.db.set_day_hands_on(day.id, done) {
                app.error = Some(e.to_string());
            }
            // Un-ticking must also pull the day back out of "done".
            if !done && day.status == DayStatus::Done {
                let _ = app.db.set_day_status(day.id, DayStatus::InProgress);
            }
            app.reload_subject();
        }
    });
}

fn interview_section(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    egui::CollapsingHeader::new(format!(
        "Interview check ({} question(s))",
        day.interview_questions.len()
    ))
    .default_open(true)
    .show(ui, |ui| {
        if day.interview_questions.is_empty() {
            ui.label("No interview questions on this day.");
        }
        for (idx, q) in day.interview_questions.iter().enumerate() {
            egui::CollapsingHeader::new(format!("Q{}. {}", idx + 1, q.q))
                .id_salt(format!("iq_{}_{}", day.id, idx))
                .show(ui, |ui| {
                    for point in &q.key_points {
                        ui.label(format!("• {point}"));
                    }
                });
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let current = app.db.interview_confidence(day.id).ok().flatten();
            ui.label("Confidence:");
            for grade in 0..=5 {
                let selected = current == Some(grade);
                if ui
                    .selectable_label(selected, grade.to_string())
                    .on_hover_text("0 = blank, 5 = could answer it cold")
                    .clicked()
                {
                    if let Err(e) = app.db.set_interview_confidence(day.id, grade) {
                        app.error = Some(e.to_string());
                    }
                }
            }
            ui.separator();
            if ui.button("Take quiz").clicked() {
                app.start_quiz("day", Some(day.id), None);
            }
            if ui.button("New quiz").on_hover_text("Generate a fresh set").clicked() {
                app.generate_quiz("day", Some(day.id), None);
            }
        });
    });
}

fn practice_log(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    ui.horizontal(|ui| {
        theme::section_label(ui, "Log practice");
        ui.add(
            egui::DragValue::new(&mut app.minutes_input)
                .range(1..=600)
                .suffix(" min"),
        );
        if ui.button("Log").clicked() {
            let minutes = app.minutes_input;
            if let Err(e) = app.db.log_minutes(day.id, minutes) {
                app.error = Some(e.to_string());
            } else {
                app.status = format!("Logged {minutes} minutes.");
                // Logging practice implies the day has started.
                if day.status == DayStatus::NotStarted {
                    let _ = app.db.set_day_status(day.id, DayStatus::InProgress);
                }
                app.reload_subject();
            }
        }
        let logged = app.db.logged_minutes_for_day(day.id).unwrap_or(0);
        ui.label(egui::RichText::new(format!("{logged} min on this day")).weak());
    });
}

fn notes(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    egui::CollapsingHeader::new("Notes")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut app.notes_buffer)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4),
            );
            if ui.button("Save notes").clicked() {
                let notes = app.notes_buffer.clone();
                if let Err(e) = app.db.set_day_notes(day.id, &notes) {
                    app.error = Some(e.to_string());
                } else {
                    app.status = "Notes saved.".to_string();
                    app.reload_subject();
                }
            }
        });
}
