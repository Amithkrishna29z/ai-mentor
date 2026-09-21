//! Reviews tab: the SM-2 due queue.

use crate::app::AiMentorApp;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Today's reviews");
        ui.label(format!(
            "{} due · {} done today",
            app.due_cards.len().saturating_sub(app.review_index),
            app.reviewed_today
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.reload_due_cards();
            }
        });
    });
    ui.separator();

    let Some(card) = app.due_cards.get(app.review_index).cloned() else {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.label("Nothing due right now.");
            ui.label(
                egui::RichText::new(
                    "Cards come from each day's interview questions and from quiz mistakes.",
                )
                .weak(),
            );
        });
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt("review_scroll")
        .show(ui, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("source: {}", card.source_type))
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&card.front).size(17.0).strong());
                ui.add_space(10.0);

                if app.review_revealed {
                    ui.separator();
                    ui.label(&card.back);
                } else if ui.button("Reveal answer").clicked() {
                    app.review_revealed = true;
                }
            });

            if app.review_revealed {
                ui.add_space(10.0);
                ui.label("How well did you recall it?");
                ui.horizontal(|ui| {
                    for grade in 0..=5 {
                        if ui
                            .button(format!("{grade}"))
                            .on_hover_text(grade_hint(grade))
                            .clicked()
                        {
                            app.grade_current_card(grade);
                        }
                    }
                });
            }

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "interval {} d · repetitions {} · ease {:.2}",
                    card.interval_days, card.repetitions, card.easiness
                ))
                .small()
                .weak(),
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
