//! Left panel: active roadmap, the subject checkbox list, and plan generation.

use crate::app::{AiMentorApp, Tab};
use crate::models::{category_of, CATEGORY_ORDER};
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    // Bottom-up so the actions stay pinned above the status bar and the lists
    // take whatever height is left, instead of pushing them off-screen.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        actions(app, ui);
        ui.add_space(6.0);
        ui.separator();

        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            roadmap_selector(app, ui);
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            subject_list(app, ui);
        });
    });
}

fn actions(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let t = theme::current(ui);
    let width = ui.available_width();

    // What is left in the queue the Reviews tab is showing, not a fresh count
    // per frame.
    let due = app.due_cards.len().saturating_sub(app.review_index);
    let reviews = egui::Button::new(if due > 0 {
        egui::RichText::new(format!("Today's reviews · {due} due"))
            .color(t.text)
            .strong()
    } else {
        egui::RichText::new("Today's reviews").color(t.text_weak)
    })
    .corner_radius(egui::CornerRadius::same(8));
    if ui.add_sized([width, 28.0], reviews).clicked() {
        app.tab = Tab::Reviews;
        app.reload_due_cards();
    }

    ui.add_space(6.0);
    let generate = egui::Button::new(
        egui::RichText::new("Generate plan for selected")
            .color(egui::Color32::WHITE)
            .strong(),
    )
    .fill(t.accent)
    .corner_radius(egui::CornerRadius::same(8));
    if ui.add_sized([width, 30.0], generate).clicked() {
        app.generate_plans_for_selected();
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let button_w = 46.0;
        ui.add_sized(
            [width - button_w - ui.spacing().item_spacing.x, 24.0],
            egui::TextEdit::singleline(&mut app.custom_subject).hint_text("Add custom subject"),
        );
        if ui.add_sized([button_w, 24.0], egui::Button::new("Add")).clicked() {
            let name = app.custom_subject.trim().to_string();
            if !name.is_empty() {
                match app.db.add_custom_stack(&name) {
                    Ok(_) => {
                        app.custom_subject.clear();
                        app.reload_stacks();
                        app.status = format!("Added {name}.");
                    }
                    Err(e) => app.error = Some(e.to_string()),
                }
            }
        }
    });
}

fn subject_list(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let t = theme::current(ui);
    ui.horizontal(|ui| {
        theme::section_label(ui, "Subjects");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let selected = app.stacks.iter().filter(|s| s.selected).count();
            if selected > 0 {
                theme::pill(ui, format!("{selected} selected"), t.accent);
            }
        });
    });
    ui.add_space(2.0);

    egui::ScrollArea::vertical()
        .id_salt("subject_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for category in CATEGORY_ORDER {
                let rows: Vec<(i64, String, bool)> = app
                    .stacks
                    .iter()
                    .filter(|s| category_of(&s.name) == *category)
                    .map(|s| (s.id, s.name.clone(), s.selected))
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(
                    egui::RichText::new(*category).color(t.text_weak).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    for (id, name, selected) in rows {
                        ui.horizontal(|ui| {
                            let mut checked = selected;
                            if ui.checkbox(&mut checked, "").changed() {
                                if let Err(e) = app.db.set_stack_selected(id, checked) {
                                    app.error = Some(e.to_string());
                                }
                                app.reload_stacks();
                            }
                            let is_active = app.active_stack == Some(id);
                            let text = if is_active {
                                egui::RichText::new(&name).color(t.text).strong()
                            } else {
                                egui::RichText::new(&name).color(t.text_weak)
                            };
                            if ui.selectable_label(is_active, text).clicked() {
                                app.select_subject(id);
                                app.tab = Tab::Study;
                            }
                            if app.stacks_with_plans.contains(&id) {
                                ui.label(egui::RichText::new("⏺").size(8.0).color(t.good))
                                    .on_hover_text("plan generated");
                            }
                        });
                    }
                });
            }
        });
}

fn roadmap_selector(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let roadmaps = app.db.roadmaps().unwrap_or_default();
    let active = roadmaps.iter().find(|r| r.active).cloned();
    let active_name = active
        .as_ref()
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "(none)".to_string());

    let t = theme::current(ui);
    theme::section_label(ui, "Roadmap");
    ui.add_space(3.0);
    egui::ComboBox::from_id_salt("roadmap_combo")
        .selected_text(active_name)
        .width(ui.available_width() - 8.0)
        .show_ui(ui, |ui| {
            for r in &roadmaps {
                if ui.selectable_label(r.active, &r.name).clicked() && !r.active {
                    let _ = app.db.set_active_roadmap(r.id);
                    app.reload_track();
                }
            }
        });

    if active.is_none() {
        return;
    }
    let Some(progress) = app.track.clone() else {
        return;
    };

    ui.add_space(7.0);
    theme::meter(
        ui,
        "Track progress",
        progress.overall,
        &format!("{:.0}%", progress.overall * 100.0),
    );
    ui.add_space(5.0);

    // Size the list to whole rows so it never clips one in half.
    let row_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
    egui::ScrollArea::vertical()
        .id_salt("roadmap_items")
        .max_height(row_height * 4.0)
        .show(ui, |ui| {
            for (idx, entry) in progress.entries.iter().enumerate() {
                let is_current = progress.current == Some(idx);
                ui.horizontal(|ui| {
                    let (marker, colour) = if is_current {
                        ("⏵", t.accent)
                    } else if entry.cleared {
                        ("⏺", t.good)
                    } else if entry.unlocked {
                        ("⏺", t.text_weak)
                    } else {
                        ("○", t.text_muted)
                    };
                    ui.label(egui::RichText::new(marker).size(9.0).color(colour));

                    let active = app.active_stack == Some(entry.stack_id);
                    let text = if active {
                        egui::RichText::new(&entry.subject).color(t.text).strong()
                    } else {
                        egui::RichText::new(&entry.subject).color(t.text_weak)
                    };
                    // Locked subjects stay unreachable from the track itself.
                    if entry.unlocked {
                        if ui.selectable_label(active, text).clicked() {
                            app.select_subject(entry.stack_id);
                            app.tab = Tab::Study;
                        }
                    } else {
                        ui.add_enabled_ui(false, |ui| ui.selectable_label(active, text))
                            .inner
                            .on_disabled_hover_text("Clear the subjects above it first");
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if entry.days_total > 0 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}/{}",
                                    entry.days_done, entry.days_total
                                ))
                                .size(10.5)
                                .color(t.text_muted)
                                .monospace(),
                            );
                        }
                    });
                });
            }
        });
}
