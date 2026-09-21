//! Study tab: subject picker, view switcher, and manual plan-JSON repair.

use crate::app::{AiMentorApp, StudyView};
use crate::ui::{day_view, overview, weekly};

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    header(app, ui);
    ui.separator();

    if let Some((stack_id, raw)) = app.failed_plan_raw.clone() {
        repair_box(app, ui, stack_id, raw);
        return;
    }

    if app.plan.is_none() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label("No plan for this subject yet.");
            ui.label("Tick it in the left panel, then press \"Generate plan for selected\".");
        });
        return;
    }

    match app.study_view {
        StudyView::Overview => overview::show(app, ui),
        StudyView::Day => day_view::show(app, ui),
        StudyView::Weekly => weekly::show(app, ui),
    }
}

fn header(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let current = app.active_stack_name();
        let options: Vec<(i64, String)> = app
            .stacks
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect();
        egui::ComboBox::from_id_salt("subject_dropdown")
            .selected_text(current)
            .width(240.0)
            .show_ui(ui, |ui| {
                for (id, name) in options {
                    if ui
                        .selectable_label(app.active_stack == Some(id), name)
                        .clicked()
                    {
                        app.select_subject(id);
                    }
                }
            });

        ui.separator();
        let mut view = app.study_view;
        ui.selectable_value(&mut view, StudyView::Overview, "Overview");
        ui.selectable_value(&mut view, StudyView::Day, "Day");
        ui.selectable_value(&mut view, StudyView::Weekly, "Weekly project");
        app.study_view = view;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.plan.is_some() && ui.button("Regenerate plan").clicked() {
                if let Some(stack_id) = app.active_stack {
                    let tech = app.active_stack_name();
                    app.regenerate_plan(stack_id, tech);
                }
            }
        });
    });
}

/// Shown when the CLI returned something unparseable: the raw output stays
/// editable so the plan can be salvaged by hand.
fn repair_box(app: &mut AiMentorApp, ui: &mut egui::Ui, stack_id: i64, raw: String) {
    ui.colored_label(
        egui::Color32::from_rgb(220, 150, 60),
        "The CLI output could not be parsed as a plan. Fix the JSON below and import it, \
         or dismiss and try again.",
    );
    let mut buffer = raw;
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut buffer)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(24),
            );
        });
    app.failed_plan_raw = Some((stack_id, buffer.clone()));

    ui.horizontal(|ui| {
        if ui.button("Import JSON").clicked() {
            app.import_plan_json(stack_id, &buffer);
        }
        if ui.button("Dismiss").clicked() {
            app.failed_plan_raw = None;
        }
    });
}
