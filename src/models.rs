//! Data structures: SQLite row models plus the serde types used to parse Claude CLI output.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Row models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TechStack {
    pub id: i64,
    pub name: String,
    pub selected: bool,
    pub is_custom: bool,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub id: i64,
    pub tech_stack_id: i64,
    pub target_hours: i64,
    pub minutes_per_day: i64,
    pub plan_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayStatus {
    NotStarted,
    InProgress,
    Done,
}

impl DayStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "done" => Self::Done,
            _ => Self::NotStarted,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::NotStarted => "Not started",
            Self::InProgress => "In progress",
            Self::Done => "Done",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Day {
    pub id: i64,
    pub plan_id: i64,
    pub day_number: i64,
    pub week_number: i64,
    pub title: String,
    pub subskill: String,
    pub objectives: Vec<String>,
    pub practice_task: String,
    pub difficulty: i64,
    pub builds_on: Vec<String>,
    pub interview_questions: Vec<InterviewQuestion>,
    pub content_md: String,
    pub est_minutes: i64,
    pub status: DayStatus,
    pub hands_on_done: bool,
    pub notes: String,
    pub content_edited: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterviewQuestion {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub key_points: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WeeklyProject {
    pub id: i64,
    pub plan_id: i64,
    pub week_number: i64,
    pub title: String,
    pub description: String,
    pub acceptance: Vec<String>,
    pub status: String,
    pub github_url: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct Verification {
    pub id: i64,
    pub weekly_project_id: i64,
    pub github_url: String,
    pub commit_sha: String,
    pub verdict: String,
    pub score: i64,
    pub report_md: String,
    pub issues: Vec<Issue>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Issue {
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub issue: String,
    #[serde(default)]
    pub suggestion: String,
}

#[derive(Debug, Clone)]
pub struct Roadmap {
    pub id: i64,
    pub name: String,
    pub is_preset: bool,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct RoadmapItem {
    pub id: i64,
    pub roadmap_id: i64,
    pub tech_stack_id: i64,
    pub tech_stack_name: String,
    pub position: i64,
}

#[derive(Debug, Clone)]
pub struct Quiz {
    pub id: i64,
    pub plan_id: i64,
    pub scope: String,
    pub day_id: Option<i64>,
    pub week_number: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct QuizQuestion {
    pub id: i64,
    pub quiz_id: i64,
    pub question: String,
    pub options: Vec<String>,
    pub correct_index: i64,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct ReviewCard {
    pub id: i64,
    pub source_type: String,
    pub source_ref: Option<i64>,
    pub plan_id: Option<i64>,
    pub front: String,
    pub back: String,
    pub easiness: f64,
    pub interval_days: i64,
    pub repetitions: i64,
    pub due_date: String,
    pub last_grade: Option<i64>,
}

// ---------------------------------------------------------------------------
// Claude CLI JSON payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanJson {
    #[serde(default)]
    pub tech: String,
    #[serde(default)]
    pub target_hours: f64,
    #[serde(default)]
    pub minutes_per_day: f64,
    #[serde(default)]
    pub subskills: Vec<SubskillJson>,
    #[serde(default)]
    pub days: Vec<DayJson>,
    #[serde(default)]
    pub weekly_projects: Vec<WeeklyProjectJson>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubskillJson {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub priority: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DayJson {
    #[serde(default)]
    pub day: i64,
    #[serde(default)]
    pub week: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subskill: String,
    #[serde(default = "default_difficulty")]
    pub difficulty: i64,
    #[serde(default)]
    pub builds_on: Vec<String>,
    #[serde(default)]
    pub objectives: Vec<String>,
    #[serde(default)]
    pub practice_task: String,
    #[serde(default = "default_minutes")]
    pub est_minutes: i64,
    #[serde(default)]
    pub interview_questions: Vec<InterviewQuestion>,
}

fn default_difficulty() -> i64 {
    1
}

fn default_minutes() -> i64 {
    60
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeeklyProjectJson {
    #[serde(default)]
    pub week: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuizJson {
    #[serde(default)]
    pub questions: Vec<QuizQuestionJson>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuizQuestionJson {
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub correct_index: i64,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationJson {
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub score: i64,
    #[serde(default)]
    pub issues: Vec<Issue>,
}

// ---------------------------------------------------------------------------
// Seed catalogue
// ---------------------------------------------------------------------------

/// The built-in subject catalogue. Category lives in code rather than in the
/// schema (the schema is fixed by the spec); custom subjects fall under "Custom".
pub const CATALOGUE: &[(&str, &[&str])] = &[
    (
        "Backend",
        &[
            "Java",
            "Spring Boot",
            "Spring Security",
            "Hibernate / JPA",
            ".NET / C#",
            "Node.js",
            "FastAPI",
        ],
    ),
    ("Database", &["MySQL", "PostgreSQL", "Redis", "MongoDB"]),
    (
        "Frontend",
        &[
            "HTML & CSS",
            "JavaScript",
            "TypeScript",
            "React",
            "Angular",
            "Tailwind CSS",
        ],
    ),
    (
        "DevOps",
        &[
            "Git",
            "Docker",
            "Kubernetes",
            "CI/CD (GitHub Actions)",
            "Linux",
            "Terraform",
        ],
    ),
    ("Cloud", &["AWS", "Azure", "GCP"]),
    (
        "Foundations",
        &["Data Structures & Algorithms", "System Design", "Kafka"],
    ),
];

pub const PRESET_ROADMAP_NAME: &str = "Full-Stack Java Spring Boot Developer";

/// Ordered subjects of the preset roadmap.
pub const PRESET_ROADMAP: &[&str] = &[
    "Java",
    "Data Structures & Algorithms",
    "Spring Boot",
    "Hibernate / JPA",
    "MySQL",
    "Spring Security",
    "React",
    "TypeScript",
    "HTML & CSS",
    "Git",
    "Docker",
    "CI/CD (GitHub Actions)",
    "Kubernetes",
    "AWS",
    "System Design",
];

/// Category for a subject name; "Custom" when it is not in the catalogue.
pub fn category_of(name: &str) -> &'static str {
    for (cat, names) in CATALOGUE {
        if names.contains(&name) {
            return cat;
        }
    }
    "Custom"
}

pub const CATEGORY_ORDER: &[&str] = &[
    "Backend",
    "Database",
    "Frontend",
    "DevOps",
    "Cloud",
    "Foundations",
    "Custom",
];
