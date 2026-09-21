//! The quiz window: take a generated recall quiz and see the scored result.

use crate::app::AiMentorApp;

pub fn show(app: &mut AiMentorApp, ctx: &egui::Context) {
    if app.quiz.is_none() {
        return;
    }
    let mut keep_open = true;

    egui::Window::new("Recall quiz")
        .open(&mut keep_open)
        .default_width(560.0)
        .vscroll(true)
        .show(ctx, |ui| {
            let Some(state) = app.quiz.as_mut() else {
                return;
            };
            ui.heading(&state.title);
            if state.submitted {
                let (correct, total) = state.score;
                ui.label(
                    egui::RichText::new(format!("Scored {correct} / {total}"))
                        .size(16.0)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(
                        "Anything you missed is now a review card in Today's reviews.",
                    )
                    .small()
                    .weak(),
                );
            }
            ui.separator();

            for (idx, question) in state.questions.iter().enumerate() {
                ui.label(egui::RichText::new(format!("{}. {}", idx + 1, question.question)).strong());
                for (opt_idx, option) in question.options.iter().enumerate() {
                    let picked = state.chosen[idx] == Some(opt_idx);
                    let is_correct = opt_idx as i64 == question.correct_index;

                    let mut text = egui::RichText::new(option);
                    if state.submitted {
                        if is_correct {
                            text = text.color(egui::Color32::from_rgb(76, 160, 106));
                        } else if picked {
                            text = text.color(egui::Color32::from_rgb(200, 70, 70));
                        }
                    }

                    if ui.radio(picked, text).clicked() && !state.submitted {
                        state.chosen[idx] = Some(opt_idx);
                    }
                }
                if state.submitted && !question.explanation.is_empty() {
                    ui.label(egui::RichText::new(&question.explanation).small().weak());
                }
                ui.add_space(8.0);
            }

            let answered = state.chosen.iter().filter(|c| c.is_some()).count();
            let total = state.questions.len();
            let submitted = state.submitted;

            ui.separator();
            ui.horizontal(|ui| {
                if submitted {
                    if ui.button("Close").clicked() {
                        app.quiz = None;
                    }
                } else {
                    ui.label(format!("{answered} of {total} answered"));
                    if ui
                        .add_enabled(answered == total, egui::Button::new("Submit"))
                        .clicked()
                    {
                        app.submit_quiz();
                    }
                }
            });
        });

    if !keep_open {
        app.quiz = None;
    }
}
