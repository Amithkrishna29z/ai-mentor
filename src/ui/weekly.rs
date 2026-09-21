//! Study > Weekly project: the applied build, plus GitHub verification by the CLI.

use egui_commonmark::CommonMarkViewer;

use crate::app::AiMentorApp;
use crate::models::{Issue, Verification};

pub fn show(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    if app.projects.is_empty() {
        ui.label("This plan has no weekly projects.");
        return;
    }

    week_picker(app, ui);
    ui.separator();

    let Some(project) = app.current_project().cloned() else {
        ui.label("No project for that week.");
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt("weekly_scroll")
        .show(ui, |ui| {
            ui.heading(format!("Week {} — {}", project.week_number, project.title));
            ui.label(&project.description);

            if !project.acceptance.is_empty() {
                ui.add_space(6.0);
                ui.strong("Acceptance criteria");
                for criterion in &project.acceptance {
                    ui.label(format!("☐ {criterion}"));
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Status:");
                for option in ["not_started", "in_progress", "done"] {
                    let selected = project.status == option;
                    if ui.selectable_label(selected, option.replace('_', " ")).clicked() {
                        if let Err(e) = app.db.set_project_status(project.id, option) {
                            app.error = Some(e.to_string());
                        }
                        app.reload_subject();
                    }
                }
                ui.separator();
                if ui.button("Take weekly quiz").clicked() {
                    app.start_quiz("week", None, Some(project.week_number));
                }
                if ui.button("New weekly quiz").clicked() {
                    app.generate_quiz("week", None, Some(project.week_number));
                }
            });

            ui.add_space(10.0);
            verify_panel(app, ui, &project);
            ui.add_space(10.0);
            results_panel(app, ui);
            ui.add_space(10.0);

            egui::CollapsingHeader::new("Project notes").show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut app.project_notes_buffer)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4),
                );
                if ui.button("Save notes").clicked() {
                    let notes = app.project_notes_buffer.clone();
                    if let Err(e) = app.db.set_project_notes(project.id, &notes) {
                        app.error = Some(e.to_string());
                    } else {
                        app.status = "Project notes saved.".to_string();
                        app.reload_subject();
                    }
                }
            });
        });
}

fn week_picker(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let weeks: Vec<i64> = app.projects.iter().map(|p| p.week_number).collect();
    ui.horizontal(|ui| {
        ui.label("Week:");
        for week in weeks {
            if ui
                .selectable_label(app.selected_week == week, week.to_string())
                .clicked()
            {
                app.selected_week = week;
                app.sync_day_buffers();
                app.reload_verifications();
            }
        }
    });
}

fn verify_panel(app: &mut AiMentorApp, ui: &mut egui::Ui, project: &crate::models::WeeklyProject) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong("Verify your build");
        ui.label(
            egui::RichText::new(
                "Push the project to GitHub, then let the reviewer read the whole repo against \
                 this week's subskills. Requires git on PATH.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.github_input)
                    .hint_text("https://github.com/you/your-project")
                    .desired_width(380.0),
            );
            if ui.button("Verify").clicked() {
                app.verify_project(project.id);
            }
            if ui.button("Save URL").clicked() {
                let url = app.github_input.trim().to_string();
                if let Err(e) = app.db.set_project_github(project.id, &url) {
                    app.error = Some(e.to_string());
                } else {
                    app.status = "Repo URL saved.".to_string();
                    app.reload_subject();
                }
            }
        });
    });
}

