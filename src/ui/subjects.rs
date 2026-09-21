//! Left panel: active roadmap, the subject checkbox list, and plan generation.

use crate::app::{AiMentorApp, Tab};
use crate::models::{category_of, CATEGORY_ORDER};
use crate::roadmap;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    roadmap_selector(app, ui);
    ui.separator();

    ui.horizontal(|ui| {
        ui.strong("Subjects");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let selected = app.stacks.iter().filter(|s| s.selected).count();
            ui.label(format!("{selected} selected"));
        });
    });

    egui::ScrollArea::vertical()
        .id_salt("subject_list")
        .max_height(ui.available_height() - 120.0)
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
                egui::CollapsingHeader::new(*category)
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
                                let has_plan =
                                    matches!(app.db.plan_for_stack(id), Ok(Some(_)));
                                let label = if has_plan {
                                    format!("{name}  •")
                                } else {
                                    name.clone()
                                };
                                if ui.selectable_label(is_active, label).clicked() {
                                    app.select_subject(id);
                                    app.tab = Tab::Study;
                                }
                            });
                        }
                    });
            }
        });

    ui.separator();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.custom_subject)
                .hint_text("Add custom subject")
                .desired_width(160.0),
        );
        if ui.button("Add").clicked() {
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

    ui.add_space(4.0);
    if ui
        .add_sized(
            [ui.available_width(), 28.0],
            egui::Button::new("Generate plan for selected"),
        )
        .clicked()
    {
        app.generate_plans_for_selected();
    }

    let due = app.db.due_card_count(&crate::db::today()).unwrap_or(0);
    if ui
        .add_sized(
            [ui.available_width(), 26.0],
            egui::Button::new(format!("Today's reviews ({due} due)")),
        )
        .clicked()
    {
        app.tab = Tab::Reviews;
        app.reload_due_cards();
    }
}

fn roadmap_selector(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let roadmaps = app.db.roadmaps().unwrap_or_default();
    let active = roadmaps.iter().find(|r| r.active).cloned();
    let active_name = active
        .as_ref()
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "(none)".to_string());

    ui.strong("Roadmap");
    egui::ComboBox::from_id_salt("roadmap_combo")
        .selected_text(active_name)
        .width(ui.available_width() - 10.0)
        .show_ui(ui, |ui| {
            for r in &roadmaps {
                if ui.selectable_label(r.active, &r.name).clicked() && !r.active {
                    let _ = app.db.set_active_roadmap(r.id);
                }
            }
        });

    let Some(active) = active else {
        return;
    };
    let Ok(progress) = roadmap::progress(&app.db, active.id) else {
        return;
    };

    ui.add(
        egui::ProgressBar::new(progress.overall)
            .desired_height(12.0)
            .text(format!("{:.0}% of track", progress.overall * 100.0)),
    );

    egui::ScrollArea::vertical()
        .id_salt("roadmap_items")
        .max_height(170.0)
        .show(ui, |ui| {
            for (idx, (_, name, stack_id, done, total, unlocked)) in
                progress.items.iter().enumerate()
            {
                let is_current = progress.current == Some(idx);
                ui.horizontal(|ui| {
                    let marker = if is_current {
                        "▶"
                    } else if *unlocked {
                        "•"
                    } else {
                        "🔒"
                    };
                    ui.label(marker);
                    let label = if *total > 0 {
                        format!("{name}  {done}/{total}")
                    } else {
                        format!("{name}  (no plan)")
                    };
                    if ui
                        .selectable_label(app.active_stack == Some(*stack_id), label)
                        .clicked()
                    {
                        app.select_subject(*stack_id);
                        app.tab = Tab::Study;
                    }
                });
            }
        });
}
