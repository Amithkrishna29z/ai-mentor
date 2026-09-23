//! Study > Day: lesson content, the hands-on gate, interview drill and logging.

use egui_commonmark::CommonMarkViewer;

use crate::app::AiMentorApp;
use crate::models::DayStatus;
use crate::timer;
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
            .add_enabled(idx > 0, egui::Button::new("⏴ Previous"))
            .clicked()
        {
            app.selected_day = Some(ids[idx - 1]);
            app.sync_day_buffers();
        }
        ui.label(format!("Day {} of {}", day_number, ids.len()));
        if ui
            .add_enabled(idx + 1 < ids.len(), egui::Button::new("Next ⏵"))
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

/// A comfortable measure for long prose. Past roughly this width the eye
/// struggles to find the start of the next line on a maximised window.
const READING_WIDTH: f32 = 760.0;

/// Rough reading time. Technical prose carrying code reads slower than
/// ordinary text, so this counts at 180 wpm and never rounds down to nothing.
fn reading_minutes(markdown: &str) -> i64 {
    let words = markdown.split_whitespace().count();
    ((words as f32 / 180.0).ceil() as i64).max(1)
}


fn content_section(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    let t = theme::current(ui);
    let has_lesson = !day.content_md.trim().is_empty();

    ui.horizontal(|ui| {
        theme::section_label(ui, "Lesson");
        if has_lesson {
            theme::chip(ui, format!("{} min read", reading_minutes(&day.content_md)));
            if day.content_edited {
                theme::chip(ui, "edited")
                    .on_hover_text("Your copy is protected from auto-overwrite");
            }
        }

        // Actions sit right so the heading stays the anchor of the row. In a
        // right-to-left layout the first widget added is the rightmost one.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            let mut edit = app.edit_mode;
            if ui.toggle_value(&mut edit, "Edit").clicked() {
                app.edit_mode = edit;
                if edit {
                    app.edit_buffer = day.content_md.clone();
                }
            }
            if has_lesson {
                if ui
                    .button("Regenerate")
                    .on_hover_text("Replaces the stored content, including your edits")
                    .clicked()
                {
                    app.fetch_day_content(day.id);
                }
            } else if ui.button("Fetch content").clicked() {
                app.fetch_day_content(day.id);
            }
        });
    });

    ui.add_space(8.0);
    if app.edit_mode {
        ui.add(
            egui::TextEdit::multiline(&mut app.edit_buffer)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(22),
        );
    } else if !has_lesson {
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No lesson yet").size(15.0).strong());
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new(
                    "Fetch it from the Claude CLI, or switch to Edit and write it yourself.",
                )
                .size(12.0)
                .color(t.text_muted),
            );
        });
        ui.add_space(10.0);
    } else {
        lesson_body(app, ui, &day.content_md, &t);
    }
}

/// Lessons run past twenty thousand characters, so the reading column gets a
/// measure, a real type scale and room to breathe.
///
/// egui_commonmark sizes headings by interpolating between `Body` and
/// `Heading`, which by default sit only 7px apart - an `##` lands at 19.8px
/// against 14px body and the whole lesson reads as one flat wall. Widening
/// that gap here is what gives the page its hierarchy.
fn lesson_body(app: &mut AiMentorApp, ui: &mut egui::Ui, markdown: &str, t: &theme::Theme) {
    let full = ui.available_width();
    let column = full.min(READING_WIDTH);
    let gutter = ((full - column) * 0.5).max(0.0);

    ui.horizontal_top(|ui| {
        ui.add_space(gutter);
        ui.allocate_ui_with_layout(
            egui::vec2(column, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_max_width(column);
                {
                    let style = ui.style_mut();
                    style.text_styles.insert(
                        egui::TextStyle::Heading,
                        egui::FontId::new(29.0, egui::FontFamily::Proportional),
                    );
                    style.text_styles.insert(
                        egui::TextStyle::Body,
                        egui::FontId::new(15.5, egui::FontFamily::Proportional),
                    );
                    style.text_styles.insert(
                        egui::TextStyle::Monospace,
                        egui::FontId::new(13.5, egui::FontFamily::Monospace),
                    );
                    // Paragraphs and list items need air at this measure.
                    style.spacing.item_spacing.y = 11.0;
                    // Code should read as inset into the card, not blend into it.
                    style.visuals.code_bg_color = if t.dark { t.plane } else { t.surface_alt };
                }
                CommonMarkViewer::new().show(ui, &mut app.md_cache, markdown);
            },
        );
    });
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

/// The 20-hour rule counts practice actually done, so the app times the
/// session rather than asking the learner to remember and estimate it.
fn timer_row(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    let t = theme::current(ui);
    let now = ui.input(|i| i.time);
    // Copy out what the row draws, so the arms can still borrow `app` mutably.
    let session = app
        .timer
        .as_ref()
        .map(|s| (s.day_id, s.is_running(), s.elapsed(now)));

    ui.horizontal(|ui| {
        theme::section_label(ui, "Practice timer");
        ui.add_space(4.0);

        match session {
            // A timer belongs to the day it was started on. Say where it is
            // running rather than quietly logging its minutes against this day.
            Some((owner, running, elapsed)) if owner != day.id => {
                let which = app
                    .days
                    .iter()
                    .find(|d| d.id == owner)
                    .map(|d| format!("Day {}", d.day_number))
                    .unwrap_or_else(|| "another day".to_string());
                theme::chip(
                    ui,
                    format!(
                        "{} {} on {which}",
                        timer::format_clock(elapsed),
                        if running { "running" } else { "paused" }
                    ),
                );
            }
            Some((_, running, elapsed)) => {
                ui.label(
                    egui::RichText::new(timer::format_clock(elapsed))
                        .size(21.0)
                        .monospace()
                        .strong()
                        .color(if running { t.accent } else { t.text_weak }),
                );
                ui.add_space(4.0);
                if ui.button(if running { "Pause" } else { "Resume" }).clicked() {
                    app.toggle_timer(now);
                }
                let stop = egui::Button::new(
                    egui::RichText::new("Stop & log")
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(t.accent)
                .corner_radius(egui::CornerRadius::same(8));
                if ui
                    .add(stop)
                    .on_hover_text("Log the elapsed minutes against this day")
                    .clicked()
                {
                    app.stop_timer(now);
                }
            }
            None => {
                let start = egui::Button::new(
                    egui::RichText::new("Start").color(egui::Color32::WHITE).strong(),
                )
                .fill(t.accent)
                .corner_radius(egui::CornerRadius::same(8));
                if ui
                    .add(start)
                    .on_hover_text("Time this session and log it when you stop")
                    .clicked()
                {
                    app.start_timer(day.id, now);
                }
                ui.label(
                    egui::RichText::new(format!("{} min planned", day.est_minutes))
                        .size(11.5)
                        .color(t.text_muted),
                );
            }
        }
    });
}

fn practice_log(app: &mut AiMentorApp, ui: &mut egui::Ui, day: &crate::models::Day) {
    timer_row(app, ui, day);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        theme::section_label(ui, "Or log it by hand");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_time_rounds_up_and_never_reads_as_zero() {
        assert_eq!(reading_minutes(""), 1, "an empty lesson still shows a minute");
        assert_eq!(reading_minutes("one two three"), 1);

        let words = vec!["word"; 180].join(" ");
        assert_eq!(reading_minutes(&words), 1);

        let words = vec!["word"; 181].join(" ");
        assert_eq!(reading_minutes(&words), 2, "a part minute rounds up");

        let words = vec!["word"; 3600].join(" ");
        assert_eq!(reading_minutes(&words), 20);
    }
}
