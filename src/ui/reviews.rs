//! Reviews tab: the SM-2 due queue.

use crate::app::AiMentorApp;
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let t = theme::current(ui);
    let remaining = app.due_cards.len().saturating_sub(app.review_index);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Today's reviews").size(19.0).strong());
        ui.add_space(4.0);
        theme::pill(ui, format!("{remaining} due"), if remaining > 0 { t.accent } else { t.good });
        theme::chip(ui, format!("{} done today", app.reviewed_today));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.reload_due_cards();
            }
        });
    });
    ui.add_space(10.0);

    let Some(card) = app.due_cards.get(app.review_index).cloned() else {
        ui.add_space(42.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("\u{2714}").size(28.0).monospace().color(t.good));
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Nothing due right now.").size(15.0).strong());
            ui.label(
                egui::RichText::new(
                    "Cards come from each day's interview questions and from quiz mistakes.",
                )
                .color(t.text_muted),
            );
        });
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt("review_scroll")
        .show(ui, |ui| {
            theme::card(ui, |ui| {
                theme::pill(
                    ui,
                    card.source_type.clone(),
                    if card.source_type == "quiz" { t.serious } else { t.accent },
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&card.front).size(17.0).strong());
                ui.add_space(12.0);

                if app.review_revealed {
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(&card.back).color(t.text_weak));
                } else {
                    let reveal = egui::Button::new(
                        egui::RichText::new("Reveal answer").color(egui::Color32::WHITE).strong(),
                    )
                    .fill(t.accent)
                    .corner_radius(egui::CornerRadius::same(8));
                    if ui.add(reveal).clicked() {
                        app.review_revealed = true;
                    }
                }
            });

            if app.review_revealed {
                ui.add_space(12.0);
                theme::card(ui, |ui| {
                    theme::section_label(ui, "How well did you recall it?");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for grade in 0..=5 {
                            // 0-2 is a lapse, 3-5 a pass: colour the two bands.
                            let fill = if grade < 3 { t.critical } else { t.good };
                            let button = egui::Button::new(
                                egui::RichText::new(format!(" {grade} "))
                                    .color(egui::Color32::WHITE)
                                    .strong(),
                            )
                            .fill(fill.gamma_multiply(0.85))
                            .corner_radius(egui::CornerRadius::same(8));
                            if ui.add(button).on_hover_text(grade_hint(grade)).clicked() {
                                app.grade_current_card(grade);
                            }
                        }
                    });
                });
            }

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!(
                    "interval {} d \u{00b7} repetitions {} \u{00b7} ease {:.2}",
                    card.interval_days, card.repetitions, card.easiness
                ))
                .size(11.0)
                .color(t.text_muted),
            );
        });
}

fn grade_hint(grade: i64) -> &'static str {
    match grade {
        0 => "Blank — no recall at all",
        1 => "Wrong, but the answer felt familiar",
        2 => "Wrong, and it was easy to see why",
        3 => "Correct, with serious effort",
        4 => "Correct, after a small hesitation",
        _ => "Perfect, instant recall",
    }
}
