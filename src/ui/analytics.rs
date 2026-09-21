//! Analytics tab: KPI cards, the activity heatmap and the weekly-minutes trend.

use egui_plot::{Bar, BarChart, Plot};

use crate::analytics::{self, Heatmap};
use crate::app::AiMentorApp;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let today = crate::srs::today();
    let weeks = app.settings_form.heatmap_weeks;

    egui::ScrollArea::vertical()
        .id_salt("analytics_scroll")
        .show(ui, |ui| {
            ui.heading("Analytics");
            ui.add_space(6.0);

            match analytics::stats(&app.db, today) {
                Ok(stats) => {
                    ui.horizontal_wrapped(|ui| {
                        kpi(ui, "Current streak", &format!("{} d", stats.current_streak));
                        kpi(ui, "Longest streak", &format!("{} d", stats.longest_streak));
                        kpi(
                            ui,
                            "Total practised",
                            &format!("{:.1} h", stats.total_minutes as f32 / 60.0),
                        );
                        kpi(ui, "Days completed", &stats.days_completed.to_string());
                        kpi(
                            ui,
                            "Avg quiz score",
                            &match stats.avg_quiz_score {
                                Some(v) => format!("{v:.0}%"),
                                None => "—".to_string(),
                            },
                        );
                        kpi(
                            ui,
                            "Cards due",
                            &format!("{} / {}", stats.cards_due, stats.cards_total),
                        );
                        kpi(
                            ui,
                            "Peak difficulty",
                            &format!("{}/5", stats.highest_difficulty),
                        );
                    });
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 90), e.to_string());
                }
            }

            ui.add_space(14.0);
            ui.strong("Activity");
            if let Ok(map) = analytics::heatmap(&app.db, today, weeks) {
                heatmap_widget(ui, &map);
            }

            ui.add_space(14.0);
            ui.strong("Minutes per week");
            if let Ok(series) = analytics::weekly_minutes(&app.db, today, weeks.min(26)) {
                trend(ui, &series);
            }

            ui.add_space(14.0);
            ui.strong("Subjects");
            subject_table(app, ui);
        });
}

fn kpi(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(112.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).small().weak());
            ui.label(egui::RichText::new(value).size(19.0).strong());
        });
    });
}

/// GitHub-style calendar grid, drawn directly with the painter.
fn heatmap_widget(ui: &mut egui::Ui, map: &Heatmap) {
    const CELL: f32 = 12.0;
    const GAP: f32 = 3.0;
    const LABEL_W: f32 = 30.0;
    const LABEL_H: f32 = 14.0;

    let width = LABEL_W + map.weeks.len() as f32 * (CELL + GAP);
    let height = LABEL_H + 7.0 * (CELL + GAP);
    let (response, painter) =
        ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
    let origin = response.rect.min;
    let base = ui.visuals().faint_bg_color;
    let accent = egui::Color32::from_rgb(56, 160, 105);

    // Weekday gutter (every other row, as GitHub does).
    for row in [1usize, 3, 5] {
        painter.text(
            origin + egui::vec2(0.0, LABEL_H + row as f32 * (CELL + GAP)),
            egui::Align2::LEFT_TOP,
            analytics::weekday_label(row),
            egui::FontId::proportional(9.0),
            ui.visuals().weak_text_color(),
        );
    }

    let mut last_month = String::new();
    let mut hovered: Option<String> = None;
    for (col, week) in map.weeks.iter().enumerate() {
        if let Some(Some(first)) = week.first() {
            let month = analytics::month_label(first.date);
            if month != last_month {
                painter.text(
                    origin + egui::vec2(LABEL_W + col as f32 * (CELL + GAP), 0.0),
                    egui::Align2::LEFT_TOP,
                    &month,
                    egui::FontId::proportional(9.0),
                    ui.visuals().weak_text_color(),
                );
                last_month = month;
            }
        }

        for (row, cell) in week.iter().enumerate() {
            let Some(cell) = cell else { continue };
            let pos = origin
                + egui::vec2(
                    LABEL_W + col as f32 * (CELL + GAP),
                    LABEL_H + row as f32 * (CELL + GAP),
                );
            let rect = egui::Rect::from_min_size(pos, egui::vec2(CELL, CELL));
            let color = if cell.minutes == 0 || map.max_minutes == 0 {
                base
            } else {
                // Four visible steps, like the GitHub palette.
                let ratio = cell.minutes as f32 / map.max_minutes as f32;
                let step = ((ratio * 4.0).ceil() as u8).clamp(1, 4);
                accent.gamma_multiply(0.25 * step as f32)
            };
            painter.rect_filled(rect, 2.0, color);

            if let Some(pointer) = response.hover_pos() {
                if rect.contains(pointer) {
                    hovered = Some(format!("{}: {} min", cell.date, cell.minutes));
                }
            }
        }
    }

    if let Some(text) = hovered {
        response.show_tooltip_text(text);
    }
}

fn trend(ui: &mut egui::Ui, series: &[(String, i64)]) {
    let bars: Vec<Bar> = series
        .iter()
        .enumerate()
        .map(|(i, (label, minutes))| {
            Bar::new(i as f64, *minutes as f64).name(label.clone()).width(0.7)
        })
        .collect();

    Plot::new("weekly_minutes")
        .height(160.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_axes([false, true])
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(BarChart::new("Minutes", bars));
        });
}

fn subject_table(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let rows = app.db.subject_progress().unwrap_or_default();
    if rows.is_empty() {
        ui.label(egui::RichText::new("No plans generated yet.").weak());
        return;
    }
    egui::Grid::new("subject_progress")
        .num_columns(4)
        .striped(true)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Subject");
            ui.strong("Days done");
            ui.strong("Practised");
            ui.strong("Progress");
            ui.end_row();

            for (name, done, total, minutes, target_hours) in rows {
                ui.label(name);
                ui.label(format!("{done}/{total}"));
                ui.label(format!("{:.1} / {} h", minutes as f32 / 60.0, target_hours));
                let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).desired_width(140.0));
                ui.end_row();
            }
        });
}
