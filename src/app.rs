//! Application state, background-job plumbing and the eframe update loop.

use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};

use egui_commonmark::CommonMarkCache;

use crate::db::{self, Db};
use crate::mentor::{self, CliConfig, JobResult};
use crate::models::*;
use crate::{job, quiz, roadmap, srs, timer, ui, verify};

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

/// A CLI job waiting its turn. Everything that shells out to the Claude CLI
/// goes through the queue, so a 20-day course fetches steadily instead of
/// launching twenty processes at once.
pub enum QueuedJob {
    Plan {
        request: mentor::PlanRequest,
    },
    JobSkills {
        description: String,
    },
    DayContent {
        day_id: i64,
        prompt: String,
    },
    Quiz {
        plan_id: i64,
        scope: String,
        day_id: Option<i64>,
        week_number: Option<i64>,
        prompt: String,
    },
    Verify {
        request: verify::VerifyRequest,
    },
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
    /// Pending CLI work and how much of it is running right now.
    pub queue: VecDeque<QueuedJob>,
    pub cli_active: usize,
    pub max_concurrent: usize,
    pub auto_fetch_lessons: bool,
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
    /// The job the learner is training for, and the posting box on the track.
    pub target_role: String,
    pub target_seniority: String,
    pub jd_input: String,
    pub reading_job: bool,
    pub new_roadmap_name: String,
    pub github_input: String,
    pub issue_filter: String,
    /// Raw CLI output kept for manual repair when JSON parsing failed.
    pub failed_plan_raw: Option<(i64, String)>,

    /// The active track, rebuilt when its data changes rather than every
    /// frame: `roadmap::progress` costs two queries per subject, and two
    /// views were each asking for it on every repaint.
    pub track: Option<roadmap::RoadmapProgress>,
    /// Subjects that already have a course, for the list markers.
    pub stacks_with_plans: HashSet<i64>,

    pub due_cards: Vec<ReviewCard>,
    pub review_index: usize,
    pub review_revealed: bool,
    pub reviewed_today: i64,

    pub quiz: Option<QuizState>,
    /// The running practice session, if one was started.
    pub timer: Option<timer::PracticeTimer>,

    pub show_settings: bool,
    pub settings_form: SettingsForm,

    pub md_cache: CommonMarkCache,
    /// A newer release found on GitHub, and the version installed this session.
    pub dark_mode: bool,
    pub theme_applied: bool,
    pub update_available: Option<crate::update::ReleaseInfo>,
    pub update_installed: Option<String>,
    pub checking_update: bool,
    pub testing_cli: bool,
    size_fitted: bool,
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
        let dark_mode = db.setting_or("dark_mode", "1") == "1";
        let max_concurrent = db.setting_i64("max_concurrent_cli", 1).clamp(1, 4) as usize;
        let auto_fetch_lessons = db.setting_or("auto_fetch_lessons", "1") == "1";
        let last_subject = db.get_setting("last_subject").and_then(|s| s.parse().ok());
        let target_role = db.setting_or("target_role", "");
        let target_seniority = db.setting_or("target_seniority", "");
        let stacks = db.all_stacks().unwrap_or_default();

