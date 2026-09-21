//! Application state, background-job plumbing and the eframe update loop.

use std::sync::mpsc::{channel, Receiver, Sender};

use egui_commonmark::CommonMarkCache;

use crate::db::{self, Db};
use crate::mentor::{self, CliConfig, JobResult};
use crate::models::*;
use crate::{quiz, srs, ui, verify};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Study,
    Reviews,
    Analytics,
    Roadmap,
}

impl Tab {
    fn key(&self) -> &'static str {
        match self {
            Tab::Study => "study",
            Tab::Reviews => "reviews",
            Tab::Analytics => "analytics",
            Tab::Roadmap => "roadmap",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "reviews" => Tab::Reviews,
            "analytics" => Tab::Analytics,
            "roadmap" => Tab::Roadmap,
            _ => Tab::Study,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudyView {
    Overview,
    Day,
    Weekly,
}

/// An in-progress quiz attempt.
pub struct QuizState {
    pub quiz_id: i64,
    pub plan_id: i64,
    pub title: String,
    pub questions: Vec<QuizQuestion>,
    pub chosen: Vec<Option<usize>>,
    pub submitted: bool,
    pub score: (i64, i64),
}

/// Editable copy of the settings, applied on Save.
pub struct SettingsForm {
    pub claude_path: String,
    pub prompt_flag: String,
    pub extra_args: String,
    pub git_path: String,
    pub target_hours: i64,
    pub minutes_per_day: i64,
    pub quiz_length: i64,
    pub heatmap_weeks: i64,
    pub unlock_threshold: i64,
}

pub struct AiMentorApp {
    pub db: Db,
    pub cfg: CliConfig,
    pub tx: Sender<JobResult>,
    pub rx: Receiver<JobResult>,
    pub jobs_in_flight: usize,
    pub status: String,
    pub error: Option<String>,

    pub stacks: Vec<TechStack>,
    pub active_stack: Option<i64>,
    pub plan: Option<Plan>,
    pub days: Vec<Day>,
    pub projects: Vec<WeeklyProject>,
    pub selected_day: Option<i64>,
    pub selected_week: i64,
    pub verifications: Vec<Verification>,

    pub tab: Tab,
    pub study_view: StudyView,

    pub edit_mode: bool,
    pub edit_buffer: String,
    pub notes_buffer: String,
    pub project_notes_buffer: String,
    pub minutes_input: i64,
    pub custom_subject: String,
    pub new_roadmap_name: String,
    pub github_input: String,
    pub issue_filter: String,
    /// Raw CLI output kept for manual repair when JSON parsing failed.
    pub failed_plan_raw: Option<(i64, String)>,

    pub due_cards: Vec<ReviewCard>,
    pub review_index: usize,
    pub review_revealed: bool,
    pub reviewed_today: i64,

    pub quiz: Option<QuizState>,

    pub show_settings: bool,
    pub settings_form: SettingsForm,

    pub md_cache: CommonMarkCache,
    last_size_save: f64,
}

impl AiMentorApp {
    pub fn new(db: Db) -> Self {
        let (tx, rx) = channel();
        let cfg = CliConfig {
            claude_path: db.setting_or("claude_cli_path", "claude"),
            prompt_flag: db.setting_or("claude_prompt_flag", "-p"),
            extra_args: db.setting_or("claude_extra_args", "--output-format text"),
            git_path: db.setting_or("git_path", "git"),
        };
        let settings_form = SettingsForm {
            claude_path: cfg.claude_path.clone(),
            prompt_flag: cfg.prompt_flag.clone(),
            extra_args: cfg.extra_args.clone(),
            git_path: cfg.git_path.clone(),
            target_hours: db.setting_i64("target_hours", 20),
            minutes_per_day: db.setting_i64("minutes_per_day", 60),
            quiz_length: db.setting_i64("quiz_length", 8),
            heatmap_weeks: db.setting_i64("heatmap_weeks", 30),
            unlock_threshold: db
                .setting_i64("roadmap_unlock_threshold", crate::roadmap::DEFAULT_UNLOCK_THRESHOLD),
        };
        let tab = Tab::parse(&db.setting_or("active_tab", "study"));
        let last_subject = db.get_setting("last_subject").and_then(|s| s.parse().ok());
        let stacks = db.all_stacks().unwrap_or_default();

        let mut app = Self {
            db,
            cfg,
            tx,
            rx,
            jobs_in_flight: 0,
            status: "Ready".to_string(),
            error: None,
            stacks,
            active_stack: last_subject,
            plan: None,
            days: Vec::new(),
            projects: Vec::new(),
            selected_day: None,
            selected_week: 1,
            verifications: Vec::new(),
            tab,
            study_view: StudyView::Overview,
            edit_mode: false,
            edit_buffer: String::new(),
            notes_buffer: String::new(),
            project_notes_buffer: String::new(),
            minutes_input: 60,
            custom_subject: String::new(),
            new_roadmap_name: String::new(),
            github_input: String::new(),
            issue_filter: "all".to_string(),
            failed_plan_raw: None,
            due_cards: Vec::new(),
            review_index: 0,
            review_revealed: false,
            reviewed_today: 0,
            quiz: None,
            show_settings: false,
            settings_form,
            md_cache: CommonMarkCache::default(),
            last_size_save: 0.0,
        };
        app.reload_subject();
        app.reload_due_cards();
        app
    }

    // -- data reloading ----------------------------------------------------

    pub fn reload_stacks(&mut self) {
        self.stacks = self.db.all_stacks().unwrap_or_default();
    }

    pub fn reload_subject(&mut self) {
        self.plan = None;
        self.days.clear();
        self.projects.clear();
        self.verifications.clear();
        let Some(stack_id) = self.active_stack else {
            return;
        };
        match self.db.plan_for_stack(stack_id) {
            Ok(Some(plan)) => {
                self.days = self.db.days_for_plan(plan.id).unwrap_or_default();
                self.projects = self.db.weekly_projects(plan.id).unwrap_or_default();
                self.plan = Some(plan);
            }
            Ok(None) => {}
            Err(e) => self.error = Some(e.to_string()),
        }
        if self.selected_day.is_none() {
            self.selected_day = self.days.first().map(|d| d.id);
        }
        self.sync_day_buffers();
        self.reload_verifications();
    }

    pub fn reload_due_cards(&mut self) {
        self.due_cards = self.db.due_cards(&db::today()).unwrap_or_default();
        self.review_index = 0;
        self.review_revealed = false;
    }

    pub fn reload_verifications(&mut self) {
        self.verifications = self
            .current_project()
            .map(|p| p.id)
            .and_then(|id| self.db.verifications(id).ok())
            .unwrap_or_default();
    }

    /// Refresh the editor buffers when the selected day changes.
    pub fn sync_day_buffers(&mut self) {
        if let Some(day) = self.current_day().cloned() {
            self.edit_buffer = day.content_md;
            self.notes_buffer = day.notes;
            self.selected_week = day.week_number;
        }
        self.edit_mode = false;
        if let Some(project) = self.current_project().cloned() {
            self.github_input = project.github_url;
            self.project_notes_buffer = project.notes;
        }
    }

    pub fn current_day(&self) -> Option<&Day> {
        let id = self.selected_day?;
        self.days.iter().find(|d| d.id == id)
    }

    pub fn current_project(&self) -> Option<&WeeklyProject> {
        self.projects
            .iter()
            .find(|p| p.week_number == self.selected_week)
    }

    pub fn active_stack_name(&self) -> String {
        self.active_stack
            .and_then(|id| self.stacks.iter().find(|s| s.id == id))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "(no subject)".to_string())
    }

    pub fn select_subject(&mut self, stack_id: i64) {
        self.active_stack = Some(stack_id);
        self.selected_day = None;
        self.quiz = None;
        let _ = self.db.set_setting("last_subject", &stack_id.to_string());
        self.reload_subject();
    }

    pub fn logged_minutes(&self) -> i64 {
        self.plan
            .as_ref()
            .and_then(|p| self.db.logged_minutes_for_plan(p.id).ok())
            .unwrap_or(0)
    }

    // -- background jobs ---------------------------------------------------

    pub fn generate_plans_for_selected(&mut self) {
        let target_hours = self.settings_form.target_hours;
        let minutes_per_day = self.settings_form.minutes_per_day;
        let selected: Vec<(i64, String)> = self
            .stacks
            .iter()
            .filter(|s| s.selected)
            .map(|s| (s.id, s.name.clone()))
            .collect();

        if selected.is_empty() {
            self.status = "Tick at least one subject first.".to_string();
            return;
        }

        let mut launched = 0;
        for (id, name) in selected {
            if matches!(self.db.plan_for_stack(id), Ok(Some(_))) {
                continue; // keep existing plans; use Regenerate to replace one
            }
            mentor::spawn_plan(
                self.tx.clone(),
                self.cfg.clone(),
                id,
                name,
                target_hours,
                minutes_per_day,
            );
            self.jobs_in_flight += 1;
            launched += 1;
        }
        self.status = if launched == 0 {
            "Every selected subject already has a plan.".to_string()
        } else {
            format!("Generating {launched} plan(s) with the Claude CLI…")
        };
    }

    pub fn regenerate_plan(&mut self, stack_id: i64, tech: String) {
        mentor::spawn_plan(
            self.tx.clone(),
            self.cfg.clone(),
            stack_id,
            tech.clone(),
            self.settings_form.target_hours,
            self.settings_form.minutes_per_day,
        );
        self.jobs_in_flight += 1;
        self.status = format!("Regenerating the {tech} plan…");
    }

    pub fn fetch_day_content(&mut self, day_id: i64) {
        let Some(day) = self.days.iter().find(|d| d.id == day_id).cloned() else {
            return;
        };
        let tech = self.active_stack_name();
        let prompt = mentor::prompt_day(&tech, &day);
        mentor::spawn_day_content(self.tx.clone(), self.cfg.clone(), day_id, prompt);
        self.jobs_in_flight += 1;
        self.status = format!("Fetching Day {} content…", day.day_number);
    }

    pub fn start_quiz(&mut self, scope: &str, day_id: Option<i64>, week: Option<i64>) {
        let Some(plan) = self.plan.as_ref().map(|p| p.id) else {
            return;
        };
        // Reuse a quiz that already exists for this scope.
        if let Ok(Some(existing)) = self.db.latest_quiz(plan, scope, day_id, week) {
            self.open_quiz(existing.id, plan);
            return;
        }
        self.generate_quiz(scope, day_id, week);
    }

    pub fn generate_quiz(&mut self, scope: &str, day_id: Option<i64>, week: Option<i64>) {
        let Some(plan) = self.plan.as_ref().map(|p| p.id) else {
            return;
        };
        let tech = self.active_stack_name();
        let scope_content = match (scope, day_id, week) {
            ("day", Some(id), _) => match self.days.iter().find(|d| d.id == id) {
                Some(day) => quiz::scope_content_day(day),
                None => return,
            },
            ("week", _, Some(w)) => {
                let project = self.projects.iter().find(|p| p.week_number == w);
                quiz::scope_content_week(&self.days, w, project)
            }
            _ => return,
        };
        let prompt = mentor::prompt_quiz(&tech, &scope_content, self.settings_form.quiz_length);
        mentor::spawn_quiz(
            self.tx.clone(),
            self.cfg.clone(),
            plan,
            scope.to_string(),
            day_id,
            week,
            prompt,
        );
        self.jobs_in_flight += 1;
        self.status = "Generating a recall quiz…".to_string();
    }

    pub fn open_quiz(&mut self, quiz_id: i64, plan_id: i64) {
        let questions = self.db.quiz_questions(quiz_id).unwrap_or_default();
        if questions.is_empty() {
            self.error = Some("That quiz has no usable questions.".to_string());
            return;
        }
        self.quiz = Some(QuizState {
            quiz_id,
            plan_id,
            title: format!("{} quiz", self.active_stack_name()),
            chosen: vec![None; questions.len()],
            questions,
            submitted: false,
            score: (0, 0),
        });
    }

    pub fn submit_quiz(&mut self) {
        let Some(state) = self.quiz.as_mut() else {
            return;
        };
        match quiz::score_attempt(
            &self.db,
            state.quiz_id,
            state.plan_id,
            &state.questions,
            &state.chosen,
        ) {
            Ok((correct, total)) => {
                state.submitted = true;
                state.score = (correct, total);
                self.status = format!("Quiz scored {correct}/{total}.");
                self.reload_due_cards();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn verify_project(&mut self, project_id: i64) {
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return;
        };
        if self.github_input.trim().is_empty() {
            self.status = "Enter the GitHub URL of your project repo first.".to_string();
            return;
        }
        let _ = self
            .db
            .set_project_github(project_id, self.github_input.trim());
        let req = verify::VerifyRequest {
            weekly_project_id: project_id,
            github_url: self.github_input.trim().to_string(),
            week_subskills: mentor::week_subskills(&self.days, project.week_number),
            description: format!("{} - {}", project.title, project.description),
            acceptance: project.acceptance.join("; "),
        };
        verify::spawn_verify(self.tx.clone(), self.cfg.clone(), req);
        self.jobs_in_flight += 1;
        self.status = "Cloning the repo and reviewing it with the Claude CLI…".to_string();
        self.reload_subject();
    }

    pub fn grade_current_card(&mut self, grade: i64) {
        let Some(card) = self.due_cards.get(self.review_index).cloned() else {
            return;
        };
        let mut card = card;
        srs::schedule(&mut card, grade, srs::today());
        if let Err(e) = self.db.save_card_schedule(&card) {
            self.error = Some(e.to_string());
            return;
        }
        self.reviewed_today += 1;
        self.review_revealed = false;
        self.review_index += 1;
        if self.review_index >= self.due_cards.len() {
            self.reload_due_cards();
            let remaining = self.due_cards.len();
            self.status = if remaining == 0 {
                "All reviews done for today.".to_string()
            } else {
                format!("{remaining} card(s) still due.")
            };
        }
    }

    /// Import plan JSON the user pasted or fixed by hand.
    pub fn import_plan_json(&mut self, stack_id: i64, raw: &str) {
        let parsed = mentor::extract_json(raw)
            .ok_or_else(|| "no JSON object found".to_string())
            .and_then(|slice| {
                serde_json::from_str::<PlanJson>(slice).map_err(|e| e.to_string())
            });
        match parsed {
            Ok(mut plan) => {
                mentor::enforce_ramp(&mut plan);
                self.store_plan(stack_id, &plan, raw);
                self.failed_plan_raw = None;
            }
            Err(e) => self.error = Some(format!("Could not parse that JSON: {e}")),
        }
    }

    fn store_plan(&mut self, stack_id: i64, plan: &PlanJson, raw: &str) {
        let target_hours = self.settings_form.target_hours;
        let minutes_per_day = self.settings_form.minutes_per_day;
        match self
            .db
            .save_plan(stack_id, target_hours, minutes_per_day, plan, raw)
        {
            Ok(_) => {
                self.status = format!("Plan saved: {} days.", plan.days.len());
                if self.active_stack.is_none() {
                    self.active_stack = Some(stack_id);
                }
                if self.active_stack == Some(stack_id) {
                    self.selected_day = None;
                    self.reload_subject();
                }
                self.reload_due_cards();
            }
            Err(e) => self.error = Some(format!("Could not save the plan: {e}")),
        }
    }

    // -- job results -------------------------------------------------------

    fn drain_jobs(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.jobs_in_flight = self.jobs_in_flight.saturating_sub(1);
            match result {
                JobResult::Plan {
                    stack_id,
                    tech,
                    outcome,
                    raw,
                } => match outcome {
                    Ok(plan) => {
                        self.status = format!("{tech}: plan ready.");
                        self.store_plan(stack_id, &plan, &raw);
                    }
                    Err(e) => {
                        self.error = Some(format!("{tech}: {e}"));
                        self.status = format!("{tech}: plan generation failed.");
                        self.failed_plan_raw = Some((stack_id, raw));
                    }
                },
                JobResult::DayContent { day_id, outcome } => match outcome {
                    Ok(md) => {
                        if let Err(e) = self.db.set_day_content(day_id, &md, false) {
                            self.error = Some(e.to_string());
                        } else {
                            self.status = "Day content fetched.".to_string();
                            self.reload_subject();
                        }
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.status = "Content fetch failed - you can still write it by hand."
                            .to_string();
                    }
                },
                JobResult::Quiz {
                    plan_id,
                    scope,
                    day_id,
                    week_number,
                    outcome,
                } => match outcome {
                    Ok(q) => {
                        match self.db.save_quiz(plan_id, &scope, day_id, week_number, &q) {
                            Ok(quiz_id) => {
                                self.status = format!("Quiz ready: {} questions.", q.questions.len());
                                self.open_quiz(quiz_id, plan_id);
                            }
                            Err(e) => self.error = Some(e.to_string()),
                        }
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.status = "Quiz generation failed.".to_string();
                    }
                },
                JobResult::Verify {
                    weekly_project_id,
                    github_url,
                    commit_sha,
                    report_md,
                    outcome,
                } => {
                    let (verdict, score, issues) = match &outcome {
                        Ok(v) => (v.verdict.clone(), v.score, v.issues.clone()),
                        Err(_) => ("unknown".to_string(), 0, Vec::new()),
                    };
                    if let Err(e) = self.db.insert_verification(
                        weekly_project_id,
                        &github_url,
                        &commit_sha,
                        &verdict,
                        score,
                        &report_md,
                        &issues,
                    ) {
                        self.error = Some(e.to_string());
                    }
                    match outcome {
                        Ok(_) => self.status = format!("Review complete: {verdict} ({score}/100)."),
                        Err(e) => {
                            self.status = "Review finished with problems.".to_string();
                            self.error = Some(e);
                        }
                    }
                    self.reload_verifications();
                }
            }
        }
    }

    // -- settings ----------------------------------------------------------

    pub fn apply_settings(&mut self) {
        let f = &self.settings_form;
        let pairs = [
            ("claude_cli_path", f.claude_path.clone()),
            ("claude_prompt_flag", f.prompt_flag.clone()),
            ("claude_extra_args", f.extra_args.clone()),
            ("git_path", f.git_path.clone()),
            ("target_hours", f.target_hours.to_string()),
            ("minutes_per_day", f.minutes_per_day.to_string()),
            ("quiz_length", f.quiz_length.to_string()),
            ("heatmap_weeks", f.heatmap_weeks.to_string()),
            ("roadmap_unlock_threshold", f.unlock_threshold.to_string()),
        ];
        for (key, value) in pairs {
            if let Err(e) = self.db.set_setting(key, &value) {
                self.error = Some(e.to_string());
                return;
            }
        }
        self.cfg = CliConfig {
            claude_path: f.claude_path.clone(),
            prompt_flag: f.prompt_flag.clone(),
            extra_args: f.extra_args.clone(),
            git_path: f.git_path.clone(),
        };
        self.status = "Settings saved.".to_string();
    }

    fn persist_window_size(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.last_size_save < 3.0 {
            return;
        }
        self.last_size_save = now;
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            let _ = self
                .db
                .set_setting("window_w", &(rect.width().round() as i64).to_string());
            let _ = self
                .db
                .set_setting("window_h", &(rect.height().round() as i64).to_string());
        }
    }
}

impl eframe::App for AiMentorApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.drain_jobs();
        if self.jobs_in_flight > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        self.persist_window_size(ctx);

