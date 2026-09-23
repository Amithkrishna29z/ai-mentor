//! SQLite access: connection, migrations, seeding and every query the UI needs.

use anyhow::{Context, Result};
use chrono::Local;
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use std::path::PathBuf;

use crate::models::*;

pub struct Db {
    pub conn: Connection,
}

fn now() -> String {
    Local::now().to_rfc3339()
}

pub fn today() -> String {
    Local::now().date_naive().to_string()
}

/// Parse a JSON array column, falling back to empty on any malformed value.
fn json_vec<T: DeserializeOwned>(raw: &str) -> Vec<T> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
}

impl Db {
    /// Open (creating if needed) the database in the OS data directory.
    pub fn open() -> Result<Self> {
        Self::open_at(&Self::db_path()?)
    }

    /// Open a database at an explicit path. Tests use this to work against a
    /// temp file instead of the real one.
    pub fn open_at(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.migrate()?;
        db.seed()?;
        Ok(db)
    }

    pub fn db_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "AiMentor", "ai-mentor")
            .context("could not resolve a platform data directory")?;
        Ok(dirs.data_dir().join("ai_mentor.db"))
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// Insert the built-in subjects and the preset roadmap once.
    fn seed(&self) -> Result<()> {
        for (_, names) in CATALOGUE {
            for name in *names {
                self.conn.execute(
                    "INSERT OR IGNORE INTO tech_stacks (name, selected, is_custom, created_at)
                     VALUES (?1, 0, 0, ?2)",
                    params![name, now()],
                )?;
            }
        }
        crate::roadmap::seed_preset(self)?;
        Ok(())
    }

    // -- settings ----------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn setting_or(&self, key: &str, default: &str) -> String {
        self.get_setting(key).unwrap_or_else(|| default.to_string())
    }

    pub fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.get_setting(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // -- tech stacks -------------------------------------------------------

    pub fn all_stacks(&self) -> Result<Vec<TechStack>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, selected, is_custom FROM tech_stacks ORDER BY name")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TechStack {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    selected: r.get::<_, i64>(2)? != 0,
                    is_custom: r.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn stack_id_by_name(&self, name: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM tech_stacks WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_stack_selected(&self, id: i64, selected: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE tech_stacks SET selected = ?2 WHERE id = ?1",
            params![id, selected as i64],
        )?;
        Ok(())
    }

    pub fn add_custom_stack(&self, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO tech_stacks (name, selected, is_custom, created_at)
             VALUES (?1, 1, 1, ?2)",
            params![name, now()],
        )?;
        Ok(self.stack_id_by_name(name)?.unwrap_or(0))
    }

    // -- plans -------------------------------------------------------------

    pub fn plan_for_stack(&self, stack_id: i64) -> Result<Option<Plan>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, tech_stack_id, target_hours, minutes_per_day, plan_json
                 FROM plans WHERE tech_stack_id = ?1",
                params![stack_id],
                |r| {
                    Ok(Plan {
                        id: r.get(0)?,
                        tech_stack_id: r.get(1)?,
                        target_hours: r.get(2)?,
                        minutes_per_day: r.get(3)?,
                        plan_json: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Replace any existing plan for the subject and expand the JSON into rows.
    /// Also seeds spaced-repetition cards from the interview questions.
    pub fn save_plan(
        &mut self,
        stack_id: i64,
        target_hours: i64,
        minutes_per_day: i64,
        plan: &PlanJson,
        raw: &str,
    ) -> Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM plans WHERE tech_stack_id = ?1", params![stack_id])?;
        tx.execute(
            "INSERT INTO plans (tech_stack_id, target_hours, minutes_per_day, plan_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![stack_id, target_hours, minutes_per_day, raw, now()],
        )?;
        let plan_id = tx.last_insert_rowid();

        for day in &plan.days {
            tx.execute(
                "INSERT INTO days (plan_id, day_number, week_number, title, subskill,
                    objectives_json, practice_task, difficulty, builds_on_json,
                    interview_questions_json, content_md, est_minutes, status,
                    hands_on_done, notes, content_edited, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '', ?11, 'not_started', 0, '', 0, ?12)",
                params![
                    plan_id,
                    day.day,
                    day.week,
                    day.title,
                    day.subskill,
                    to_json(&day.objectives),
                    day.practice_task,
                    day.difficulty,
                    to_json(&day.builds_on),
                    to_json(&day.interview_questions),
                    day.est_minutes,
                    now(),
                ],
            )?;
            let day_id = tx.last_insert_rowid();

            for q in &day.interview_questions {
                if q.q.trim().is_empty() {
                    continue;
                }
                let back = if q.key_points.is_empty() {
                    String::from("(no key points supplied)")
                } else {
                    q.key_points
                        .iter()
                        .map(|p| format!("- {p}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                tx.execute(
                    "INSERT INTO review_cards (source_type, source_ref, plan_id, front, back,
                        easiness, interval_days, repetitions, due_date, created_at, updated_at)
                     VALUES ('interview', ?1, ?2, ?3, ?4, 2.5, 0, 0, ?5, ?6, ?6)",
                    params![day_id, plan_id, q.q, back, today(), now()],
                )?;
            }
        }

        for wp in &plan.weekly_projects {
            tx.execute(
                "INSERT INTO weekly_projects (plan_id, week_number, title, description,
                    acceptance_json, status, github_url, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'not_started', '', '')",
                params![
                    plan_id,
                    wp.week,
                    wp.title,
                    wp.description,
                    to_json(&wp.acceptance_criteria),
                ],
            )?;
        }

        tx.commit()?;
        Ok(plan_id)
    }

    // -- days --------------------------------------------------------------

    pub fn days_for_plan(&self, plan_id: i64) -> Result<Vec<Day>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, day_number, week_number, title, subskill, objectives_json,
                    practice_task, difficulty, builds_on_json, interview_questions_json,
                    content_md, est_minutes, status, hands_on_done, notes, content_edited
             FROM days WHERE plan_id = ?1 ORDER BY day_number",
        )?;
        let rows = stmt
            .query_map(params![plan_id], |r| {
                Ok(Day {
                    id: r.get(0)?,
                    plan_id: r.get(1)?,
                    day_number: r.get(2)?,
                    week_number: r.get(3)?,
                    title: r.get(4)?,
                    subskill: r.get(5)?,
                    objectives: json_vec(&r.get::<_, String>(6)?),
                    practice_task: r.get(7)?,
                    difficulty: r.get(8)?,
                    builds_on: json_vec(&r.get::<_, String>(9)?),
                    interview_questions: json_vec(&r.get::<_, String>(10)?),
                    content_md: r.get(11)?,
                    est_minutes: r.get(12)?,
                    status: DayStatus::parse(&r.get::<_, String>(13)?),
                    hands_on_done: r.get::<_, i64>(14)? != 0,
                    notes: r.get(15)?,
                    content_edited: r.get::<_, i64>(16)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_day_content(&self, day_id: i64, md: &str, edited: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE days SET content_md = ?2, content_edited = ?3, updated_at = ?4 WHERE id = ?1",
            params![day_id, md, edited as i64, now()],
        )?;
        Ok(())
    }

    pub fn set_day_status(&self, day_id: i64, status: DayStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE days SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![day_id, status.as_str(), now()],
        )?;
        Ok(())
    }

    pub fn set_day_hands_on(&self, day_id: i64, done: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE days SET hands_on_done = ?2, updated_at = ?3 WHERE id = ?1",
            params![day_id, done as i64, now()],
        )?;
        Ok(())
    }

    pub fn set_day_notes(&self, day_id: i64, notes: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE days SET notes = ?2, updated_at = ?3 WHERE id = ?1",
            params![day_id, notes, now()],
        )?;
        Ok(())
    }

    // -- study sessions ----------------------------------------------------

    pub fn log_minutes(&self, day_id: i64, minutes: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO study_sessions (day_id, minutes, logged_at) VALUES (?1, ?2, ?3)",
            params![day_id, minutes, now()],
        )?;
        Ok(())
    }

    pub fn logged_minutes_for_plan(&self, plan_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(s.minutes), 0) FROM study_sessions s
             JOIN days d ON d.id = s.day_id WHERE d.plan_id = ?1",
            params![plan_id],
            |r| r.get(0),
        )?)
    }

    pub fn logged_minutes_for_day(&self, day_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(minutes), 0) FROM study_sessions WHERE day_id = ?1",
            params![day_id],
            |r| r.get(0),
        )?)
    }

    /// `(date, minutes)` totals per calendar day, for the heatmap and trends.
    pub fn minutes_by_date(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT substr(logged_at, 1, 10) AS d, SUM(minutes)
             FROM study_sessions GROUP BY d ORDER BY d",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // -- weekly projects ---------------------------------------------------

    pub fn weekly_projects(&self, plan_id: i64) -> Result<Vec<WeeklyProject>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, week_number, title, description, acceptance_json,
                    status, github_url, notes
             FROM weekly_projects WHERE plan_id = ?1 ORDER BY week_number",
        )?;
        let rows = stmt
            .query_map(params![plan_id], |r| {
                Ok(WeeklyProject {
                    id: r.get(0)?,
                    plan_id: r.get(1)?,
                    week_number: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                    acceptance: json_vec(&r.get::<_, String>(5)?),
                    status: r.get(6)?,
                    github_url: r.get(7)?,
                    notes: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_project_github(&self, id: i64, url: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE weekly_projects SET github_url = ?2 WHERE id = ?1",
            params![id, url],
        )?;
        Ok(())
    }

    pub fn set_project_status(&self, id: i64, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE weekly_projects SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    pub fn set_project_notes(&self, id: i64, notes: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE weekly_projects SET notes = ?2 WHERE id = ?1",
            params![id, notes],
        )?;
        Ok(())
    }

    pub fn insert_verification(
        &self,
        wp_id: i64,
        github_url: &str,
        commit_sha: &str,
        report_md: &str,
        review: &VerificationJson,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO project_verifications (weekly_project_id, github_url, commit_sha,
                verdict, score, report_md, issues_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                wp_id,
                github_url,
                commit_sha,
                review.verdict,
                review.score,
                report_md,
                to_json(&review.issues),
                now()
            ],
        )?;
        Ok(())
    }

    pub fn verifications(&self, wp_id: i64) -> Result<Vec<Verification>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, weekly_project_id, github_url, commit_sha, verdict, score,
                    report_md, issues_json, created_at
             FROM project_verifications WHERE weekly_project_id = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map(params![wp_id], |r| {
                Ok(Verification {
                    id: r.get(0)?,
                    weekly_project_id: r.get(1)?,
                    github_url: r.get(2)?,
                    commit_sha: r.get(3)?,
                    verdict: r.get(4)?,
                    score: r.get(5)?,
                    report_md: r.get(6)?,
                    issues: json_vec(&r.get::<_, String>(7)?),
                    created_at: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // -- roadmaps ----------------------------------------------------------

    pub fn roadmaps(&self) -> Result<Vec<Roadmap>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, is_preset, active FROM roadmaps ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Roadmap {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    is_preset: r.get::<_, i64>(2)? != 0,
                    active: r.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_active_roadmap(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE roadmaps SET active = 0", [])?;
        self.conn
            .execute("UPDATE roadmaps SET active = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn create_roadmap(&self, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO roadmaps (name, is_preset, active, created_at)
             VALUES (?1, 0, 0, ?2)",
            params![name, now()],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM roadmaps WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?)
    }

    pub fn delete_roadmap(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM roadmaps WHERE id = ?1 AND is_preset = 0", params![id])?;
        Ok(())
    }

    pub fn roadmap_items(&self, roadmap_id: i64) -> Result<Vec<RoadmapItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT ri.id, ri.roadmap_id, ri.tech_stack_id, ts.name, ri.position
             FROM roadmap_items ri JOIN tech_stacks ts ON ts.id = ri.tech_stack_id
             WHERE ri.roadmap_id = ?1 ORDER BY ri.position",
        )?;
        let rows = stmt
            .query_map(params![roadmap_id], |r| {
                Ok(RoadmapItem {
                    id: r.get(0)?,
                    roadmap_id: r.get(1)?,
                    tech_stack_id: r.get(2)?,
                    tech_stack_name: r.get(3)?,
                    position: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_roadmap_item(&self, roadmap_id: i64, stack_id: i64) -> Result<()> {
        let next: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM roadmap_items WHERE roadmap_id = ?1",
            params![roadmap_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO roadmap_items (roadmap_id, tech_stack_id, position)
             VALUES (?1, ?2, ?3)",
            params![roadmap_id, stack_id, next],
        )?;
        Ok(())
    }

    /// Drop every subject from a roadmap, for rebuilding it from a posting.
    pub fn clear_roadmap_items(&self, roadmap_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM roadmap_items WHERE roadmap_id = ?1",
            params![roadmap_id],
        )?;
        Ok(())
    }

    pub fn remove_roadmap_item(&self, item_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM roadmap_items WHERE id = ?1", params![item_id])?;
        Ok(())
    }

    pub fn set_item_position(&self, item_id: i64, position: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE roadmap_items SET position = ?2 WHERE id = ?1",
            params![item_id, position],
        )?;
        Ok(())
    }

    // -- quizzes -----------------------------------------------------------

    pub fn save_quiz(
        &mut self,
        plan_id: i64,
        scope: &str,
        day_id: Option<i64>,
        week_number: Option<i64>,
        quiz: &QuizJson,
    ) -> Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO quizzes (plan_id, scope, day_id, week_number, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![plan_id, scope, day_id, week_number, now()],
        )?;
        let quiz_id = tx.last_insert_rowid();
        for q in &quiz.questions {
            if q.options.len() < 2 {
                continue;
            }
            let correct = q.correct_index.clamp(0, q.options.len() as i64 - 1);
            tx.execute(
                "INSERT INTO quiz_questions (quiz_id, question, options_json, correct_index, explanation)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![quiz_id, q.question, to_json(&q.options), correct, q.explanation],
            )?;
        }
        tx.commit()?;
        Ok(quiz_id)
    }

    /// Most recent quiz for a day or week, if one has been generated.
    pub fn latest_quiz(
        &self,
        plan_id: i64,
        scope: &str,
        day_id: Option<i64>,
        week_number: Option<i64>,
    ) -> Result<Option<Quiz>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, plan_id, scope, day_id, week_number FROM quizzes
                 WHERE plan_id = ?1 AND scope = ?2
                   AND (day_id IS ?3) AND (week_number IS ?4)
                 ORDER BY id DESC LIMIT 1",
                params![plan_id, scope, day_id, week_number],
                |r| {
                    Ok(Quiz {
                        id: r.get(0)?,
                        plan_id: r.get(1)?,
                        scope: r.get(2)?,
                        day_id: r.get(3)?,
                        week_number: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn quiz_questions(&self, quiz_id: i64) -> Result<Vec<QuizQuestion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, quiz_id, question, options_json, correct_index, explanation
             FROM quiz_questions WHERE quiz_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![quiz_id], |r| {
                Ok(QuizQuestion {
                    id: r.get(0)?,
                    quiz_id: r.get(1)?,
                    question: r.get(2)?,
                    options: json_vec(&r.get::<_, String>(3)?),
                    correct_index: r.get(4)?,
                    explanation: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn insert_attempt(
        &self,
        quiz_id: i64,
        score: i64,
        total: i64,
        answers_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO quiz_attempts (quiz_id, score, total, answers_json, attempted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![quiz_id, score, total, answers_json, now()],
        )?;
        Ok(())
    }

    /// Average quiz score as a percentage across every attempt.
    pub fn avg_quiz_score(&self) -> Result<Option<f64>> {
        Ok(self.conn.query_row(
            "SELECT CASE WHEN SUM(total) > 0 THEN 100.0 * SUM(score) / SUM(total) END
             FROM quiz_attempts",
            [],
            |r| r.get(0),
        )?)
    }

    // -- review cards ------------------------------------------------------

    pub fn due_cards(&self, today_str: &str) -> Result<Vec<ReviewCard>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_type, source_ref, plan_id, front, back, easiness,
                    interval_days, repetitions, due_date, last_grade
             FROM review_cards WHERE due_date <= ?1 ORDER BY due_date, id",
        )?;
        let rows = stmt
            .query_map(params![today_str], |r| {
                Ok(ReviewCard {
                    id: r.get(0)?,
                    source_type: r.get(1)?,
                    source_ref: r.get(2)?,
                    plan_id: r.get(3)?,
                    front: r.get(4)?,
                    back: r.get(5)?,
                    easiness: r.get(6)?,
                    interval_days: r.get(7)?,
                    repetitions: r.get(8)?,
                    due_date: r.get(9)?,
                    last_grade: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn due_card_count(&self, today_str: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM review_cards WHERE due_date <= ?1",
            params![today_str],
            |r| r.get(0),
        )?)
    }

    pub fn total_card_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM review_cards", [], |r| r.get(0))?)
    }

    /// Insert a card, or refresh an existing one with the same source.
    pub fn upsert_card(
        &self,
        source_type: &str,
        source_ref: Option<i64>,
        plan_id: Option<i64>,
        front: &str,
        back: &str,
    ) -> Result<()> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM review_cards WHERE source_type = ?1 AND source_ref IS ?2",
                params![source_type, source_ref],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            // A mistake resets the card so it comes back tomorrow.
            Some(id) => {
                self.conn.execute(
                    "UPDATE review_cards SET front = ?2, back = ?3, repetitions = 0,
                        interval_days = 1, due_date = ?4, updated_at = ?5 WHERE id = ?1",
                    params![id, front, back, today(), now()],
                )?;
            }
            None => {
                self.conn.execute(
                    "INSERT INTO review_cards (source_type, source_ref, plan_id, front, back,
                        easiness, interval_days, repetitions, due_date, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 2.5, 0, 0, ?6, ?7, ?7)",
                    params![source_type, source_ref, plan_id, front, back, today(), now()],
                )?;
            }
        }
        Ok(())
    }

    pub fn save_card_schedule(&self, card: &ReviewCard) -> Result<()> {
        self.conn.execute(
            "UPDATE review_cards SET easiness = ?2, interval_days = ?3, repetitions = ?4,
                due_date = ?5, last_grade = ?6, updated_at = ?7 WHERE id = ?1",
            params![
                card.id,
                card.easiness,
                card.interval_days,
                card.repetitions,
                card.due_date,
                card.last_grade,
                now()
            ],
        )?;
        Ok(())
    }

    // -- interview self-rating --------------------------------------------

    pub fn set_interview_confidence(&self, day_id: i64, confidence: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO interview_reviews (day_id, confidence, reviewed_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(day_id) DO UPDATE SET confidence = excluded.confidence,
                                               reviewed_at = excluded.reviewed_at",
            params![day_id, confidence, now()],
        )?;
        Ok(())
    }

    pub fn interview_confidence(&self, day_id: i64) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT confidence FROM interview_reviews WHERE day_id = ?1",
                params![day_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    // -- cross-subject stats ----------------------------------------------

    /// Progress of every subject that has a plan, for the analytics table.
    pub fn subject_progress(&self) -> Result<Vec<SubjectProgress>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts.name,
                    SUM(CASE WHEN d.status = 'done' THEN 1 ELSE 0 END),
                    COUNT(d.id),
                    COALESCE((SELECT SUM(s.minutes) FROM study_sessions s
                              JOIN days dd ON dd.id = s.day_id WHERE dd.plan_id = p.id), 0),
                    p.target_hours
             FROM plans p
             JOIN tech_stacks ts ON ts.id = p.tech_stack_id
             LEFT JOIN days d ON d.plan_id = p.id
             GROUP BY p.id ORDER BY ts.name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SubjectProgress {
                    name: r.get(0)?,
                    days_done: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    days_total: r.get(2)?,
                    logged_minutes: r.get(3)?,
                    target_hours: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Subjects that already have a generated course. One query instead of
    /// `plan_for_stack` per row per frame.
    pub fn stacks_with_plans(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT tech_stack_id FROM plans")?;
        let rows = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn total_days_done(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM days WHERE status = 'done'",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn highest_difficulty_reached(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(difficulty), 0) FROM days WHERE status = 'done'",
            [],
            |r| r.get(0),
        )?)
    }
}

/// One row of the cross-subject progress table.
pub struct SubjectProgress {
    pub name: String,
    pub days_done: i64,
    pub days_total: i64,
    pub logged_minutes: i64,
    pub target_hours: i64,
}

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tech_stacks (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  selected   INTEGER NOT NULL DEFAULT 0,
  is_custom  INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plans (
  id              INTEGER PRIMARY KEY,
  tech_stack_id   INTEGER NOT NULL REFERENCES tech_stacks(id) ON DELETE CASCADE,
  target_hours    INTEGER NOT NULL DEFAULT 20,
  minutes_per_day INTEGER NOT NULL DEFAULT 60,
  plan_json       TEXT NOT NULL,
  created_at      TEXT NOT NULL,
  UNIQUE(tech_stack_id)
);

CREATE TABLE IF NOT EXISTS days (
  id            INTEGER PRIMARY KEY,
  plan_id       INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
  day_number    INTEGER NOT NULL,
  week_number   INTEGER NOT NULL,
  title         TEXT NOT NULL,
  subskill      TEXT NOT NULL,
  objectives_json TEXT NOT NULL DEFAULT '[]',
  practice_task TEXT NOT NULL DEFAULT '',
  difficulty    INTEGER NOT NULL DEFAULT 1,
  builds_on_json TEXT NOT NULL DEFAULT '[]',
  interview_questions_json TEXT NOT NULL DEFAULT '[]',
  content_md    TEXT NOT NULL DEFAULT '',
  est_minutes   INTEGER NOT NULL DEFAULT 60,
  status        TEXT NOT NULL DEFAULT 'not_started',
  hands_on_done INTEGER NOT NULL DEFAULT 0,
  notes         TEXT NOT NULL DEFAULT '',
  content_edited INTEGER NOT NULL DEFAULT 0,
  updated_at    TEXT NOT NULL,
  UNIQUE(plan_id, day_number)
);

CREATE TABLE IF NOT EXISTS interview_reviews (
  id          INTEGER PRIMARY KEY,
  day_id      INTEGER NOT NULL REFERENCES days(id) ON DELETE CASCADE,
  confidence  INTEGER NOT NULL DEFAULT 0,
  reviewed_at TEXT NOT NULL,
  UNIQUE(day_id)
);

CREATE TABLE IF NOT EXISTS study_sessions (
  id        INTEGER PRIMARY KEY,
  day_id    INTEGER NOT NULL REFERENCES days(id) ON DELETE CASCADE,
  minutes   INTEGER NOT NULL,
  logged_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS weekly_projects (
  id            INTEGER PRIMARY KEY,
  plan_id       INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
  week_number   INTEGER NOT NULL,
  title         TEXT NOT NULL,
  description   TEXT NOT NULL,
  acceptance_json TEXT NOT NULL DEFAULT '[]',
  status        TEXT NOT NULL DEFAULT 'not_started',
  github_url    TEXT NOT NULL DEFAULT '',
  notes         TEXT NOT NULL DEFAULT '',
  UNIQUE(plan_id, week_number)
);

CREATE TABLE IF NOT EXISTS project_verifications (
  id             INTEGER PRIMARY KEY,
  weekly_project_id INTEGER NOT NULL REFERENCES weekly_projects(id) ON DELETE CASCADE,
  github_url     TEXT NOT NULL,
  commit_sha     TEXT NOT NULL DEFAULT '',
  verdict        TEXT NOT NULL DEFAULT 'unknown',
  score          INTEGER NOT NULL DEFAULT 0,
  report_md      TEXT NOT NULL DEFAULT '',
  issues_json    TEXT NOT NULL DEFAULT '[]',
  created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS roadmaps (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  is_preset  INTEGER NOT NULL DEFAULT 0,
  active     INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS roadmap_items (
  id            INTEGER PRIMARY KEY,
  roadmap_id    INTEGER NOT NULL REFERENCES roadmaps(id) ON DELETE CASCADE,
  tech_stack_id INTEGER NOT NULL REFERENCES tech_stacks(id) ON DELETE CASCADE,
  position      INTEGER NOT NULL,
  UNIQUE(roadmap_id, tech_stack_id)
);

CREATE TABLE IF NOT EXISTS quizzes (
  id          INTEGER PRIMARY KEY,
  plan_id     INTEGER NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
  scope       TEXT NOT NULL,
  day_id      INTEGER REFERENCES days(id) ON DELETE CASCADE,
  week_number INTEGER,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS quiz_questions (
  id            INTEGER PRIMARY KEY,
  quiz_id       INTEGER NOT NULL REFERENCES quizzes(id) ON DELETE CASCADE,
  question      TEXT NOT NULL,
  options_json  TEXT NOT NULL,
  correct_index INTEGER NOT NULL,
  explanation   TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS quiz_attempts (
  id             INTEGER PRIMARY KEY,
  quiz_id        INTEGER NOT NULL REFERENCES quizzes(id) ON DELETE CASCADE,
  score          INTEGER NOT NULL,
  total          INTEGER NOT NULL,
  answers_json   TEXT NOT NULL,
  attempted_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS review_cards (
  id           INTEGER PRIMARY KEY,
  source_type  TEXT NOT NULL,
  source_ref   INTEGER,
  plan_id      INTEGER REFERENCES plans(id) ON DELETE CASCADE,
  front        TEXT NOT NULL,
  back         TEXT NOT NULL,
  easiness     REAL NOT NULL DEFAULT 2.5,
  interval_days INTEGER NOT NULL DEFAULT 0,
  repetitions  INTEGER NOT NULL DEFAULT 0,
  due_date     TEXT NOT NULL,
  last_grade   INTEGER,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

#[cfg(test)]
mod course_pipeline {
    use super::*;
    use crate::mentor::{self, CliConfig};

    /// The whole study-course path against the real Claude CLI: generate the
    /// course, store it, fetch a day's lesson, and read it back the way the
    /// Day view does. Ignored by default:
    /// `cargo test -- --ignored --nocapture course_is_generated`.
    #[test]
    #[ignore]
    fn course_is_generated_and_its_lesson_is_readable() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut db = Db::open_at(&tmp.path().join("test.db")).expect("db opens");

        let stack_id = db
            .stack_id_by_name("Redis")
            .expect("query")
            .expect("Redis is seeded");

        // 1. The CLI builds the course.
        let cfg = CliConfig::default();
        let prompt = mentor::prompt_plan("Redis", 2, 60, &[], "");
        let raw = mentor::run_cli(&cfg, &prompt, None).expect("claude CLI ran");
        let slice = mentor::extract_json(&raw).expect("JSON in the reply");
        let mut plan: PlanJson = serde_json::from_str(slice).expect("plan parses");
        mentor::enforce_ramp(&mut plan);
        assert!(!plan.days.is_empty(), "the course has days");

        // 2. It is stored as days the app can show.
        let plan_id = db
            .save_plan(stack_id, 2, 60, &plan, &raw)
            .expect("plan saved");
        let days = db.days_for_plan(plan_id).expect("days load");
        assert_eq!(days.len(), plan.days.len());
        assert!(
            days.iter().all(|d| d.content_md.is_empty()),
            "lessons start empty"
        );

        // 3. The lesson for day 1 is fetched by the CLI, exactly as the queue does.
        let day = &days[0];
        println!("fetching lesson for Day {}: {}", day.day_number, day.title);
        let lesson_prompt = mentor::prompt_day("Redis", day);
        let lesson = mentor::run_cli(&cfg, &lesson_prompt, None).expect("lesson fetched");
        db.set_day_content(day.id, lesson.trim(), false)
            .expect("lesson stored");

        // 4. It reads back as study material the Day view can render.
        let reloaded = db.days_for_plan(plan_id).expect("days reload");
        let first = &reloaded[0];
        println!("lesson is {} chars", first.content_md.len());
        println!(
            "--- first 400 chars ---\n{}",
            first.content_md.chars().take(400).collect::<String>()
        );
        assert!(
            first.content_md.len() > 500,
            "the stored lesson has real content, got {} chars",
            first.content_md.len()
        );
        assert!(!first.content_edited, "untouched lessons stay auto-refreshable");
    }
}