        let mut app = Self {
            db,
            cfg,
            tx,
            rx,
            jobs_in_flight: 0,
            queue: VecDeque::new(),
            cli_active: 0,
            max_concurrent,
            auto_fetch_lessons,
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
            target_role,
            target_seniority,
            jd_input: String::new(),
            reading_job: false,
            new_roadmap_name: String::new(),
            github_input: String::new(),
            issue_filter: "all".to_string(),
            failed_plan_raw: None,
            track: None,
            stacks_with_plans: HashSet::new(),
            due_cards: Vec::new(),
            review_index: 0,
            review_revealed: false,
            reviewed_today: 0,
            quiz: None,
            timer: None,
            show_settings: false,
            settings_form,
            md_cache: CommonMarkCache::default(),
            dark_mode,
            theme_applied: false,
            update_available: None,
            update_installed: None,
            checking_update: false,
            testing_cli: false,
            size_fitted: false,
            last_size_save: 0.0,
        };
        app.reload_subject();
        app.reload_due_cards();
        if app.db.setting_or("check_updates_on_start", "1") == "1" {
            app.check_for_update();
        }
        app
    }

    // -- data reloading ----------------------------------------------------

    pub fn reload_stacks(&mut self) {
        self.stacks = self.db.all_stacks().unwrap_or_default();
        self.reload_track();
    }

    /// Recompute the cached track snapshot. Call after anything that moves a
    /// day's status, a plan, or the roadmap itself.
    pub fn reload_track(&mut self) {
        self.track = self
            .db
            .roadmaps()
            .ok()
            .and_then(|rs| rs.into_iter().find(|r| r.active))
            .and_then(|r| roadmap::progress(&self.db, r.id).ok());
        self.stacks_with_plans = self
            .db
            .stacks_with_plans()
            .unwrap_or_default()
            .into_iter()
            .collect();
    }

    pub fn reload_subject(&mut self) {
        self.plan = None;
        self.days.clear();
        self.projects.clear();
        self.verifications.clear();
        // The track does not depend on the selected subject, and this returns
        // early when nothing is selected - as on a fresh install.
        self.reload_track();
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

    // -- the CLI job queue -------------------------------------------------

    pub fn enqueue(&mut self, job: QueuedJob) {
        self.queue.push_back(job);
        self.pump_queue();
    }

    /// Start queued jobs until the concurrency cap is reached.
    pub fn pump_queue(&mut self) {
        while self.cli_active < self.max_concurrent {
            let Some(job) = self.queue.pop_front() else {
                return;
            };
            match job {
                QueuedJob::Plan { request } => {
                    mentor::spawn_plan(self.tx.clone(), self.cfg.clone(), request)
                }
                QueuedJob::JobSkills { description } => {
                    mentor::spawn_job_skills(self.tx.clone(), self.cfg.clone(), description)
                }
                QueuedJob::DayContent { day_id, prompt } => {
                    mentor::spawn_day_content(self.tx.clone(), self.cfg.clone(), day_id, prompt)
                }
                QueuedJob::Quiz {
                    plan_id,
                    scope,
                    day_id,
                    week_number,
                    prompt,
                } => mentor::spawn_quiz(
                    self.tx.clone(),
                    self.cfg.clone(),
                    plan_id,
                    scope,
                    day_id,
                    week_number,
                    prompt,
                ),
                QueuedJob::Verify { request } => {
                    verify::spawn_verify(self.tx.clone(), self.cfg.clone(), request)
                }
            }
            self.cli_active += 1;
            self.jobs_in_flight += 1;
        }
    }

    /// Drop everything still waiting. A job already running finishes.
    pub fn clear_queue(&mut self) {
        let dropped = self.queue.len();
        self.queue.clear();
        self.status = format!("Stopped: {dropped} queued job(s) dropped.");
    }

    /// Queue a lesson fetch for every day of a plan that has no content yet.
    fn enqueue_lessons_for_plan(&mut self, plan_id: i64, tech: &str) {
        let Ok(days) = self.db.days_for_plan(plan_id) else {
            return;
        };
        let mut queued = 0;
        for day in days {
            // Never clobber a lesson the learner has edited or already has.
            if day.content_edited || !day.content_md.trim().is_empty() {
                continue;
            }
            let prompt = mentor::prompt_day(tech, &day);
            self.queue.push_back(QueuedJob::DayContent {
                day_id: day.id,
                prompt,
            });
            queued += 1;
        }
        if queued > 0 {
            self.status = format!("{tech}: fetching {queued} lessons with the Claude CLI…");
            self.pump_queue();
        }
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
            let request = mentor::PlanRequest {
                builds_on: self.track_context(id),
                role: self.target_role.clone(),
                stack_id: id,
                tech: name,
                target_hours,
                minutes_per_day,
            };
            self.queue.push_back(QueuedJob::Plan { request });
            launched += 1;
        }
        self.pump_queue();
        self.status = if launched == 0 {
            "Every selected subject already has a plan.".to_string()
        } else {
            format!("Generating {launched} course(s) with the Claude CLI…")
        };
    }

    pub fn regenerate_plan(&mut self, stack_id: i64, tech: String) {
        let request = mentor::PlanRequest {
            builds_on: self.track_context(stack_id),
            role: self.target_role.clone(),
            stack_id,
            tech: tech.clone(),
            target_hours: self.settings_form.target_hours,
            minutes_per_day: self.settings_form.minutes_per_day,
        };
        self.enqueue(QueuedJob::Plan { request });
        self.status = format!("Generating the {tech} course…");
    }

    // -- the roadmap track -------------------------------------------------

    /// Subjects already cleared ahead of this one on the active track. A new
    /// course is told about them so it builds on that ground instead of
    /// starting the learner over.
    fn track_context(&self, stack_id: i64) -> Vec<String> {
        let Some(progress) = self.track.as_ref() else {
            return Vec::new();
        };
        match progress.index_of(stack_id) {
            Some(idx) => progress.cleared_before(idx),
            None => Vec::new(),
        }
    }

    /// The active roadmap's id, if there is one.
    fn active_roadmap(&self) -> Option<i64> {
        self.db
            .roadmaps()
            .ok()?
            .into_iter()
            .find(|r| r.active)
            .map(|r| r.id)
    }

    /// Ticking a subject puts it on the track; unticking takes it off. The
    /// checkbox and track membership are the same idea, so they stay in step -
    /// a topic picked up mid-study lands at the end of the track and inherits
    /// the chaining, rather than sitting outside the plan.
    pub fn set_on_track(&mut self, stack_id: i64, on: bool) {
        if let Err(e) = self.db.set_stack_selected(stack_id, on) {
            self.error = Some(e.to_string());
            return;
        }
        let Some(roadmap_id) = self.active_roadmap() else {
            self.reload_stacks();
            return;
        };
        let name = self
            .stacks
            .iter()
            .find(|s| s.id == stack_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        if on {
            let _ = self.db.add_roadmap_item(roadmap_id, stack_id);
            self.status = format!("{name} added to the end of your track.");
        } else {
            if let Ok(items) = self.db.roadmap_items(roadmap_id) {
                if let Some(item) = items.iter().find(|i| i.tech_stack_id == stack_id) {
                    let _ = self.db.remove_roadmap_item(item.id);
                }
            }
            // The course and its days survive; only the track entry goes.
            self.status = format!("{name} taken off your track.");
        }
        self.reload_stacks();
    }

    /// Read a pasted job posting and rebuild the track from what it asks for.
    pub fn read_job_description(&mut self) {
        let description = self.jd_input.trim().to_string();
        if description.len() < 40 {
            self.status = "Paste the job posting first.".to_string();
            return;
        }
        if self.reading_job {
            return;
        }
        self.reading_job = true;
        self.enqueue(QueuedJob::JobSkills { description });
        self.status = "Reading the posting for the skills it asks for…".to_string();
    }

    /// Turn an extracted posting into a track: match each skill onto a subject
    /// the app can teach, invent one where it cannot, and lay them out in the
    /// order the posting says to learn them.
    fn build_track_from_job(&mut self, spec: &JobSpecJson) {
        let role = spec.role.trim();
        if !role.is_empty() {
            self.target_role = role.to_string();
            let _ = self.db.set_setting("target_role", role);
        }
        self.target_seniority = spec.seniority.trim().to_string();
        let _ = self
            .db
            .set_setting("target_seniority", &self.target_seniority.clone());

        let known: Vec<String> = self.db
            .all_stacks()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.name)
            .collect();

        let mut skills = spec.skills.clone();
        skills.sort_by_key(|s| s.priority);

        let mut wanted: Vec<i64> = Vec::new();
        let mut matched = 0usize;
        let mut invented = 0usize;
        for skill in &skills {
            let stack_id = match job::match_subject(&skill.name, &known) {
                Some(name) => {
                    matched += 1;
                    self.db.stack_id_by_name(&name).ok().flatten()
                }
                None => {
                    let name = job::custom_subject_name(&skill.name);
                    if name.is_empty() {
                        continue;
                    }
                    invented += 1;
                    self.db.add_custom_stack(&name).ok()
                }
            };
            if let Some(id) = stack_id {
                if !wanted.contains(&id) {
                    wanted.push(id);
                }
            }
        }
        if wanted.is_empty() {
            self.error = Some("That posting did not name any studiable skills.".to_string());
            return;
        }

        // The posting defines its own track, so it gets its own roadmap rather
        // than overwriting the preset.
        let track_name = if role.is_empty() {
            "Job track".to_string()
        } else {
            format!("{role} (job track)")
        };
        let Ok(roadmap_id) = self.db.create_roadmap(&track_name) else {
            self.error = Some("Could not create the job track.".to_string());
            return;
        };
        let _ = self.db.clear_roadmap_items(roadmap_id);
        for id in &wanted {
            let _ = self.db.add_roadmap_item(roadmap_id, *id);
            let _ = self.db.set_stack_selected(*id, true);
        }
        let _ = self.db.set_active_roadmap(roadmap_id);

        self.reload_stacks();
        self.tab = Tab::Roadmap;
        self.status = format!(
            "{track_name}: {} subjects ({matched} known, {invented} added).",
            wanted.len()
        );
    }

    /// Move to whatever the track says comes next: select that subject, and
    /// generate its course if it does not have one yet.
    pub fn start_next_on_track(&mut self) {
        let Some(progress) = self.track.as_ref() else {
            self.status = "No roadmap is active.".to_string();
            return;
        };
        let Some(entry) = progress.current_entry() else {
            self.status = "Every subject on the track is cleared.".to_string();
            return;
        };
        let (stack_id, subject) = (entry.stack_id, entry.subject.clone());
        self.select_subject(stack_id);
        self.tab = Tab::Study;
        if matches!(self.db.plan_for_stack(stack_id), Ok(Some(_))) {
            self.status = format!("{subject}: picking up where the track left off.");
        } else {
            self.regenerate_plan(stack_id, subject);
        }
    }

    pub fn fetch_day_content(&mut self, day_id: i64) {
        let Some(day) = self.days.iter().find(|d| d.id == day_id).cloned() else {
            return;
        };
        let tech = self.active_stack_name();
        let prompt = mentor::prompt_day(&tech, &day);
        self.enqueue(QueuedJob::DayContent { day_id, prompt });
        self.status = format!("Fetching the Day {} lesson…", day.day_number);
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
        self.enqueue(QueuedJob::Quiz {
            plan_id: plan,
            scope: scope.to_string(),
            day_id,
            week_number: week,
            prompt,
        });
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
        self.enqueue(QueuedJob::Verify { request: req });
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

    // -- the practice timer ------------------------------------------------

    pub fn start_timer(&mut self, day_id: i64, now: f64) {
        self.timer = Some(timer::PracticeTimer::started(day_id, now));
        self.status = "Practice timer running.".to_string();
    }

    pub fn toggle_timer(&mut self, now: f64) {
        let Some(session) = self.timer.as_mut() else {
            return;
        };
        if session.is_running() {
            session.pause(now);
            self.status = "Practice timer paused.".to_string();
        } else {
            session.resume(now);
            self.status = "Practice timer running.".to_string();
        }
    }

    /// Stop the timer and log its minutes against the day it was started on,
    /// which is not necessarily the day on screen now.
    pub fn stop_timer(&mut self, now: f64) {
        let Some(session) = self.timer.take() else {
            return;
        };
        let minutes = session.minutes(now);
        if minutes < 1 {
            self.status = "Timer stopped - under a minute, so nothing was logged.".to_string();
            return;
        }
        if let Err(e) = self.db.log_minutes(session.day_id, minutes) {
            self.error = Some(e.to_string());
            return;
        }
        // Practising implies the day has started, exactly as a manual log does.
        if self
            .days
            .iter()
            .any(|d| d.id == session.day_id && d.status == DayStatus::NotStarted)
        {
            let _ = self.db.set_day_status(session.day_id, DayStatus::InProgress);
        }
        self.status = format!("Logged {minutes} min of practice.");
        self.reload_subject();
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
            Ok(plan_id) => {
                self.status = format!("Course saved: {} days.", plan.days.len());
                if self.active_stack.is_none() {
                    self.active_stack = Some(stack_id);
                }
                if self.active_stack == Some(stack_id) {
                    self.selected_day = None;
                    self.reload_subject();
                }
                self.reload_due_cards();
                // Pull the actual study material so the course is readable in
                // the app rather than a list of empty days.
                if self.auto_fetch_lessons {
                    let tech = self
                        .stacks
                        .iter()
                        .find(|s| s.id == stack_id)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| plan.tech.clone());
                    self.enqueue_lessons_for_plan(plan_id, &tech);
                }
            }
            Err(e) => self.error = Some(format!("Could not save the plan: {e}")),
        }
    }

    // -- job results -------------------------------------------------------

    fn drain_jobs(&mut self) {
        let mut finished_cli = 0usize;
        while let Ok(result) = self.rx.try_recv() {
            self.jobs_in_flight = self.jobs_in_flight.saturating_sub(1);
            if !matches!(
                result,
                JobResult::UpdateCheck { .. }
                    | JobResult::UpdateInstall { .. }
                    | JobResult::CliTest { .. }
            ) {
                finished_cli += 1;
            }
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
                            let left = self.queue.len();
                            self.status = if left > 0 {
                                format!("Lesson ready · {left} still queued")
                            } else {
                                "Lesson ready.".to_string()
                            };
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
                    let review = match &outcome {
                        Ok(v) => v.clone(),
                        Err(_) => VerificationJson {
                            verdict: "unknown".to_string(),
                            ..Default::default()
                        },
                    };
                    if let Err(e) = self.db.insert_verification(
                        weekly_project_id,
                        &github_url,
                        &commit_sha,
                        &report_md,
                        &review,
                    ) {
                        self.error = Some(e.to_string());
                    }
                    match outcome {
                        Ok(_) => {
                            self.status = format!(
                                "Review complete: {} ({}/100).",
                                review.verdict, review.score
                            )
                        }
                        Err(e) => {
                            self.status = "Review finished with problems.".to_string();
                            self.error = Some(e);
                        }
                    }
                    self.reload_verifications();
                }
                JobResult::UpdateCheck { outcome } => {
                    self.checking_update = false;
                    match outcome {
                        Ok(Some(release)) => {
                            self.status = format!("Version {} is available.", release.version);
                            self.update_available = Some(release);
                        }
                        Ok(None) => {
                            self.update_available = None;
                            self.status =
                                format!("AI Mentor {} is up to date.", crate::update::current_version());
                        }
                        Err(e) => self.error = Some(format!("Update check failed: {e}")),
                    }
                }
                JobResult::JobSkills { outcome } => {
                    self.reading_job = false;
                    match outcome {
                        Ok(spec) => self.build_track_from_job(&spec),
                        Err(e) => {
                            self.error = Some(e);
                            self.status = "Could not read that posting.".to_string();
                        }
                    }
                }
                JobResult::CliTest { outcome } => {
                    self.testing_cli = false;
                    match outcome {
                        Ok(reply) => {
                            self.status = format!("CLI replied: {reply}");
                            self.error = None;
                        }
                        Err(e) => {
                            self.status = "The Claude CLI could not be run.".to_string();
                            self.error = Some(e);
                        }
                    }
                }
                JobResult::UpdateInstall { outcome } => match outcome {
                    Ok(version) => {
                        self.update_available = None;
                        self.update_installed = Some(version.clone());
                        self.status =
                            format!("Version {version} installed - restart to start using it.");
                    }
                    Err(e) => {
                        self.error = Some(format!("Update failed: {e}"));
                        self.status = "The running version is untouched.".to_string();
                    }
                },
            }
        }

        // Free the slots those jobs held and start whatever is next in line.
        self.cli_active = self.cli_active.saturating_sub(finished_cli);
        if finished_cli > 0 {
            self.pump_queue();
        }
    }

    // -- updates -----------------------------------------------------------

    pub fn check_for_update(&mut self) {
        if self.checking_update {
            return;
        }
        self.checking_update = true;
        self.jobs_in_flight += 1;
        crate::update::spawn_check(self.tx.clone());
    }

    /// Check the CLI on a worker like every other CLI call: running it inline
    /// froze the window for as long as Claude took to answer. The config comes
    /// from the settings form, so a path can be tried before it is saved.
    pub fn test_cli(&mut self, cfg: CliConfig) {
        if self.testing_cli {
            return;
        }
        self.testing_cli = true;
        self.jobs_in_flight += 1;
        self.status = "Testing the Claude CLI…".to_string();
        mentor::spawn_cli_test(self.tx.clone(), cfg);
    }

    pub fn install_update(&mut self) {
        self.jobs_in_flight += 1;
        self.status = "Downloading the new version…".to_string();
        crate::update::spawn_install(self.tx.clone());
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

    /// Shrink the window onto the display if the restored size does not fit.
    ///
    /// eframe has its own monitor clamp, but it sizes the monitor with
    /// `monitor.scale_factor()`, which winit reports as 1.0 here: on a 1920x1200
    /// display at 150% it believes the screen is 1920 points wide rather than
    /// 1280, so a 1350-point window sails through unclamped and hangs off the
    /// edge. egui's own `monitor_size` is reported in points correctly, so do
    /// the fit here instead - once, on the first frame that reports a monitor.
    fn fit_to_monitor(&mut self, ctx: &egui::Context) {
        if self.size_fitted {
            return;
        }
        let (monitor, outer, inner) = ctx.input(|i| {
            let v = i.viewport();
            (v.monitor_size, v.outer_rect, v.inner_rect)
        });
        let (Some(monitor), Some(inner)) = (monitor, inner) else {
            return; // not reported yet - try again next frame
        };
        if monitor.x <= 1.0 || monitor.y <= 1.0 {
            return;
        }
        self.size_fitted = true;

        // Borders and title bar, plus room for a taskbar along one edge.
        let chrome = outer.map_or(egui::Vec2::ZERO, |o| o.size() - inner.size());
        let limit = egui::vec2(monitor.x - chrome.x, monitor.y - chrome.y - 48.0);
        let size = inner.size();
        if size.x <= limit.x && size.y <= limit.y {
            return;
        }

        let fitted = egui::vec2(
            size.x.min(limit.x).max(crate::MIN_INNER_SIZE[0]),
            size.y.min(limit.y).max(crate::MIN_INNER_SIZE[1]),
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(fitted));
        // Re-centre for the new size; `center_on_screen` would still be using
        // the oversized rect this frame.
        let outer_size = fitted + chrome;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
            egui::pos2(
                ((monitor.x - outer_size.x) * 0.5).max(0.0),
                ((monitor.y - outer_size.y) * 0.5).max(0.0),
            ),
        ));
    }

    fn persist_window_size(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.last_size_save < 3.0 {
            return;
        }
        self.last_size_save = now;
        // A maximized window's size is not a size to reopen at: restoring it
        // un-maximized would hang off the edge of the screen.
        if ctx.input(|i| i.viewport().maximized) == Some(true) {
            return;
        }
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
        if !self.theme_applied {
            ui::theme::apply(ctx, self.dark_mode);
            self.theme_applied = true;
        }
        self.drain_jobs();
        if self.jobs_in_flight > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        if self.timer.as_ref().is_some_and(|s| s.is_running()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.fit_to_monitor(ctx);
        self.persist_window_size(ctx);

        let t = ui::theme::theme_for(self.dark_mode);

        egui::Panel::top("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .stroke(egui::Stroke::new(1.0, t.border)),
            )
            .show(root, |ui| self.header(ui, &t));

        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::symmetric(14, 6))
                    .stroke(egui::Stroke::new(1.0, t.border)),
            )
            .show(root, |ui| self.status_bar(ui, &t));

        egui::Panel::left("subjects")
            .resizable(true)
            .default_size(292.0)
            // A left panel's range defaults to 96..=INFINITY, so `default_size`
            // only picks the starting width - one stray drag on the divider can
            // hand the sidebar half the window and squeeze the lesson off the
            // screen. egui clamps a stored width to this range when it loads,
            // so bounding it here also recovers an already-dragged panel.
            .size_range(240.0..=380.0)
            .frame(
                egui::Frame::new()
                    .fill(t.plane)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(root, |ui| {
                ui::subjects::show(self, ui);
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.plane)
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(root, |ui| match self.tab {
                Tab::Study => ui::study::show(self, ui),
                Tab::Reviews => ui::reviews::show(self, ui),
                Tab::Analytics => ui::analytics::show(self, ui),
                Tab::Roadmap => ui::roadmap::show(self, ui),
            });

        ui::settings::show(self, ctx);
        ui::quiz_window::show(self, ctx);
    }
}

