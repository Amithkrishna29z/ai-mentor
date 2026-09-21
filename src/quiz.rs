//! Recall quizzes: what to quiz on, scoring an attempt, and feeding mistakes
//! into the spaced-repetition queue.

use anyhow::Result;
use serde_json::json;

use crate::db::Db;
use crate::models::{Day, QuizQuestion, WeeklyProject};

/// What a day-scoped quiz covers.
pub fn scope_content_day(day: &Day) -> String {
    let objectives = day.objectives.join("; ");
    format!(
        "Day {} - \"{}\" (subskill: {}). Objectives: {}. Hands-on task: {}",
        day.day_number, day.title, day.subskill, objectives, day.practice_task
    )
}

/// What a week-scoped quiz covers: the week's day titles plus its project.
pub fn scope_content_week(days: &[Day], week: i64, project: Option<&WeeklyProject>) -> String {
    let titles: Vec<String> = days
        .iter()
        .filter(|d| d.week_number == week)
        .map(|d| format!("Day {}: {} ({})", d.day_number, d.title, d.subskill))
        .collect();
    let project_part = project
        .map(|p| format!(" Weekly project: {} - {}", p.title, p.description))
        .unwrap_or_default();
    format!("Week {week}. {}{}", titles.join("; "), project_part)
}

/// Score an attempt, persist it, and turn every wrong answer into a review card.
/// Returns `(correct, total)`.
pub fn score_attempt(
    db: &Db,
    quiz_id: i64,
    plan_id: i64,
    questions: &[QuizQuestion],
    chosen: &[Option<usize>],
) -> Result<(i64, i64)> {
    let mut correct = 0i64;
    let mut answers = Vec::new();

    for (idx, question) in questions.iter().enumerate() {
        let picked = chosen.get(idx).copied().flatten();
        let is_correct = picked == Some(question.correct_index as usize);
        if is_correct {
            correct += 1;
        } else {
            let answer = question
                .options
                .get(question.correct_index as usize)
                .cloned()
                .unwrap_or_default();
            let back = if question.explanation.trim().is_empty() {
                answer
            } else {
                format!("{answer}\n\n{}", question.explanation)
            };
            db.upsert_card(
                "quiz",
                Some(question.id),
                Some(plan_id),
                &question.question,
                &back,
            )?;
        }
        answers.push(json!({
            "question_id": question.id,
            "chosen_index": picked,
            "correct": is_correct,
        }));
    }

    let total = questions.len() as i64;
    db.insert_attempt(quiz_id, correct, total, &json!(answers).to_string())?;
    Ok((correct, total))
}
