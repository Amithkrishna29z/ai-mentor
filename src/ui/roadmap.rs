//! Roadmap tab: the ordered track, reordering, and roadmap management.

use crate::app::{AiMentorApp, Tab};
use crate::roadmap;

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
            }
        });
    });

    let Ok(progress) = roadmap::progress(&app.db, active.id) else {
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
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("roadmap_track")
        .show(ui, |ui| {
            for (idx, (item_id, name, stack_id, done, total, unlocked)) in
                progress.items.iter().enumerate()
            {
                let is_current = progress.current == Some(idx);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}.", idx + 1));
                        let marker = if is_current {
                            "▶ next up"
                        } else if *unlocked {
                            "unlocked"
                        } else {
                            "locked"
                        };
                        ui.label(egui::RichText::new(marker).small().weak());

                        if ui
                            .selectable_label(app.active_stack == Some(*stack_id), name)
                            .clicked()
                        {
                            app.select_subject(*stack_id);
                            app.tab = Tab::Study;
                        }

                        let fraction = roadmap::fraction(*done, *total);
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(160.0)
                                .text(if *total > 0 {
                                    format!("{done}/{total} days")
                                } else {
                                    "no plan".to_string()
                                }),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Remove").clicked() {
                                let _ = app.db.remove_roadmap_item(*item_id);
                            }
                            if ui.small_button("▼").clicked() {
                                let _ = roadmap::move_item(&app.db, active.id, *item_id, 1);
                            }
                            if ui.small_button("▲").clicked() {
                                let _ = roadmap::move_item(&app.db, active.id, *item_id, -1);
                            }
                        });
                    });
                });
            }
        });

    ui.separator();
    add_controls(app, ui, active.id);
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
                        app.new_roadmap_name.clear();
                        app.status = format!("Roadmap \"{name}\" created.");
                    }
                    Err(e) => app.error = Some(e.to_string()),
                }
            }
        }
    });
}