fn results_panel(app: &mut AiMentorApp, ui: &mut egui::Ui) {
    let runs = app.verifications.clone();
    if runs.is_empty() {
        ui.label(egui::RichText::new("No verification runs yet.").weak());
        return;
    }

    let latest = runs[0].clone();
    ui.horizontal(|ui| {
        ui.strong("Latest review");
        ui.label(
            egui::RichText::new(format!(" {} ", latest.verdict))
                .color(egui::Color32::WHITE)
                .background_color(verdict_color(&latest.verdict)),
        );
        ui.label(format!("score {}/100", latest.score));
        if !latest.commit_sha.is_empty() {
            ui.label(
                egui::RichText::new(format!("@{}", &latest.commit_sha[..latest.commit_sha.len().min(8)]))
                    .small()
                    .weak(),
            );
        }
        if latest.verdict == "pass" {
            if let Some(project_id) = app.current_project().map(|p| p.id) {
                if ui.button("Mark project done").clicked() {
                    if let Err(e) = app.db.set_project_status(project_id, "done") {
                        app.error = Some(e.to_string());
                    }
                    app.reload_subject();
                }
            }
        }
    });

    issue_list(app, ui, &latest.issues);

    egui::CollapsingHeader::new("Full report")
        .default_open(true)
        .show(ui, |ui| {
            let report = latest.report_md.clone();
            if report.trim().is_empty() {
                ui.label("(empty report)");
            } else {
                CommonMarkViewer::new().show(ui, &mut app.md_cache, &report);
            }
        });

    if runs.len() > 1 {
        egui::CollapsingHeader::new(format!("History ({} earlier run(s))", runs.len() - 1)).show(
            ui,
            |ui| {
                for run in runs.iter().skip(1) {
                    history_row(ui, run);
                }
            },
        );
    }
}

fn history_row(ui: &mut egui::Ui, run: &Verification) {
    egui::CollapsingHeader::new(format!(
        "{} · {} · {}/100",
        run.created_at.split('T').next().unwrap_or(&run.created_at),
        run.verdict,
        run.score
    ))
    .id_salt(run.id)
    .show(ui, |ui| {
        for issue in &run.issues {
            ui.label(format!(
                "[{}] {}:{} — {}",
                issue.severity,
                issue.file,
                issue.line.unwrap_or(0),
                issue.issue
            ));
        }
    });
}

fn issue_list(app: &mut AiMentorApp, ui: &mut egui::Ui, issues: &[Issue]) {
    ui.horizontal(|ui| {
        ui.label("Issues:");
        for filter in ["all", "high", "med", "low"] {
            if ui
                .selectable_label(app.issue_filter == filter, filter)
                .clicked()
            {
                app.issue_filter = filter.to_string();
            }
        }
    });

    let filtered: Vec<&Issue> = issues
        .iter()
        .filter(|i| app.issue_filter == "all" || i.severity.eq_ignore_ascii_case(&app.issue_filter))
        .collect();

    if filtered.is_empty() {
        ui.label(egui::RichText::new("Nothing at this severity.").weak());
        return;
    }

    for issue in filtered {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(" {} ", issue.severity))
                        .small()
                        .color(egui::Color32::WHITE)
                        .background_color(severity_color(&issue.severity)),
                );
                let location = match issue.line {
                    Some(line) if !issue.file.is_empty() => format!("{}:{}", issue.file, line),
                    _ => issue.file.clone(),
                };
                if !location.is_empty() {
                    ui.label(egui::RichText::new(location).monospace().small());
                }
            });
            ui.label(&issue.issue);
            if !issue.suggestion.is_empty() {
                ui.label(egui::RichText::new(format!("→ {}", issue.suggestion)).weak());
            }
        });
    }
}

fn verdict_color(verdict: &str) -> egui::Color32 {
    match verdict {
        "pass" => egui::Color32::from_rgb(76, 160, 106),
        "needs_work" => egui::Color32::from_rgb(214, 150, 60),
        "fail" => egui::Color32::from_rgb(200, 70, 70),
        _ => egui::Color32::from_gray(120),
    }
}

fn severity_color(severity: &str) -> egui::Color32 {
    match severity.to_ascii_lowercase().as_str() {
        "high" => egui::Color32::from_rgb(200, 70, 70),
        "med" | "medium" => egui::Color32::from_rgb(214, 150, 60),
        _ => egui::Color32::from_rgb(110, 140, 180),
    }
}
