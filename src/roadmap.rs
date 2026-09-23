//! Roadmap seeding (the built-in preset) and sequential progression logic.

use anyhow::Result;
use chrono::Local;
use rusqlite::params;

use crate::db::Db;
use crate::models::{PRESET_ROADMAP, PRESET_ROADMAP_NAME};

/// Percentage of a subject's days that must be `done` before the roadmap
/// treats it as cleared and unlocks the next subject.
pub const DEFAULT_UNLOCK_THRESHOLD: i64 = 80;

/// Insert the preset roadmap and its ordered items once, on first run.
pub fn seed_preset(db: &Db) -> Result<()> {
    let now = Local::now().to_rfc3339();
    db.conn.execute(
        "INSERT OR IGNORE INTO roadmaps (name, is_preset, active, created_at) VALUES (?1, 1, 1, ?2)",
        params![PRESET_ROADMAP_NAME, now],
    )?;
    let roadmap_id: i64 = db.conn.query_row(
        "SELECT id FROM roadmaps WHERE name = ?1",
        params![PRESET_ROADMAP_NAME],
        |r| r.get(0),
    )?;

    for (position, subject) in PRESET_ROADMAP.iter().enumerate() {
        if let Some(stack_id) = db.stack_id_by_name(subject)? {
            db.conn.execute(
                "INSERT OR IGNORE INTO roadmap_items (roadmap_id, tech_stack_id, position)
                 VALUES (?1, ?2, ?3)",
                params![roadmap_id, stack_id, position as i64],
            )?;
        }
    }

    // Make sure exactly one roadmap is active.
    let active: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM roadmaps WHERE active = 1", [], |r| {
            r.get(0)
        })?;
    if active == 0 {
        db.set_active_roadmap(roadmap_id)?;
    }
    Ok(())
}

/// Fraction (0.0..=1.0) of a subject's plan that is complete, by days done.
/// A subject with no generated plan yet is 0.0.
pub fn subject_progress(db: &Db, stack_id: i64) -> Result<(i64, i64)> {
    let Some(plan) = db.plan_for_stack(stack_id)? else {
        return Ok((0, 0));
    };
    let done: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM days WHERE plan_id = ?1 AND status = 'done'",
        params![plan.id],
        |r| r.get(0),
    )?;
    let total: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM days WHERE plan_id = ?1",
        params![plan.id],
        |r| r.get(0),
    )?;
    Ok((done, total))
}

pub fn fraction(done: i64, total: i64) -> f32 {
    if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    }
}

/// One subject on the track, with the progress that gates the next one.
#[derive(Clone)]
pub struct RoadmapEntry {
    pub item_id: i64,
    pub subject: String,
    pub stack_id: i64,
    pub days_done: i64,
    pub days_total: i64,
    /// False while a subject earlier on the track is still short of the
    /// threshold. The track is walked in order, so this is what locks it.
    pub unlocked: bool,
    /// Past the unlock threshold, so the subject after it opens.
    pub cleared: bool,
}

/// Per-item progress for a roadmap plus the index of the subject to work on next.
#[derive(Clone)]
pub struct RoadmapProgress {
    pub entries: Vec<RoadmapEntry>,
    pub current: Option<usize>,
    pub overall: f32,
}

impl RoadmapProgress {
    /// The subject to work on now: the first one not yet cleared.
    pub fn current_entry(&self) -> Option<&RoadmapEntry> {
        self.entries.get(self.current?)
    }

    /// Where a subject sits on the track, if it is on it at all.
    pub fn index_of(&self, stack_id: i64) -> Option<usize> {
        self.entries.iter().position(|e| e.stack_id == stack_id)
    }

    /// Subjects already cleared ahead of `index`. This is the ground a new
    /// course should build on rather than re-teach, which is what chains the
    /// track together instead of leaving each subject to start from scratch.
    pub fn cleared_before(&self, index: usize) -> Vec<String> {
        self.entries
            .iter()
            .take(index)
            .filter(|e| e.cleared)
            .map(|e| e.subject.clone())
            .collect()
    }
}

pub fn progress(db: &Db, roadmap_id: i64) -> Result<RoadmapProgress> {
    let threshold = db.setting_i64("roadmap_unlock_threshold", DEFAULT_UNLOCK_THRESHOLD);
    let mut entries = Vec::new();
    let mut current = None;
    let mut done_sum = 0.0f32;
    let mut unlocked = true;

    for (idx, item) in db.roadmap_items(roadmap_id)?.into_iter().enumerate() {
        let (days_done, days_total) = subject_progress(db, item.tech_stack_id)?;
        let frac = fraction(days_done, days_total);
        done_sum += frac;
        let cleared = days_total > 0 && (frac * 100.0) as i64 >= threshold;
        entries.push(RoadmapEntry {
            item_id: item.id,
            subject: item.tech_stack_name,
            stack_id: item.tech_stack_id,
            days_done,
            days_total,
            unlocked,
            cleared,
        });
        if !cleared && current.is_none() {
            current = Some(idx);
        }
        // Everything after the first uncleared subject is still locked.
        if !cleared {
            unlocked = false;
        }
    }

    let overall = if entries.is_empty() {
        0.0
    } else {
        done_sum / entries.len() as f32
    };
    Ok(RoadmapProgress {
        entries,
        current,
        overall,
    })
}