impl AiMentorApp {
    fn header(&mut self, ui: &mut egui::Ui, t: &ui::theme::Theme) {
        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(
                mark.shrink(1.0),
                egui::CornerRadius::same(4),
                t.accent,
            );
            ui.label(egui::RichText::new("AI Mentor").size(17.0).strong());
            ui.add_space(10.0);
            self.tab_bar(ui, t);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let toggle = if self.dark_mode { "Light" } else { "Dark" };
                if ui
                    .button(egui::RichText::new(toggle).size(12.0))
                    .on_hover_text("Switch theme")
                    .clicked()
                {
                    self.dark_mode = !self.dark_mode;
                    self.theme_applied = false;
                    let _ = self
                        .db
                        .set_setting("dark_mode", if self.dark_mode { "1" } else { "0" });
                }
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
                self.update_badge(ui, t);
            });
        });
    }

    /// Segmented control: one filled pill marks the active tab.
    fn tab_bar(&mut self, ui: &mut egui::Ui, t: &ui::theme::Theme) {
        egui::Frame::new()
            .fill(t.surface_alt)
            .corner_radius(egui::CornerRadius::same(9))
            .inner_margin(egui::Margin::same(3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (tab, label) in [
                        (Tab::Study, "Study"),
                        (Tab::Reviews, "Reviews"),
                        (Tab::Analytics, "Analytics"),
                        (Tab::Roadmap, "Roadmap"),
                    ] {
                        let selected = self.tab == tab;
                        let text = if selected {
                            egui::RichText::new(label).color(egui::Color32::WHITE).strong()
                        } else {
                            egui::RichText::new(label).color(t.text_weak)
                        };
                        let button = egui::Button::new(text)
                            .fill(if selected {
                                t.accent
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(7));
                        if ui.add(button).clicked() && !selected {
                            self.tab = tab;
                            let _ = self.db.set_setting("active_tab", tab.key());
                            if tab == Tab::Reviews {
                                self.reload_due_cards();
                            }
                        }
                    }
                });
            });
    }

    /// "Update available" / "restart to finish" affordance in the header.
    fn update_badge(&mut self, ui: &mut egui::Ui, t: &ui::theme::Theme) {
        if let Some(version) = self.update_installed.clone() {
            ui::theme::pill(ui, format!("v{version} ready \u{2014} restart"), t.good);
            return;
        }
        if let Some(release) = self.update_available.clone() {
            let button = egui::Button::new(
                egui::RichText::new(format!("Update to {}", release.version))
                    .color(egui::Color32::WHITE)
                    .strong(),
            )
            .fill(t.good)
            .corner_radius(egui::CornerRadius::same(8));
            let hover = if release.notes.is_empty() {
                release.name.clone()
            } else {
                release.notes.clone()
            };
            if ui.add(button).on_hover_text(hover).clicked() {
                self.install_update();
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, t: &ui::theme::Theme) {
        ui.horizontal(|ui| {
            if self.jobs_in_flight > 0 {
                ui.spinner();
                let queued = self.queue.len();
                let label = if queued > 0 {
                    format!("{} running · {queued} queued", self.jobs_in_flight)
                } else {
                    format!("{} running", self.jobs_in_flight)
                };
                ui.label(egui::RichText::new(label).color(t.accent).size(12.0));
                if queued > 0 && ui.small_button("stop").clicked() {
                    self.clear_queue();
                }
                ui.add_space(6.0);
            }
            if let Some(session) = self.timer.as_ref() {
                let elapsed = session.elapsed(ui.input(|i| i.time));
                ui::theme::pill(
                    ui,
                    format!("{} practice", timer::format_clock(elapsed)),
                    if session.is_running() {
                        t.accent
                    } else {
                        t.text_muted
                    },
                );
                ui.add_space(6.0);
            }
            ui.label(
                egui::RichText::new(&self.status)
                    .color(t.text_weak)
                    .size(12.0),
            );

            if let Some(err) = self.error.clone() {
                ui.add_space(6.0);
                ui::theme::pill(ui, "error", t.critical);
                ui.label(egui::RichText::new(err).color(t.critical).size(12.0));
                if ui.small_button("dismiss").clicked() {
                    self.error = None;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("v{}", crate::update::current_version()))
                        .color(t.text_muted)
                        .size(11.0),
                );
            });
        });
    }
}

