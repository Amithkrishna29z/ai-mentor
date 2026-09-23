//! Roadmap tab: the ordered track, reordering, and roadmap management.

use crate::app::{AiMentorApp, Tab};
use crate::roadmap;
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let roadmaps = app.db.roadmaps().unwrap_or_default();
    let Some(active) = roadmaps.iter().find(|r| r.active).cloned() else {
        ui.label("No roadmap is active.");
        return;
    };

    ui.horizontal(|ui| {
        ui.heading(&active.name);
        if active.is_preset {
            ui.label(egui::RichText::new("preset").small().weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !active.is_preset && ui.button("Delete roadmap").clicked() {
                let _ = app.db.delete_roadmap(active.id);
                if let Some(first) = app.db.roadmaps().unwrap_or_default().first() {
                    let _ = app.db.set_active_roadmap(first.id);
                }
                app.reload_track();
            }
        });
    });

    let Some(progress) = app.track.clone() else {
        return;
    };
    ui.add(
        egui::ProgressBar::new(progress.overall)
            .desired_height(18.0)
            .text(format!("{:.0}% of the track complete", progress.overall * 100.0)),
    );
    ui.label(
        egui::RichText::new(format!(
            "A subject unlocks the next one at {}% of its days done.",
            app.settings_form.unlock_threshold
        ))
        .small()
        .weak(),
    );
    ui.add_space(8.0);
    target_job(app, ui);
    ui.add_space(8.0);
    next_up(app, ui, &progress);
    ui.add_space(8.0);
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("roadmap_track")
        .show(ui, |ui| {
            let threshold = app.settings_form.unlock_threshold;
            for (idx, entry) in progress.entries.iter().enumerate() {
                let is_current = progress.current == Some(idx);
                let (item_id, stack_id) = (&entry.item_id, &entry.stack_id);
                let (done, total) = (&entry.days_done, &entry.days_total);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    let t = theme::current(ui);
                    ui.horizontal(|ui| {
                        ui.label(format!("{}.", idx + 1));
                        let marker = if is_current {
                            "⏵ next up"
                        } else if entry.cleared {
                            "cleared"
                        } else if entry.unlocked {
                            "unlocked"
                        } else {
                            "locked"
                        };
                        ui.label(egui::RichText::new(marker).small().weak());

                        // The track is an order, not a menu: a locked subject
                        // cannot be opened from here. The Subjects list below
                        // stays free for anyone who wants to step outside it.
                        let selected = app.active_stack == Some(*stack_id);
                        if entry.unlocked {
                            if ui.selectable_label(selected, &entry.subject).clicked() {
                                app.select_subject(*stack_id);
                                app.tab = Tab::Study;
                            }
                        } else {
                            ui.add_enabled_ui(false, |ui| {
                                ui.selectable_label(selected, &entry.subject)
                            })
                            .inner
                            .on_disabled_hover_text(format!(
                                "Locked until every subject above reaches {threshold}% of its days done"
                            ));
                        }

                        // A bar only reads as progress when there is progress to
                        // show; an empty course gets a plain chip instead.
                        if *total > 0 {
                            ui.add(
                                egui::ProgressBar::new(roadmap::fraction(*done, *total))
                                    .desired_width(160.0)
                                    .desired_height(8.0)
                                    .corner_radius(egui::CornerRadius::same(4))
                                    .fill(t.accent),
                            );
                            ui.label(
                                egui::RichText::new(format!("{done}/{total} days"))
                                    .size(11.5)
                                    .color(t.text_muted)
                                    .monospace(),
                            );
                        } else {
                            theme::chip(ui, "no course yet");
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Worded, not glyphed: the arrow/cross glyphs are
                            // absent from egui's bundled font and render as boxes.
                            if ui
                                .small_button("Remove")
                                .on_hover_text("Take this subject off the track")
                                .clicked()
                            {
                                let _ = app.db.remove_roadmap_item(*item_id);
                                app.reload_track();
                            }
                            if ui.small_button("Down").clicked() {
                                let _ = roadmap::move_item(&app.db, active.id, *item_id, 1);
                                app.reload_track();
                            }
                            if ui.small_button("Up").clicked() {
                                let _ = roadmap::move_item(&app.db, active.id, *item_id, -1);
                                app.reload_track();
                            }
                        });
                    });
                });
            }
        });

    ui.separator();
    add_controls(app, ui, active.id);
}

