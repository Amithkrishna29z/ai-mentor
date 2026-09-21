//! Analytics tab: KPI cards, the activity heatmap and the weekly-minutes trend.

use egui_plot::{Bar, BarChart, Plot};

use crate::analytics::{self, Heatmap};
use crate::app::AiMentorApp;
use crate::ui::theme;

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let today = crate::srs::today();
    let weeks = app.settings_form.heatmap_weeks;

    egui::ScrollArea::vertical()
        .id_salt("analytics_scroll")
        .show(ui, |ui| {
            let t = theme::current(ui);
            ui.label(egui::RichText::new("Analytics").size(19.0).strong());
            ui.add_space(8.0);

            match analytics::stats(&app.db, today) {
                Ok(stats) => {
                    ui.horizontal_wrapped(|ui| {
                        theme::stat_tile(
                            ui,
                            "Current streak",
                            &format!("{} d", stats.current_streak),
                            Some(if stats.current_streak > 0 { t.accent } else { t.text_muted }),
                        );
                        theme::stat_tile(
                            ui,
                            "Longest streak",
                            &format!("{} d", stats.longest_streak),
                            None,
                        );
                        theme::stat_tile(
                            ui,
                            "Total practised",
                            &format!("{:.1} h", stats.total_minutes as f32 / 60.0),
                            None,
                        );
                        theme::stat_tile(
                            ui,
                            "Days completed",
                            &stats.days_completed.to_string(),
                            None,
                        );
                        theme::stat_tile(
                            ui,
                            "Avg quiz score",
                            &match stats.avg_quiz_score {
                                Some(v) => format!("{v:.0}%"),
                                None => "\u{2014}".to_string(),
                            },
                            None,
                        );
                        theme::stat_tile(
                            ui,
                            "Cards due",
                            &format!("{} / {}", stats.cards_due, stats.cards_total),
                            Some(if stats.cards_due > 0 { t.accent } else { t.text }),
                        );
                        theme::stat_tile(
                            ui,
                            "Peak difficulty",
                            &format!("{}/5", stats.highest_difficulty),
                            None,
                        );
                    });
                }
                Err(e) => {
                    ui.colored_label(t.critical, e.to_string());
                }
            }

            ui.add_space(12.0);
            theme::titled_card(ui, "Study activity", |ui| {
                if let Ok(map) = analytics::heatmap(&app.db, today, weeks) {
                    heatmap_widget(ui, &map);
                }
            });

            ui.add_space(12.0);
            theme::titled_card(ui, "Minutes per week", |ui| {
                if let Ok(series) = analytics::weekly_minutes(&app.db, today, weeks.min(26)) {
                    trend(ui, &series);
                }
            });

            ui.add_space(12.0);
            theme::titled_card(ui, "Subjects", |ui| subject_table(app, ui));
        });
}

/// GitHub-style calendar grid, drawn directly with the painter.
fn heatmap_widget(ui: &mut egui::Ui, map: &Heatmap) {
    const CELL: f32 = 12.0;
    const GAP: f32 = 3.0;
    const LABEL_W: f32 = 30.0;
    const LABEL_H: f32 = 14.0;

    let t = theme::current(ui);
    let width = LABEL_W + map.weeks.len() as f32 * (CELL + GAP);
    let height = LABEL_H + 7.0 * (CELL + GAP) + 20.0;
    let (response, painter) =
        ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
    let origin = response.rect.min;

    // Weekday gutter (every other row, as GitHub does).
    for row in [1usize, 3, 5] {
        painter.text(
            origin + egui::vec2(0.0, LABEL_H + row as f32 * (CELL + GAP)),
            egui::Align2::LEFT_TOP,
            analytics::weekday_label(row),
            egui::FontId::proportional(9.5),
            t.text_muted,
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
                    egui::FontId::proportional(9.5),
                    t.text_muted,
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
            // One-hue sequential ramp: four steps plus an empty step that
            // recedes toward the card surface.
            let step = if cell.minutes == 0 || map.max_minutes == 0 {
                0
            } else {
                let ratio = cell.minutes as f32 / map.max_minutes as f32;
                ((ratio * 4.0).ceil() as u8).clamp(1, 4)
            };
            painter.rect_filled(rect, 3.0, theme::heat_color(step, &t));

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

    // Legend: less -> more, in the same steps the cells use.
    let legend_y = origin.y + LABEL_H + 7.0 * (CELL + GAP) + 6.0;
    let mut x = origin.x + LABEL_W;
    painter.text(
        egui::pos2(x, legend_y),
        egui::Align2::LEFT_TOP,
        "Less",
        egui::FontId::proportional(9.5),
        t.text_muted,
    );
    x += 26.0;
    for step in 0..=4u8 {
        let rect = egui::Rect::from_min_size(egui::pos2(x, legend_y), egui::vec2(9.0, 9.0));
        painter.rect_filled(rect, 2.0, theme::heat_color(step, &t));
        x += 12.0;
    }
    painter.text(
        egui::pos2(x + 2.0, legend_y),
        egui::Align2::LEFT_TOP,
        "More",
        egui::FontId::proportional(9.5),
        t.text_muted,
    );
}

/// Single series, so no legend: the card title names it.
fn trend(ui: &mut egui::Ui, series: &[(String, i64)]) {
    let t = theme::current(ui);
    let peak = series.iter().map(|(_, m)| *m).max().unwrap_or(0);
    let bars: Vec<Bar> = series
        .iter()
        .enumerate()
        .map(|(i, (label, minutes))| {
            Bar::new(i as f64, *minutes as f64)
                .name(label.clone())
                .width(0.62)
                .fill(t.accent)
                .stroke(egui::Stroke::NONE)
        })
        .collect();

    Plot::new("weekly_minutes")
        .height(150.0)
        .width(ui.available_width())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .show_grid([false, true])
        .show_axes([false, true])
        // Minutes are never negative: anchor the axis at zero rather than
        // letting an empty series render a -10..10 range.
        .include_y(0.0)
        .include_y(peak.max(10) as f64)
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(BarChart::new("Minutes", bars));
        });
}

fn subject_table(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let t = theme::current(ui);
    let rows = app.db.subject_progress().unwrap_or_default();
    if rows.is_empty() {
        ui.label(egui::RichText::new("No plans generated yet.").color(t.text_muted));
        return;
    }
    egui::Grid::new("subject_progress")
        .num_columns(4)
        .striped(true)
        .spacing([18.0, 7.0])
        .show(ui, |ui| {
            for header in ["Subject", "Days done", "Practised", "Progress"] {
                ui.label(
                    egui::RichText::new(header.to_uppercase())
                        .size(10.0)
                        .color(t.text_muted)
                        .strong(),
                );
            }
            ui.end_row();

            for (name, done, total, minutes, target_hours) in rows {
                ui.label(name);
                ui.label(
                    egui::RichText::new(format!("{done}/{total}")).monospace().size(12.5),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{:.1} / {} h",
                        minutes as f32 / 60.0,
                        target_hours
                    ))
                    .monospace()
                    .size(12.5),
                );
                let fraction = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(150.0)
                        .desired_height(7.0)
                        .corner_radius(egui::CornerRadius::same(4))
                        .fill(t.accent),
                );
                ui.end_row();
            }
        });
}
