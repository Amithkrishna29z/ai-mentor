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

/// Per-item progress for a roadmap plus the index of the subject to work on next.
pub struct RoadmapProgress {
    /// `(item_id, subject_name, stack_id, days_done, days_total, unlocked)`
    pub items: Vec<(i64, String, i64, i64, i64, bool)>,
    pub current: Option<usize>,
    pub overall: f32,
}

pub fn progress(db: &Db, roadmap_id: i64) -> Result<RoadmapProgress> {
    let threshold = db.setting_i64("roadmap_unlock_threshold", DEFAULT_UNLOCK_THRESHOLD);
    let mut items = Vec::new();
    let mut current = None;
    let mut done_sum = 0.0f32;
    let mut unlocked = true;

    for (idx, item) in db.roadmap_items(roadmap_id)?.into_iter().enumerate() {
        let (done, total) = subject_progress(db, item.tech_stack_id)?;
        let frac = fraction(done, total);
        done_sum += frac;
        let cleared = total > 0 && (frac * 100.0) as i64 >= threshold;
        items.push((
            item.id,
            item.tech_stack_name,
            item.tech_stack_id,
            done,
            total,
            unlocked,
        ));
        if !cleared && current.is_none() {
            current = Some(idx);
        }
        // Everything after the first uncleared subject is still locked.
        if !cleared {
            unlocked = false;
        }
    }

    let overall = if items.is_empty() {
        0.0
    } else {
        done_sum / items.len() as f32
    };
    Ok(RoadmapProgress {
        items,
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