/// The job being trained for. Paste a real posting and the track is rebuilt
/// from the skills it asks for, in the order the posting says to learn them.
fn target_job(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let t = theme::current(ui);
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            theme::section_label(ui, "Target job");
            let role = app.target_role.trim().to_string();
            if role.is_empty() {
                theme::chip(ui, "none set");
            } else {
                theme::pill(ui, role, t.accent);
                let seniority = app.target_seniority.trim().to_string();
                if !seniority.is_empty() {
                    theme::chip(ui, seniority);
                }
            }
            if app.reading_job {
                ui.spinner();
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Paste a real posting. The skills it asks for become your track, in the \
                 order to learn them, and every course from then on is written for that role.",
            )
            .size(11.5)
            .color(t.text_muted),
        );
        ui.add_space(6.0);

        let header = if app.target_role.trim().is_empty() {
            "Paste a job posting"
        } else {
            "Use a different posting"
        };
        egui::CollapsingHeader::new(header)
            .id_salt("jd_box")
            .default_open(app.target_role.trim().is_empty())
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut app.jd_input)
                        .hint_text("The whole posting - responsibilities and requirements")
                        .desired_width(f32::INFINITY)
                        .desired_rows(6),
                );
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    let build = egui::Button::new(
                        egui::RichText::new("Build my track from this job")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(t.accent)
                    .corner_radius(egui::CornerRadius::same(8));
                    if ui.add_enabled(!app.reading_job, build).clicked() {
                        app.read_job_description();
                    }
                    if ui.button("Clear").clicked() {
                        app.jd_input.clear();
                    }
                    ui.label(
                        egui::RichText::new("Replaces the track, not your courses.")
                            .size(11.0)
                            .color(t.text_muted),
                    );
                });
            });
    });
}

/// The one thing the track is for: what to work on now, and one click to get
/// there. When a subject clears, `current` moves on by itself and this card
/// points at the next skill.
fn next_up(app: &mut AiMentorApp, ui: &mut egui::Ui, progress: &roadmap::RoadmapProgress) {
    let t = theme::current(ui);
    let Some(idx) = progress.current else {
        theme::card(ui, |ui| {
            ui.label(
                egui::RichText::new("Track complete")
                    .size(16.0)
                    .strong()
                    .color(t.good),
            );
            ui.label(
                egui::RichText::new("Every subject is past the unlock threshold.")
                    .size(12.0)
                    .color(t.text_muted),
            );
        });
        return;
    };

    let entry = &progress.entries[idx];
    let builds_on = progress.cleared_before(idx);
    let has_plan = app.stacks_with_plans.contains(&entry.stack_id);

    theme::card(ui, |ui| {
        theme::section_label(ui, "Work on this now");
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&entry.subject).size(18.0).strong());
            theme::chip(ui, format!("step {} of {}", idx + 1, progress.entries.len()));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if has_plan {
                    "Continue"
                } else {
                    "Generate course"
                };
                let go = egui::Button::new(
                    egui::RichText::new(label).color(egui::Color32::WHITE).strong(),
                )
                .fill(t.accent)
                .corner_radius(egui::CornerRadius::same(8));
                if ui.add(go).clicked() {
                    app.start_next_on_track();
                }
            });
        });
        ui.add_space(3.0);
        let line = if builds_on.is_empty() {
            "First subject on the track - it starts from the ground up.".to_string()
        } else {
            format!("Builds on {}.", builds_on.join(", "))
        };
        ui.label(egui::RichText::new(line).size(12.0).color(t.text_weak));
    });
}

fn add_controls(app: &mut AiMentorApp, ui: &mut egui::Ui, roadmap_id: i64) {
    let existing: Vec<i64> = app
        .db
        .roadmap_items(roadmap_id)
        .unwrap_or_default()
        .iter()
        .map(|i| i.tech_stack_id)
        .collect();

    ui.horizontal(|ui| {
        ui.label("Add subject:");
        let candidates: Vec<(i64, String)> = app
            .stacks
            .iter()
            .filter(|s| !existing.contains(&s.id))
            .map(|s| (s.id, s.name.clone()))
            .collect();
        egui::ComboBox::from_id_salt("add_roadmap_subject")
            .selected_text("choose…")
            .show_ui(ui, |ui| {
                for (id, name) in candidates {
                    if ui.selectable_label(false, name).clicked() {
                        let _ = app.db.add_roadmap_item(roadmap_id, id);
                        app.reload_track();
                    }
                }
            });
    });

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.new_roadmap_name)
                .hint_text("New roadmap name")
                .desired_width(220.0),
        );
        if ui.button("Create & activate").clicked() {
            let name = app.new_roadmap_name.trim().to_string();
            if !name.is_empty() {
                match app.db.create_roadmap(&name) {
                    Ok(id) => {
                        let _ = app.db.set_active_roadmap(id);
                        app.reload_track();
                        app.new_roadmap_name.clear();
                        app.status = format!("Roadmap \"{name}\" created.");
                    }
                    Err(e) => app.error = Some(e.to_string()),
                }
            }
        }
    });
}