pub fn status_color(status: DayStatus, t: &ui::theme::Theme) -> egui::Color32 {
    match status {
        DayStatus::NotStarted => t.text_muted,
        DayStatus::InProgress => t.accent,
        DayStatus::Done => t.good,
    }
}

/// A posting has to come out the other side as an ordered, studiable track.
#[cfg(test)]
mod job_track {
    use super::*;

    fn app() -> (tempfile::TempDir, AiMentorApp) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let db = Db::open_at(&tmp.path().join("t.db")).expect("db opens");
        db.set_setting("check_updates_on_start", "0").expect("setting");
        (tmp, AiMentorApp::new(db))
    }

    fn skill(name: &str, priority: i64) -> JobSkillJson {
        JobSkillJson {
            name: name.to_string(),
            priority,
            ..Default::default()
        }
    }

    #[test]
    fn a_posting_becomes_a_track_in_the_order_it_asks_for() {
        let (_tmp, mut app) = app();
        app.build_track_from_job(&JobSpecJson {
            role: "Java Backend Developer".to_string(),
            seniority: "junior".to_string(),
            skills: vec![
                skill("Java 17", 1),
                skill("Spring Boot 3.x", 2),
                skill("Postgres", 3),
                skill("gRPC", 4), // nothing in the catalogue teaches this
            ],
        });

        assert_eq!(app.target_role, "Java Backend Developer");
        assert_eq!(app.target_seniority, "junior");

        let entries = &app.track.as_ref().expect("a track was built").entries;
        let subjects: Vec<&str> = entries.iter().map(|e| e.subject.as_str()).collect();
        assert_eq!(
            subjects,
            vec!["Java", "Spring Boot", "PostgreSQL", "gRPC"],
            "matched to the catalogue where possible, invented where not, in posting order"
        );

        for name in ["Java", "Spring Boot", "PostgreSQL", "gRPC"] {
            let id = app.db.stack_id_by_name(name).unwrap().unwrap();
            assert!(
                app.stacks.iter().any(|s| s.id == id && s.selected),
                "{name} is ticked, so the track and the checkboxes agree"
            );
        }
    }

    #[test]
    fn the_posting_track_is_its_own_roadmap_and_leaves_the_preset_alone() {
        let (_tmp, mut app) = app();
        let before = app.db.roadmaps().unwrap().len();

        app.build_track_from_job(&JobSpecJson {
            role: "React Developer".to_string(),
            skills: vec![skill("React", 1), skill("TypeScript", 2)],
            ..Default::default()
        });

        let after = app.db.roadmaps().unwrap();
        assert_eq!(after.len(), before + 1, "a new roadmap, not an overwrite");
        let active = after.iter().find(|r| r.active).expect("one is active");
        assert_eq!(active.name, "React Developer (job track)");
        assert!(
            after.iter().any(|r| r.is_preset && !r.active),
            "the preset survives, just inactive"
        );
    }

    #[test]
    fn a_posting_with_nothing_studiable_is_refused_rather_than_wiping_the_track() {
        let (_tmp, mut app) = app();
        let roadmaps_before = app.db.roadmaps().unwrap().len();

        app.build_track_from_job(&JobSpecJson {
            role: "Vibes Engineer".to_string(),
            skills: vec![skill("   ", 1)],
            ..Default::default()
        });

        assert!(app.error.is_some(), "it says so");
        assert_eq!(app.db.roadmaps().unwrap().len(), roadmaps_before);
    }

    /// A real posting does this: it listed both "Java 17" and "Java
    /// Collections", which are one subject here.
    #[test]
    fn two_skills_naming_the_same_subject_make_one_track_entry() {
        let (_tmp, mut app) = app();
        app.build_track_from_job(&JobSpecJson {
            role: "Java Backend Engineer".to_string(),
            skills: vec![
                skill("Java 17", 1),
                skill("Java Collections", 2),
                skill("Spring Boot 3", 3),
            ],
            ..Default::default()
        });

        let subjects: Vec<&str> = app
            .track
            .as_ref()
            .expect("a track")
            .entries
            .iter()
            .map(|e| e.subject.as_str())
            .collect();
        assert_eq!(
            subjects,
            vec!["Java", "Spring Boot"],
            "Java appears once, at the position it was first asked for"
        );
    }

    #[test]
    fn ticking_a_subject_mid_study_appends_it_to_the_track() {
        let (_tmp, mut app) = app();
        let before = app.track.as_ref().expect("preset track").entries.len();
        let redis = app.db.stack_id_by_name("Redis").unwrap().unwrap();

        app.set_on_track(redis, true);
        let entries = &app.track.as_ref().unwrap().entries;
        assert_eq!(entries.len(), before + 1);
        assert_eq!(
            entries.last().unwrap().subject,
            "Redis",
            "a topic picked up later lands at the end, behind what is already underway"
        );

        app.set_on_track(redis, false);
        assert_eq!(
            app.track.as_ref().unwrap().entries.len(),
            before,
            "unticking takes it back off"
        );
    }
}

