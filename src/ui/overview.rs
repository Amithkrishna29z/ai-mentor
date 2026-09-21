//! Study > Overview: subskills, the day grid, and 20-hour progress.

use crate::app::{difficulty_color, status_color, AiMentorApp, StudyView};
use crate::models::{DayStatus, PlanJson};

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
            ui.horizontal(|ui| {
                ui.heading(app.active_stack_name());
                ui.label(format!(
                    "· {} days · {} min/day · {} h target",
                    app.days.len(),
                    plan.minutes_per_day,
                    plan.target_hours
                ));
            });
            ui.add_space(6.0);

            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_height(16.0)
                    .text(format!(
                        "{:.1} / {} hours practised",
                        logged as f32 / 60.0,
                        plan.target_hours
                    )),
            );

            let reached = app
                .days
                .iter()
                .filter(|d| d.status == DayStatus::Done)
                .map(|d| d.difficulty)
                .max()
                .unwrap_or(0);
            ui.add(
                egui::ProgressBar::new(reached as f32 / 5.0)
                    .desired_height(12.0)
                    .text(format!("highest difficulty completed: {reached}/5")),
            );

            ui.add_space(10.0);
            subskills(app, ui, &plan);
            ui.add_space(10.0);
            ui.strong("Days");
            ui.label(
                egui::RichText::new(
                    "Difficulty never decreases: later days combine earlier subskills.",
                )
                .small()
                .weak(),
            );
            ui.add_space(4.0);
            day_grid(app, ui);
        });
}

fn subskills(app: &AiMentorApp, ui: &mut egui::Ui, plan: &crate::models::Plan) {
    let parsed: Option<PlanJson> = serde_json::from_str(&plan.plan_json)
        .ok()
        .or_else(|| crate::mentor::extract_json(&plan.plan_json).and_then(|j| serde_json::from_str(j).ok()));

    egui::CollapsingHeader::new("High-leverage subskills")
        .default_open(true)
        .show(ui, |ui| match parsed {
            Some(p) if !p.subskills.is_empty() => {
                let mut list = p.subskills;
                list.sort_by_key(|s| s.priority);
                for s in list {
                    ui.horizontal_top(|ui| {
                        ui.label(format!("{}.", s.priority));
                        ui.vertical(|ui| {
                            ui.strong(&s.name);
                            if !s.why.is_empty() {
                                ui.label(egui::RichText::new(&s.why).small().weak());
                            }
                        });
                    });
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
                    ui.label(format!("• {name}"));
                }
            }
        });
}

fn day_grid(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let days: Vec<_> = app.days.clone();
    let mut week = 0;

    for day in days {
        if day.week_number != week {
            week = day.week_number;
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("Week {week}")).strong());
        }
        ui.horizontal(|ui| {
            let badge = egui::RichText::new(format!(" {} ", day.difficulty))
                .color(egui::Color32::WHITE)
                .background_color(difficulty_color(day.difficulty))
                .monospace();
            ui.label(badge);
            ui.colored_label(status_color(day.status), "●")
                .on_hover_text(day.status.label());

            let selected = app.selected_day == Some(day.id);
            let label = format!("Day {} — {}", day.day_number, day.title);
            if ui.selectable_label(selected, label).clicked() {
                app.selected_day = Some(day.id);
                app.study_view = StudyView::Day;
                app.sync_day_buffers();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if day.hands_on_done {
                    ui.label(egui::RichText::new("hands-on ✔").small().weak());
                }
                ui.label(egui::RichText::new(format!("{} min", day.est_minutes)).small().weak());
            });
        });
    }
}
