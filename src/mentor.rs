//! Claude CLI integration: prompt builders, process invocation on a background
//! thread, JSON extraction/repair, and the 20-hour-rule complexity ramp.

use anyhow::{anyhow, bail, Result};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Sender;

use crate::models::*;

/// How the `claude` binary is invoked. Every part is editable in Settings so a
/// differently named or wrapped binary still works.
#[derive(Debug, Clone)]
pub struct CliConfig {
    pub claude_path: String,
    pub prompt_flag: String,
    pub extra_args: String,
    pub git_path: String,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            claude_path: "claude".to_string(),
            prompt_flag: "-p".to_string(),
            extra_args: "--output-format text".to_string(),
            git_path: "git".to_string(),
        }
    }
}

/// Everything one course generation needs. A struct rather than another
/// positional argument, matching `verify::VerifyRequest`.
pub struct PlanRequest {
    pub stack_id: i64,
    pub tech: String,
    pub target_hours: i64,
    pub minutes_per_day: i64,
    /// Subjects already cleared earlier on the track.
    pub builds_on: Vec<String>,
    /// The job the learner is training for; empty when none is set.
    pub role: String,
}

/// Results handed back to the UI thread from background jobs.
pub enum JobResult {
    Plan {
        stack_id: i64,
        tech: String,
        outcome: Result<PlanJson, String>,
        raw: String,
    },
    DayContent {
        day_id: i64,
        outcome: Result<String, String>,
    },
    Quiz {
        plan_id: i64,
        scope: String,
        day_id: Option<i64>,
        week_number: Option<i64>,
        outcome: Result<QuizJson, String>,
    },
    Verify {
        weekly_project_id: i64,
        github_url: String,
        commit_sha: String,
        report_md: String,
        outcome: Result<VerificationJson, String>,
    },
    UpdateCheck {
        outcome: std::result::Result<Option<crate::update::ReleaseInfo>, String>,
    },
    UpdateInstall {
        outcome: std::result::Result<String, String>,
    },
    CliTest {
        outcome: std::result::Result<String, String>,
    },
    JobSkills {
        outcome: std::result::Result<JobSpecJson, String>,
    },
}

/// Spawn child processes without a console window.
///
/// The app is built with `windows_subsystem = "windows"`, so it owns no
/// console; without this flag Windows allocates a fresh console window for
/// every child, and a batch of plan generations flashes up a terminal each.
#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut Command) {}