/// The timer through the app rather than the state machine: a stopped session
/// has to reach `study_sessions`, which is what every hour count is built on.
#[cfg(test)]
mod timer_logging {
    use super::*;

    fn app_with_one_day() -> (tempfile::TempDir, AiMentorApp, i64) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut db = Db::open_at(&tmp.path().join("t.db")).expect("db opens");
        // Keep the suite offline.
        db.set_setting("check_updates_on_start", "0").expect("setting");

        let stack_id = db
            .stack_id_by_name("Redis")
            .expect("query")
            .expect("Redis is seeded");
        let plan = PlanJson {
            days: vec![DayJson {
                day: 1,
                week: 1,
                difficulty: 1,
                est_minutes: 60,
                ..Default::default()
            }],
            ..Default::default()
        };
        let plan_id = db.save_plan(stack_id, 1, 60, &plan, "{}").expect("plan saved");
        let day_id = db.days_for_plan(plan_id).expect("days")[0].id;

        let mut app = AiMentorApp::new(db);
        app.select_subject(stack_id);
        (tmp, app, day_id)
    }

    #[test]
    fn stopping_the_timer_logs_its_minutes_and_starts_the_day() {
        let (_tmp, mut app, day_id) = app_with_one_day();
        assert_eq!(app.db.logged_minutes_for_day(day_id).unwrap(), 0);

        app.start_timer(day_id, 0.0);
        app.stop_timer(25.0 * 60.0);

        assert_eq!(app.db.logged_minutes_for_day(day_id).unwrap(), 25);
        assert!(app.timer.is_none(), "the timer is cleared once logged");
        let day = app.days.iter().find(|d| d.id == day_id).expect("day reloaded");
        assert_eq!(
            day.status,
            DayStatus::InProgress,
            "practising a not-started day starts it"
        );
    }

    #[test]
    fn paused_time_is_not_logged() {
        let (_tmp, mut app, day_id) = app_with_one_day();
        app.start_timer(day_id, 0.0);
        app.toggle_timer(600.0); // pause after 10 min
        app.toggle_timer(3000.0); // resume 40 min later
        app.stop_timer(3300.0); // and run 5 min more

        assert_eq!(app.db.logged_minutes_for_day(day_id).unwrap(), 15);
    }

    #[test]
    fn a_session_under_a_minute_logs_nothing() {
        let (_tmp, mut app, day_id) = app_with_one_day();
        app.start_timer(day_id, 0.0);
        app.stop_timer(20.0);

        assert_eq!(app.db.logged_minutes_for_day(day_id).unwrap(), 0);
        assert!(app.timer.is_none(), "a discarded session still clears");
    }
}
