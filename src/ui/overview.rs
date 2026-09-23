//! Study > Overview: subskills, the day grid, and 20-hour progress.

use crate::app::{status_color, AiMentorApp, StudyView};
use crate::models::{DayStatus, PlanJson};
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let Some(plan) = app.plan.clone() else {
        return;
    };
    let logged = app.logged_minutes();
    let target_minutes = plan.target_hours * 60;
    let fraction = if target_minutes > 0 {
        (logged as f32 / target_minutes as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    egui::ScrollArea::vertical()
        .id_salt("overview_scroll")
        .show(ui, |ui| {
            let t = theme::current(ui);
            let done = app.days.iter().filter(|d| d.status == DayStatus::Done).count();
            let reached = app
                .days
                .iter()
                .filter(|d| d.status == DayStatus::Done)
                .map(|d| d.difficulty)
                .max()
                .unwrap_or(0);

            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(app.active_stack_name()).size(19.0).strong());
                    ui.add_space(4.0);
                    theme::chip(ui, format!("{} days", app.days.len()));
                    theme::chip(ui, format!("{} min/day", plan.minutes_per_day));
                    theme::chip(ui, format!("{} h target", plan.target_hours));
                });
                ui.add_space(10.0);
                theme::meter(
                    ui,
                    "Practice toward the 20-hour goal",
                    fraction,
                    &format!("{:.1} / {} h", logged as f32 / 60.0, plan.target_hours),
                );
                ui.add_space(8.0);
                theme::meter(
                    ui,
                    "Difficulty reached",
                    reached as f32 / 5.0,
                    &format!("{reached} / 5"),
                );
                ui.add_space(8.0);
                theme::meter(
                    ui,
                    "Days completed",
                    if app.days.is_empty() { 0.0 } else { done as f32 / app.days.len() as f32 },
                    &format!("{done} / {}", app.days.len()),
                );
            });

            ui.add_space(10.0);
            subskills(app, ui, &plan);
            ui.add_space(10.0);

            theme::card(ui, |ui| {
                theme::section_label(ui, "Days");
                ui.label(
                    egui::RichText::new(
                        "Difficulty never decreases \u{2014} later days combine earlier subskills.",
                    )
                    .size(11.5)
                    .color(t.text_muted),
                );
                ui.add_space(8.0);
                day_grid(app, ui);
            });
        });
}

fn subskills(app: &AiMentorApp, ui: &mut egui::Ui, plan: &crate::models::Plan) {
    let parsed: Option<PlanJson> = serde_json::from_str(&plan.plan_json)
        .ok()
        .or_else(|| crate::mentor::extract_json(&plan.plan_json).and_then(|j| serde_json::from_str(j).ok()));

    theme::titled_card(ui, "High-leverage subskills", |ui| match parsed {
        Some(p) if !p.subskills.is_empty() => {
            let t = theme::current(ui);
            let mut list = p.subskills;
            list.sort_by_key(|s| s.priority);
            for s in list {
                ui.horizontal_top(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:02}", s.priority))
                            .color(t.accent)
                            .monospace(),
                    );
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&s.name).strong());
                        if !s.why.is_empty() {
                            ui.label(egui::RichText::new(&s.why).size(11.5).color(t.text_weak));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        }
        // Fall back to the distinct subskills recorded on the days.
        _ => {
            let mut seen: Vec<&str> = Vec::new();
            for day in &app.days {
                if !seen.contains(&day.subskill.as_str()) {
                    seen.push(&day.subskill);
                }
            }
            for name in seen {
                ui.label(format!("\u{2022} {name}"));
            }
        }
    });
}

fn day_grid(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let days: Vec<_> = app.days.clone();
    let mut week = 0;

    let t = theme::current(ui);

    for day in days {
        if day.week_number != week {
            week = day.week_number;
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("WEEK {week}"))
                    .size(10.5)
                    .color(t.text_muted)
                    .strong(),
            );
            ui.add_space(2.0);
        }

        let selected = app.selected_day == Some(day.id);
        let fill = if selected { t.surface_alt } else { egui::Color32::TRANSPARENT };
        let response = egui::Frame::new()
            .fill(fill)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::pill(
                        ui,
                        day.difficulty.to_string(),
                        theme::difficulty_color(day.difficulty, t.dark),
                    )
                    .on_hover_text(format!("difficulty {}/5", day.difficulty));
                    ui.colored_label(status_color(day.status, &t), "\u{23fa}")
                        .on_hover_text(day.status.label());
                    ui.label(
                        egui::RichText::new(format!("Day {}", day.day_number))
                            .color(t.text_muted)
                            .size(12.0),
                    );
                    ui.label(egui::RichText::new(&day.title).color(t.text));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} min", day.est_minutes))
                                .size(11.0)
                                .color(t.text_muted),
                        );
                        if day.hands_on_done {
                            ui.label(
                                egui::RichText::new("\u{2714}")
                                    .size(11.0)
                                    .monospace()
                                    .color(t.good),
                            )
                                .on_hover_text("hands-on task done");
                        }
                    });
                });
            })
            .response
            .interact(egui::Sense::click());

        if response.clicked() {
            app.selected_day = Some(day.id);
            app.study_view = StudyView::Day;
            app.sync_day_buffers();
        }
    }
}