/// Move an item one slot up or down and renumber positions compactly.
pub fn move_item(db: &Db, roadmap_id: i64, item_id: i64, delta: i64) -> Result<()> {
    let items = db.roadmap_items(roadmap_id)?;
    let Some(pos) = items.iter().position(|i| i.id == item_id) else {
        return Ok(());
    };
    let target = pos as i64 + delta;
    if target < 0 || target >= items.len() as i64 {
        return Ok(());
    }
    let mut order: Vec<i64> = items.iter().map(|i| i.id).collect();
    order.swap(pos, target as usize);
    for (position, id) in order.into_iter().enumerate() {
        db.set_item_position(id, position as i64)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DayJson, DayStatus, PlanJson};

    /// A track of `(subject, days_in_course, days_done)`, in order.
    fn track(spec: &[(&str, i64, i64)]) -> (tempfile::TempDir, Db, i64) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut db = Db::open_at(&tmp.path().join("t.db")).expect("db opens");
        let roadmap_id = db.create_roadmap("test track").expect("roadmap");
        db.set_active_roadmap(roadmap_id).expect("activated");

        for (subject, days, done) in spec {
            let stack_id = db
                .stack_id_by_name(subject)
                .expect("query")
                .unwrap_or_else(|| panic!("{subject} is seeded"));
            db.add_roadmap_item(roadmap_id, stack_id).expect("item added");
            if *days == 0 {
                continue; // a subject with no course generated yet
            }
            let plan = PlanJson {
                days: (1..=*days)
                    .map(|day| DayJson {
                        day,
                        week: 1,
                        difficulty: 1,
                        est_minutes: 60,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            let plan_id = db.save_plan(stack_id, 1, 60, &plan, "{}").expect("plan saved");
            for day in db
                .days_for_plan(plan_id)
                .expect("days")
                .iter()
                .take(*done as usize)
            {
                db.set_day_status(day.id, DayStatus::Done).expect("marked done");
            }
        }
        (tmp, db, roadmap_id)
    }

    #[test]
    fn clearing_a_subject_unlocks_the_next_one() {
        // 8 of 10 days is exactly the default 80% threshold.
        let (_tmp, db, id) = track(&[("Java", 10, 8), ("Spring Boot", 10, 0), ("Docker", 0, 0)]);
        let p = progress(&db, id).expect("progress");

        assert!(p.entries[0].cleared, "Java is at the threshold");
        assert!(p.entries[1].unlocked, "so Spring Boot opens");
        assert!(!p.entries[1].cleared);
        assert!(!p.entries[2].unlocked, "Docker stays behind Spring Boot");
        assert_eq!(p.current, Some(1), "Spring Boot is what to work on now");
        assert_eq!(p.current_entry().map(|e| e.subject.as_str()), Some("Spring Boot"));
    }

    #[test]
    fn a_subject_short_of_the_threshold_keeps_the_next_locked() {
        let (_tmp, db, id) = track(&[("Java", 10, 7), ("Spring Boot", 10, 0)]);
        let p = progress(&db, id).expect("progress");

        assert!(!p.entries[0].cleared, "70% is under the 80% threshold");
        assert!(p.entries[0].unlocked, "the first subject is always open");
        assert!(!p.entries[1].unlocked, "Spring Boot is still locked");
        assert_eq!(p.current, Some(0), "stay on Java");
    }

    #[test]
    fn a_subject_with_no_course_yet_never_counts_as_cleared() {
        let (_tmp, db, id) = track(&[("Java", 0, 0), ("Spring Boot", 10, 0)]);
        let p = progress(&db, id).expect("progress");

        assert!(!p.entries[0].cleared, "zero days is not a cleared subject");
        assert!(!p.entries[1].unlocked);
        assert_eq!(p.current, Some(0));
    }

    #[test]
    fn a_new_course_builds_on_the_subjects_already_cleared() {
        let (_tmp, db, id) = track(&[
            ("Java", 10, 10),
            ("Data Structures & Algorithms", 10, 9),
            ("Spring Boot", 0, 0),
        ]);
        let p = progress(&db, id).expect("progress");

        assert_eq!(p.current, Some(2), "both earlier subjects cleared");
        assert_eq!(
            p.cleared_before(2),
            vec![
                "Java".to_string(),
                "Data Structures & Algorithms".to_string()
            ],
            "Spring Boot is told what came before it, in track order"
        );
        assert!(
            p.cleared_before(0).is_empty(),
            "the first subject builds on nothing"
        );
    }

    #[test]
    fn an_unfinished_subject_is_left_out_of_what_the_next_builds_on() {
        // Java cleared, DSA only half done, then Spring Boot.
        let (_tmp, db, id) = track(&[
            ("Java", 10, 10),
            ("Data Structures & Algorithms", 10, 5),
            ("Spring Boot", 0, 0),
        ]);
        let p = progress(&db, id).expect("progress");

        assert_eq!(p.current, Some(1), "DSA is the one to work on");
        assert_eq!(
            p.cleared_before(2),
            vec!["Java".to_string()],
            "a half-done subject is not claimed as known ground"
        );
    }

    #[test]
    fn index_of_finds_a_subject_on_the_track_and_ignores_one_off_it() {
        let (_tmp, db, id) = track(&[("Java", 10, 10), ("Spring Boot", 10, 0)]);
        let p = progress(&db, id).expect("progress");
        let spring = db.stack_id_by_name("Spring Boot").unwrap().unwrap();
        let redis = db.stack_id_by_name("Redis").unwrap().unwrap();

        assert_eq!(p.index_of(spring), Some(1));
        assert_eq!(p.index_of(redis), None, "Redis is not on this track");
    }
}

