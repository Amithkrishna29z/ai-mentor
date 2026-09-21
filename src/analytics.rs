//! Aggregations for the analytics dashboard: streaks, totals, the activity
//! heatmap grid and the weekly-minutes trend.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use std::collections::BTreeMap;

use crate::db::Db;

pub struct Stats {
    pub current_streak: i64,
    pub longest_streak: i64,
    pub total_minutes: i64,
    pub days_completed: i64,
    pub avg_quiz_score: Option<f64>,
    pub cards_due: i64,
    pub cards_total: i64,
    pub highest_difficulty: i64,
}

/// One cell of the GitHub-style heatmap.
#[derive(Clone, Copy)]
pub struct HeatCell {
    pub date: NaiveDate,
    pub minutes: i64,
}

pub struct Heatmap {
    /// Columns of 7 cells, oldest week first; each column runs Sunday..Saturday.
    pub weeks: Vec<Vec<Option<HeatCell>>>,
    pub max_minutes: i64,
}

fn minutes_map(db: &Db) -> Result<BTreeMap<NaiveDate, i64>> {
    let mut map = BTreeMap::new();
    for (date, minutes) in db.minutes_by_date()? {
        if let Ok(d) = date.parse::<NaiveDate>() {
            *map.entry(d).or_insert(0) += minutes;
        }
    }
    Ok(map)
}

pub fn stats(db: &Db, today: NaiveDate) -> Result<Stats> {
    let map = minutes_map(db)?;
    let active: Vec<NaiveDate> = map
        .iter()
        .filter(|(_, m)| **m > 0)
        .map(|(d, _)| *d)
        .collect();

    // Longest run of consecutive active days.
    let mut longest = 0i64;
    let mut run = 0i64;
    let mut prev: Option<NaiveDate> = None;
    for date in &active {
        run = match prev {
            Some(p) if *date == p + Duration::days(1) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        prev = Some(*date);
    }

    // Current streak counts back from today (or yesterday, if today is idle).
    let mut current = 0i64;
    let mut cursor = if map.get(&today).copied().unwrap_or(0) > 0 {
        today
    } else {
        today - Duration::days(1)
    };
    while map.get(&cursor).copied().unwrap_or(0) > 0 {
        current += 1;
        cursor -= Duration::days(1);
    }

    Ok(Stats {
        current_streak: current,
        longest_streak: longest,
        total_minutes: map.values().sum(),
        days_completed: db.total_days_done()?,
        avg_quiz_score: db.avg_quiz_score()?,
        cards_due: db.due_card_count(&today.to_string())?,
        cards_total: db.total_card_count()?,
        highest_difficulty: db.highest_difficulty_reached()?,
    })
}

/// Build a calendar grid covering the last `weeks` weeks, ending today.
pub fn heatmap(db: &Db, today: NaiveDate, weeks: i64) -> Result<Heatmap> {
    let map = minutes_map(db)?;
    let weeks = weeks.clamp(4, 53);

    // Walk back to the Sunday that starts the window.
    let days_since_sunday = today.weekday().num_days_from_sunday() as i64;
    let last_sunday = today - Duration::days(days_since_sunday);
    let start = last_sunday - Duration::weeks(weeks - 1);

    let mut grid = Vec::new();
    let mut max_minutes = 0i64;
    for w in 0..weeks {
        let mut column = Vec::with_capacity(7);
        for d in 0..7 {
            let date = start + Duration::weeks(w) + Duration::days(d);
            if date > today {
                column.push(None);
            } else {
                let minutes = map.get(&date).copied().unwrap_or(0);
                max_minutes = max_minutes.max(minutes);
                column.push(Some(HeatCell { date, minutes }));
            }
        }
        grid.push(column);
    }

    Ok(Heatmap {
        weeks: grid,
        max_minutes,
    })
}

/// Minutes per ISO week for the trend chart: `(label, minutes)`, oldest first.
pub fn weekly_minutes(db: &Db, today: NaiveDate, weeks: i64) -> Result<Vec<(String, i64)>> {
    let map = minutes_map(db)?;
    let weeks = weeks.clamp(4, 53);
    let start_of_week = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let mut out = Vec::new();

    for w in (0..weeks).rev() {
        let week_start = start_of_week - Duration::weeks(w);
        let total: i64 = (0..7)
            .map(|d| map.get(&(week_start + Duration::days(d))).copied().unwrap_or(0))
            .sum();
        out.push((format!("{}", week_start.format("%d %b")), total));
    }
    Ok(out)
}

pub fn weekday_label(index: usize) -> &'static str {
    match index {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    }
}

/// Month label for a heatmap column, shown when the month changes.
pub fn month_label(date: NaiveDate) -> String {
    date.format("%b").to_string()
}

pub fn is_week_start(date: NaiveDate) -> bool {
    date.weekday() == Weekday::Sun
}