        egui::Panel::top("top_bar").show(root, |ui| {
            ui.horizontal(|ui| {
                ui.heading("AI Mentor");
                ui.separator();
                let mut tab = self.tab;
                ui.selectable_value(&mut tab, Tab::Study, "Study");
                ui.selectable_value(&mut tab, Tab::Reviews, "Reviews");
                ui.selectable_value(&mut tab, Tab::Analytics, "Analytics");
                ui.selectable_value(&mut tab, Tab::Roadmap, "Roadmap");
                if tab != self.tab {
                    self.tab = tab;
                    let _ = self.db.set_setting("active_tab", tab.key());
                    if tab == Tab::Reviews {
                        self.reload_due_cards();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Settings").clicked() {
                        self.show_settings = true;
                    }
                });
            });
        });

        egui::Panel::bottom("status_bar").show(root, |ui| {
            ui.horizontal(|ui| {
                if self.jobs_in_flight > 0 {
                    ui.spinner();
                    ui.label(format!("{} job(s) running", self.jobs_in_flight));
                    ui.separator();
                }
                ui.label(&self.status);
                if let Some(err) = self.error.clone() {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
                    if ui.small_button("dismiss").clicked() {
                        self.error = None;
                    }
                }
            });
        });

        egui::Panel::left("subjects")
            .resizable(true)
            .default_size(290.0)
            .show(root, |ui| {
                ui::subjects::show(self, ui);
            });

        egui::CentralPanel::default().show(root, |ui| match self.tab {
            Tab::Study => ui::study::show(self, ui),
            Tab::Reviews => ui::reviews::show(self, ui),
            Tab::Analytics => ui::analytics::show(self, ui),
            Tab::Roadmap => ui::roadmap::show(self, ui),
        });

        ui::settings::show(self, ctx);
        ui::quiz_window::show(self, ctx);
    }
}

/// Colour for a 1-5 difficulty badge.
pub fn difficulty_color(difficulty: i64) -> egui::Color32 {
    match difficulty {
        1 => egui::Color32::from_rgb(76, 160, 106),
        2 => egui::Color32::from_rgb(120, 160, 70),
        3 => egui::Color32::from_rgb(196, 160, 60),
        4 => egui::Color32::from_rgb(214, 120, 60),
        _ => egui::Color32::from_rgb(200, 70, 70),
    }
}

pub fn status_color(status: DayStatus) -> egui::Color32 {
    match status {
        DayStatus::NotStarted => egui::Color32::from_gray(120),
        DayStatus::InProgress => egui::Color32::from_rgb(90, 150, 210),
        DayStatus::Done => egui::Color32::from_rgb(76, 160, 106),
    }
}