/// Run the CLI once and return stdout. `cwd` sets the working directory so
/// Claude Code can read a cloned repository's files.
pub fn run_cli(cfg: &CliConfig, prompt: &str, cwd: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new(&cfg.claude_path);
    hide_console(&mut cmd);
    cmd.arg(&cfg.prompt_flag).arg(prompt);
    for arg in cfg.extra_args.split_whitespace() {
        cmd.arg(arg);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().map_err(|e| {
        anyhow!(
            "could not run '{}': {e}. Install the Claude CLI or set its path in Settings.",
            cfg.claude_path
        )
    })?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} exited with {}: {}",
            cfg.claude_path,
            output.status,
            err.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// JSON extraction and repair
// ---------------------------------------------------------------------------

/// Pull the first balanced JSON object out of a CLI response, tolerating
/// ```json fences and surrounding prose.
pub fn extract_json(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let start = raw.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The *last* balanced JSON object in a response — the verification prompt asks
/// for a trailing JSON block after the human-readable report.
pub fn extract_last_json(raw: &str) -> Option<&str> {
    let mut best: Option<&str> = None;
    let mut search_from = 0usize;
    while let Some(rel) = raw[search_from..].find('{') {
        let abs = search_from + rel;
        if let Some(found) = extract_json(&raw[abs..]) {
            best = Some(found);
            search_from = abs + found.len();
        } else {
            search_from = abs + 1;
        }
        if search_from >= raw.len() {
            break;
        }
    }
    best
}

fn parse_json<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T> {
    let slice = extract_json(raw).ok_or_else(|| anyhow!("no JSON object found in CLI output"))?;
    Ok(serde_json::from_str(slice)?)
}

/// Run a prompt expecting JSON; on a parse failure, retry once with a reminder.
fn cli_json<T: serde::de::DeserializeOwned>(
    cfg: &CliConfig,
    prompt: &str,
) -> std::result::Result<(T, String), (String, String)> {
    let raw = match run_cli(cfg, prompt, None) {
        Ok(r) => r,
        Err(e) => return Err((e.to_string(), String::new())),
    };
    match parse_json::<T>(&raw) {
        Ok(v) => Ok((v, raw)),
        Err(first_err) => {
            let retry_prompt = format!(
                "{prompt}\n\nIMPORTANT: your previous answer could not be parsed ({first_err}). \
                 Return valid JSON only - no prose, no markdown fences."
            );
            let raw2 = match run_cli(cfg, &retry_prompt, None) {
                Ok(r) => r,
                Err(e) => return Err((e.to_string(), raw)),
            };
            match parse_json::<T>(&raw2) {
                Ok(v) => Ok((v, raw2)),
                Err(e) => Err((format!("could not parse CLI JSON: {e}"), raw2)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

const PLAN_PROMPT: &str = r#"You are a technical mentor applying Josh Kaufman's "The First 20 Hours" method, training the learner to be hands-on and interview-ready in {TECH}. Deconstruct {TECH} into the highest-leverage subskills (the 20% that delivers 80% of real-world capability), then sequence them into a day-by-day plan of {MINUTES_PER_DAY}-minute sessions totaling {TARGET_HOURS} hours ({DAY_COUNT} days), grouped into 7-day weeks, each week ending in an applied project.

{PRIOR_SUBJECTS}

{TARGET_ROLE}

Hard requirements: (1) "difficulty" (1-5) must be non-decreasing day over day - start foundational, end at interview/systems level. Week 1 = foundation (1-2), week 2 = applied/integration (2-3), week 3 = advanced/edge-cases (4), final days = interview-grade/systems (5). (2) From Week 2 onward each day must reuse earlier subskills - list them in "builds_on" - so complexity compounds instead of resetting. (3) Every day's "practice_task" is a concrete coding/build task, never "read about X". (4) Every day includes 2-4 realistic "interview_questions" with the key points of a strong answer. (5) Weekly projects grow in scope week over week.

Output ONLY valid JSON matching this schema, nothing else:
{
  "tech": string,
  "target_hours": number,
  "minutes_per_day": number,
  "subskills": [{ "name": string, "why": string, "priority": number }],
  "days": [{ "day": number, "week": number, "title": string, "subskill": string,
            "difficulty": number, "builds_on": [string],
            "objectives": [string], "practice_task": string, "est_minutes": number,
            "interview_questions": [{ "q": string, "key_points": [string] }] }],
  "weekly_projects": [{ "week": number, "title": string, "description": string,
                        "acceptance_criteria": [string] }]
}"#;

/// `builds_on` is the subjects the learner already cleared earlier on their
/// roadmap. Naming them is what keeps a track coherent: Spring Boot should
/// lean on the Java already done rather than teach it again.
pub fn prompt_plan(
    tech: &str,
    target_hours: i64,
    minutes_per_day: i64,
    builds_on: &[String],
    role: &str,
) -> String {
    let day_count = day_count(target_hours, minutes_per_day);
    let target = if role.trim().is_empty() {
        "The learner has not named a target role, so aim at general professional \
         competence and the interviews that go with it."
            .to_string()
    } else {
        format!(
            "The learner is training to be hired as: {}. Choose the subskills, the weekly \
             projects and the interview questions that this role is actually hired to do - \
             what a real interview for it would probe, and what the job would have them build \
             in their first months. Skip anything that would not come up.",
            role.trim()
        )
    };
    let prior = if builds_on.is_empty() {
        "This is the first subject on the learner's track, so assume no ground \
         has been covered yet."
            .to_string()
    } else {
        format!(
            "The learner has already worked through these subjects on the same track, in \
             this order: {}. Treat that as known ground - build directly on it, say \
             explicitly where {tech} extends or depends on it, and do not spend days \
             re-teaching it.",
            builds_on.join(", ")
        )
    };
    PLAN_PROMPT
        .replace("{TECH}", tech)
        .replace("{MINUTES_PER_DAY}", &minutes_per_day.to_string())
        .replace("{TARGET_HOURS}", &target_hours.to_string())
        .replace("{DAY_COUNT}", &day_count.to_string())
        .replace("{PRIOR_SUBJECTS}", &prior)
        .replace("{TARGET_ROLE}", &target)
}

const JOB_PROMPT: &str = r#"Read this job posting and work out what somebody would have to learn to be hired for it.

Output ONLY valid JSON matching this schema, nothing else:
{
  "role": string,
  "seniority": string,
  "skills": [ { "name": string, "why": string, "priority": number, "must_have": boolean } ]
}

Rules: "role" is the job title, normalised (e.g. "Java Backend Developer"). "seniority" is one of intern, junior, mid, senior. Each "name" is the bare technology or subject on its own - "Spring Boot", never "strong Spring Boot experience". Order "skills" in the order they should be learned: foundations first, then what depends on them; "priority" counts up from 1 in that same order. Set "must_have" true only where the posting treats the skill as required rather than nice to have. Return at most 12 skills - the ones that actually decide whether somebody gets this job. "why" is one line on what the role uses it for.

The posting follows.
---
{JOB_DESCRIPTION}"#;

pub fn prompt_job_skills(description: &str) -> String {
    JOB_PROMPT.replace("{JOB_DESCRIPTION}", description.trim())
}

/// Read a job posting into a skill list on a worker thread.
pub fn spawn_job_skills(tx: Sender<JobResult>, cfg: CliConfig, description: String) {
    std::thread::spawn(move || {
        let outcome = cli_json::<JobSpecJson>(&cfg, &prompt_job_skills(&description))
            .map(|(spec, _)| spec)
            .map_err(|(e, _)| e);
        let _ = tx.send(JobResult::JobSkills { outcome });
    });
}

/// Days in a plan: ceil(target_hours * 60 / minutes_per_day).
pub fn day_count(target_hours: i64, minutes_per_day: i64) -> i64 {
    let per_day = minutes_per_day.max(1);
    (target_hours * 60 + per_day - 1) / per_day
}

const DAY_PROMPT: &str = r#"You are a technical mentor training a hands-on, interview-ready engineer. Write a focused {EST_MINUTES}-minute code-first lesson in Markdown for Day {DAY} of learning {TECH} at difficulty {DIFFICULTY}/5. Subskill: "{SUBSKILL}". This day builds on: {BUILDS_ON} - assume the learner already knows those and go deeper; do not re-teach basics already covered. Objectives: {OBJECTIVES}.

Structure: (1) concept in brief, (2) a substantial worked example with real code at this difficulty, (3) common pitfalls and edge cases, (4) the hands-on task to complete: "{PRACTICE_TASK}", (5) an "Interview check" section with these questions and strong-answer key points: {INTERVIEW_QUESTIONS}.

Match the depth to difficulty {DIFFICULTY} - higher means more edge cases, performance, trade-offs, and system-level reasoning. Return Markdown only."#;

pub fn prompt_day(tech: &str, day: &Day) -> String {
    let builds_on = if day.builds_on.is_empty() {
        "(nothing yet - this is a foundation day)".to_string()
    } else {
        day.builds_on.join(", ")
    };
    let objectives = if day.objectives.is_empty() {
        "(none supplied)".to_string()
    } else {
        day.objectives.join("; ")
    };
    let questions = if day.interview_questions.is_empty() {
        "(generate 2-3 suitable ones)".to_string()
    } else {
        day.interview_questions
            .iter()
            .map(|q| q.q.clone())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    DAY_PROMPT
        .replace("{EST_MINUTES}", &day.est_minutes.to_string())
        .replace("{DAY}", &day.day_number.to_string())
        .replace("{TECH}", tech)
        .replace("{DIFFICULTY}", &day.difficulty.to_string())
        .replace("{SUBSKILL}", &day.subskill)
        .replace("{BUILDS_ON}", &builds_on)
        .replace("{OBJECTIVES}", &objectives)
        .replace("{PRACTICE_TASK}", &day.practice_task)
        .replace("{INTERVIEW_QUESTIONS}", &questions)
}

const QUIZ_PROMPT: &str = r#"Generate a {N}-question multiple-choice recall quiz on {SCOPE_CONTENT} for {TECH}. Each question has 4 options, one correct, and a one-line explanation. Bias toward concepts a learner forgets and interviewers probe. Output ONLY JSON:
{ "questions": [ { "question": string, "options": [string,string,string,string], "correct_index": number, "explanation": string } ] }"#;

pub fn prompt_quiz(tech: &str, scope_content: &str, n: i64) -> String {
    QUIZ_PROMPT
        .replace("{N}", &n.to_string())
        .replace("{SCOPE_CONTENT}", scope_content)
        .replace("{TECH}", tech)
}

const VERIFY_PROMPT: &str = r#"You are a senior code reviewer. Review this repository against what the learner just studied this week: {WEEK_SUBSKILLS} and the project requirements: {PROJECT_DESCRIPTION} / acceptance criteria: {ACCEPTANCE}.

Find real bugs, security issues, bad practices, and gaps vs. the acceptance criteria. Give concrete, actionable suggested changes referencing files/lines where possible. Write the review as Markdown.

End your answer with a single JSON block and nothing after it:
{"verdict":"pass|needs_work|fail","score":0-100,"issues":[{"severity":"high|med|low","file":string,"line":number,"issue":string,"suggestion":string}]}"#;

pub fn prompt_verify(week_subskills: &str, description: &str, acceptance: &str) -> String {
    VERIFY_PROMPT
        .replace("{WEEK_SUBSKILLS}", week_subskills)
        .replace("{PROJECT_DESCRIPTION}", description)
        .replace("{ACCEPTANCE}", acceptance)
}

// ---------------------------------------------------------------------------
// Complexity ramp enforcement
// ---------------------------------------------------------------------------

/// Guarantee the 20-hour-rule ramp in code, whatever the model returned:
/// clamp difficulty to 1..=5, stable-sort days by difficulty, then renumber
/// days and weeks sequentially. Weekly projects are deduped and re-keyed so the
/// week numbers line up with the renumbered days.
pub fn enforce_ramp(plan: &mut PlanJson) {
    for day in &mut plan.days {
        day.difficulty = day.difficulty.clamp(1, 5);
        if day.est_minutes <= 0 {
            day.est_minutes = 60;
        }
    }
    plan.days.sort_by_key(|d| d.difficulty); // stable: ties keep CLI order
    for (idx, day) in plan.days.iter_mut().enumerate() {
        day.day = idx as i64 + 1;
        day.week = (idx as i64 / 7) + 1;
    }

    let weeks = plan.days.last().map(|d| d.week).unwrap_or(0);
    plan.weekly_projects.sort_by_key(|w| w.week);
    plan.weekly_projects.dedup_by_key(|w| w.week);
    plan.weekly_projects.retain(|w| w.week >= 1 && w.week <= weeks);
    // Fill any week the model skipped so every week has a project row.
    for week in 1..=weeks {
        if !plan.weekly_projects.iter().any(|w| w.week == week) {
            plan.weekly_projects.push(WeeklyProjectJson {
                week,
                title: format!("Week {week} project"),
                description:
                    "Apply this week's subskills in a single working build of your own design."
                        .to_string(),
                acceptance_criteria: vec![
                    "Runs end to end".to_string(),
                    "Uses every subskill studied this week".to_string(),
                ],
            });
        }
    }
    plan.weekly_projects.sort_by_key(|w| w.week);
}

/// Subskills studied in a given week, for the verification prompt.
pub fn week_subskills(days: &[Day], week: i64) -> String {
    let names: Vec<String> = days
        .iter()
        .filter(|d| d.week_number == week)
        .map(|d| format!("{} ({})", d.subskill, d.title))
        .collect();
    if names.is_empty() {
        "(no days recorded for this week)".to_string()
    } else {
        names.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Background jobs
// ---------------------------------------------------------------------------

pub fn spawn_plan(tx: Sender<JobResult>, cfg: CliConfig, req: PlanRequest) {
    std::thread::spawn(move || {
        let PlanRequest {
            stack_id,
            tech,
            target_hours,
            minutes_per_day,
            builds_on,
            role,
        } = req;
        let prompt = prompt_plan(&tech, target_hours, minutes_per_day, &builds_on, &role);
        let result = match cli_json::<PlanJson>(&cfg, &prompt) {
            Ok((mut plan, raw)) => {
                enforce_ramp(&mut plan);
                JobResult::Plan {
                    stack_id,
                    tech,
                    outcome: Ok(plan),
                    raw,
                }
            }
            Err((err, raw)) => JobResult::Plan {
                stack_id,
                tech,
                outcome: Err(err),
                raw,
            },
        };
        let _ = tx.send(result);
    });
}

pub fn spawn_day_content(tx: Sender<JobResult>, cfg: CliConfig, day_id: i64, prompt: String) {
    std::thread::spawn(move || {
        let outcome = run_cli(&cfg, &prompt, None)
            .map(|md| md.trim().to_string())
            .map_err(|e| e.to_string());
        let _ = tx.send(JobResult::DayContent { day_id, outcome });
    });
}

pub fn spawn_quiz(
    tx: Sender<JobResult>,
    cfg: CliConfig,
    plan_id: i64,
    scope: String,
    day_id: Option<i64>,
    week_number: Option<i64>,
    prompt: String,
) {
    std::thread::spawn(move || {
        let outcome = cli_json::<QuizJson>(&cfg, &prompt)
            .map(|(q, _)| q)
            .map_err(|(e, _)| e);
        let _ = tx.send(JobResult::Quiz {
            plan_id,
            scope,
            day_id,
            week_number,
            outcome,
        });
    });
}

/// A cheap round trip so a misconfigured path surfaces before a long job.
pub fn spawn_cli_test(tx: Sender<JobResult>, cfg: CliConfig) {
    std::thread::spawn(move || {
        let outcome = run_cli(&cfg, "Reply with the single word: ready", None)
            .map(|out| out.trim().chars().take(60).collect::<String>())
            .map_err(|e| e.to_string());
        let _ = tx.send(JobResult::CliTest { outcome });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(day: i64, difficulty: i64) -> DayJson {
        DayJson {
            day,
            week: 1,
            title: format!("Day {day}"),
            difficulty,
            est_minutes: 60,
            ..Default::default()
        }
    }

    #[test]
    fn ramp_is_monotonic_after_enforcement() {
        let mut plan = PlanJson {
            days: vec![day(1, 4), day(2, 1), day(3, 9), day(4, 2), day(5, 0)],
            ..Default::default()
        };
        enforce_ramp(&mut plan);

        let difficulties: Vec<i64> = plan.days.iter().map(|d| d.difficulty).collect();
        assert_eq!(difficulties, vec![1, 1, 2, 4, 5], "out-of-range values clamp to 1..=5");
        assert!(difficulties.windows(2).all(|w| w[0] <= w[1]));

        let numbers: Vec<i64> = plan.days.iter().map(|d| d.day).collect();
        assert_eq!(numbers, vec![1, 2, 3, 4, 5], "days are renumbered sequentially");
    }

    #[test]
    fn weeks_are_seven_days_and_every_week_gets_a_project() {
        let mut plan = PlanJson {
            days: (1..=20).map(|i| day(i, 1 + i / 7)).collect(),
            ..Default::default()
        };
        enforce_ramp(&mut plan);

        assert_eq!(plan.days[0].week, 1);
        assert_eq!(plan.days[6].week, 1);
        assert_eq!(plan.days[7].week, 2);
        assert_eq!(plan.days[19].week, 3);
        let weeks: Vec<i64> = plan.weekly_projects.iter().map(|w| w.week).collect();
        assert_eq!(weeks, vec![1, 2, 3]);
    }

    #[test]
    fn json_survives_fences_and_surrounding_prose() {
        let raw = "Sure! Here is the plan:\n```json\n{\"tech\":\"Java\",\"days\":[]}\n```\nHope that helps.";
        let slice = extract_json(raw).expect("object found");
        let parsed: PlanJson = serde_json::from_str(slice).expect("parses");
        assert_eq!(parsed.tech, "Java");
    }

    #[test]
    fn verdict_block_is_taken_from_the_end_of_a_review() {
        let raw = r#"# Review
The `{}` placeholder in Foo.java is wrong.
{"verdict":"needs_work","score":62,"issues":[{"severity":"high","file":"A.java","line":12,"issue":"NPE","suggestion":"guard"}]}"#;
        let slice = extract_last_json(raw).expect("trailing object");
        let parsed: VerificationJson = serde_json::from_str(slice).expect("parses");
        assert_eq!(parsed.verdict, "needs_work");
        assert_eq!(parsed.score, 62);
        assert_eq!(parsed.issues.len(), 1);
    }

    #[test]
    fn day_count_rounds_up() {
        assert_eq!(day_count(20, 60), 20);
        assert_eq!(day_count(20, 90), 14);
        assert_eq!(day_count(20, 45), 27);
    }

    #[test]
    fn the_plan_prompt_names_the_job_being_trained_for() {
        let unaimed = prompt_plan("Java", 20, 60, &[], "");
        assert!(
            unaimed.contains("not named a target role"),
            "with no role it still says so rather than leaving a placeholder"
        );
        assert!(!unaimed.contains("{TARGET_ROLE}"));

        let aimed = prompt_plan("Java", 20, 60, &[], "  Java Backend Developer  ");
        assert!(
            aimed.contains("hired as: Java Backend Developer"),
            "the role is named, trimmed"
        );
    }

    #[test]
    fn the_job_prompt_carries_the_posting_and_asks_for_json() {
        let prompt = prompt_job_skills("  We need Java and Spring Boot.  ");
        assert!(prompt.contains("We need Java and Spring Boot."));
        assert!(prompt.contains("must_have"), "the schema is spelled out");
        assert!(
            !prompt.contains("{JOB_DESCRIPTION}"),
            "the placeholder is filled in"
        );
    }

    #[test]
    fn a_plan_prompt_names_what_the_track_already_cleared() {
        let first = prompt_plan("Java", 20, 60, &[], "");
        assert!(
            first.contains("first subject on the learner"),
            "a track opener says there is no prior ground"
        );
        assert!(!first.contains("already worked through"));

        let later = prompt_plan(
            "Spring Boot",
            20,
            60,
            &["Java".to_string(), "MySQL".to_string()],
            "",
        );
        assert!(
            later.contains("Java, MySQL"),
            "earlier subjects are listed in track order"
        );
        assert!(
            later.contains("do not spend days"),
            "and the course is told not to re-teach them"
        );
    }
}

/// Exercises the real `claude` binary end to end: prompt -> CLI -> JSON ->
/// ramp enforcement. Ignored by default so the suite stays offline; run with
/// `cargo test -- --ignored --nocapture`.
#[cfg(test)]
mod cli_integration {
    use super::*;

    #[test]
    #[ignore]
    fn plan_prompt_round_trips_through_the_real_cli() {
        let cfg = CliConfig::default();
        let prompt = prompt_plan("Redis", 4, 60, &[], "");
        let raw = run_cli(&cfg, &prompt, None).expect("claude CLI ran");
        let slice = extract_json(&raw).expect("a JSON object in the reply");
        let mut plan: PlanJson = serde_json::from_str(slice).expect("parses as a plan");

        assert!(!plan.days.is_empty(), "the plan has days");
        assert!(!plan.subskills.is_empty(), "the plan has subskills");
        enforce_ramp(&mut plan);

        let difficulties: Vec<i64> = plan.days.iter().map(|d| d.difficulty).collect();
        assert!(
            difficulties.windows(2).all(|w| w[0] <= w[1]),
            "difficulty never decreases: {difficulties:?}"
        );
        assert!(
            plan.days.iter().all(|d| !d.practice_task.trim().is_empty()),
            "every day has a hands-on task"
        );
        println!(
            "{} days, difficulties {:?}, {} weekly projects",
            plan.days.len(),
            difficulties,
            plan.weekly_projects.len()
        );
    }
}

/// The job-posting path against the real `claude` binary: posting -> skills
/// JSON -> subjects this app can actually teach. Ignored by default; run with
/// `cargo test -- --ignored --nocapture a_job_posting`.
#[cfg(test)]
mod job_integration {
    use super::*;
    use crate::job;
    use crate::models::CATALOGUE;

    const POSTING: &str = "Backend Engineer (Java) - Bengaluru, hybrid\n\nWe are looking for a backend engineer to join the payments platform team. You will design and ship REST services that move real money, and own them in production.\n\nWhat you will do:\n- Build and maintain microservices in Java 17 and Spring Boot 3\n- Model and query data in PostgreSQL, and keep queries fast under load\n- Cache hot paths with Redis and publish events to Kafka\n- Containerise services with Docker and ship them to Kubernetes\n- Keep the CI/CD pipeline green in GitHub Actions\n\nWhat we are looking for:\n- 2+ years of professional Java, strong OOP and collections\n- Solid grasp of data structures and algorithms\n- Experience with Hibernate/JPA\n- Comfortable on Linux and with Git\n- Nice to have: gRPC, AWS, observability tooling";

    #[test]
    #[ignore]
    fn a_job_posting_becomes_subjects_this_app_can_teach() {
        let cfg = CliConfig::default();
        let raw = run_cli(&cfg, &prompt_job_skills(POSTING), None).expect("claude CLI ran");
        let slice = extract_json(&raw).expect("a JSON object in the reply");
        let spec: JobSpecJson = serde_json::from_str(slice).expect("parses as a job spec");

        println!("role      : {} ({})", spec.role, spec.seniority);
        println!("skills    : {}", spec.skills.len());
        assert!(!spec.role.trim().is_empty(), "the posting has a role");
        assert!(!spec.skills.is_empty(), "and named skills");
        assert!(spec.skills.len() <= 12, "capped at 12, got {}", spec.skills.len());

        let known: Vec<String> = CATALOGUE
            .iter()
            .flat_map(|(_, names)| names.iter().map(|n| (*n).to_string()))
            .collect();

        let mut ordered = spec.skills.clone();
        ordered.sort_by_key(|s| s.priority);
        let mut matched = 0;
        for skill in &ordered {
            match job::match_subject(&skill.name, &known) {
                Some(subject) => {
                    matched += 1;
                    println!("  {:>2}. {:28} -> {}", skill.priority, skill.name, subject);
                }
                None => println!(
                    "  {:>2}. {:28} -> (custom: {})",
                    skill.priority,
                    skill.name,
                    job::custom_subject_name(&skill.name)
                ),
            }
        }
        assert!(
            matched * 2 >= ordered.len(),
            "most of a mainstream Java posting should map onto the catalogue, \
             matched {matched} of {}",
            ordered.len()
        );
    }
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    /// The no-console flag must not interfere with launching a child or
    /// capturing its stdout.
    #[test]
    fn hidden_child_still_runs_and_its_output_is_captured() {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/c").arg("echo").arg("hello");
            c
        } else {
            let mut c = Command::new("echo");
            c.arg("hello");
            c
        };
        hide_console(&mut cmd);

        let output = cmd.output().expect("child ran");
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "hello",
            "stdout is still captured with the console hidden"
        );
    }
}
