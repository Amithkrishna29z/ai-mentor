//! SM-2 spaced repetition scheduling. Pure algorithm - no CLI involved.

use chrono::{Duration, NaiveDate};

use crate::models::ReviewCard;

/// Apply an SM-2 grade (0..=5) to a card, updating its interval, easiness,
/// repetition count and due date in place.
pub fn schedule(card: &mut ReviewCard, grade: i64, today: NaiveDate) {
    let grade = grade.clamp(0, 5);

    if grade >= 3 {
        card.interval_days = match card.repetitions {
            0 => 1,
            1 => 6,
            _ => ((card.interval_days as f64) * card.easiness).round() as i64,
        };
        card.repetitions += 1;
    } else {
        card.repetitions = 0;
        card.interval_days = 1;
    }

    let g = grade as f64;
    card.easiness = (card.easiness + (0.1 - (5.0 - g) * (0.08 + (5.0 - g) * 0.02))).max(1.3);
    card.interval_days = card.interval_days.max(1);
    card.due_date = (today + Duration::days(card.interval_days)).to_string();
    card.last_grade = Some(grade);
}

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> ReviewCard {
        ReviewCard {
            id: 1,
            source_type: "interview".into(),
            source_ref: None,
            plan_id: None,
            front: "q".into(),
            back: "a".into(),
            easiness: 2.5,
            interval_days: 0,
            repetitions: 0,
            due_date: "2026-01-01".into(),
            last_grade: None,
        }
    }

    #[test]
    fn first_three_good_reviews_follow_one_six_then_ef() {
        let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let mut c = card();

        schedule(&mut c, 5, day);
        assert_eq!(c.interval_days, 1);
        assert_eq!(c.repetitions, 1);

        schedule(&mut c, 5, day);
        assert_eq!(c.interval_days, 6);
        assert_eq!(c.repetitions, 2);

        // The third interval uses the easiness factor as it stood *before* this
        // review, matching the SM-2 ordering in the spec.
        let ef_before = c.easiness;
        schedule(&mut c, 5, day);
        assert_eq!(c.interval_days, (6.0 * ef_before).round() as i64);
    }

    #[test]
    fn a_lapse_resets_repetitions_and_interval() {
        let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let mut c = card();
        schedule(&mut c, 5, day);
        schedule(&mut c, 5, day);
        schedule(&mut c, 1, day);
        assert_eq!(c.repetitions, 0);
        assert_eq!(c.interval_days, 1);
        assert_eq!(c.due_date, "2026-01-02");
    }

    #[test]
    fn easiness_never_drops_below_floor() {
        let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let mut c = card();
        for _ in 0..20 {
            schedule(&mut c, 0, day);
        }
        assert!(c.easiness >= 1.3);
    }
}
