//! Pure scheduling policy. No operating-system calls, miner processes or GUI.
//! This exact module is tested by rustc --test before the Windows build.
use std::collections::{HashMap, HashSet};

pub type ActiveOccurrences = HashMap<String, HashSet<String>>;
pub type StopSnapshot = HashMap<String, HashSet<String>>;

#[derive(Clone, Copy, Debug)]
pub struct LocalMoment {
    /// Local calendar day number, not a UTC timestamp. Only differences matter.
    pub day: i32,
    /// ISO weekday: Monday=1, Sunday=7.
    pub weekday: u32,
    pub second: u32,
}

#[derive(Clone, Copy)]
pub struct Window<'a> {
    pub id: &'a str,
    pub enabled: bool,
    pub days: &'a [u32],
    pub start: &'a str,
    pub end: &'a str,
    pub miners: &'a [String],
}

fn parse_second(text: &str) -> Option<u32> {
    let (hour, minute) = text.split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    if hour > 23 || minute > 59 { return None; }
    Some(hour * 3600 + minute * 60)
}

/// Identify the *dated occurrence*, including the previous start day for an
/// overnight window. A saved Stop for Friday must never suppress Saturday.
pub fn active_occurrence(window: Window<'_>, now: LocalMoment) -> Option<String> {
    if !window.enabled || !(1..=7).contains(&now.weekday) || now.second >= 86400 {
        return None;
    }
    let start = parse_second(window.start)?;
    let end = parse_second(window.end)?;
    let start_day = if start <= end {
        if !window.days.contains(&now.weekday) || now.second < start || now.second >= end {
            return None;
        }
        now.day
    } else if now.second >= start && window.days.contains(&now.weekday) {
        now.day
    } else {
        let previous_weekday = if now.weekday == 1 { 7 } else { now.weekday - 1 };
        if now.second >= end || !window.days.contains(&previous_weekday) { return None; }
        now.day.checked_sub(1)?
    };
    Some(format!("{}@{}", window.id, start_day))
}


/// Only the scheduler owns sessions it started itself. A user-started session is
/// an explicit override and must survive the end of a schedule window.
pub fn scheduler_should_stop_session(running: bool, started_by: &str, scheduled_now: bool) -> bool {
    running && started_by == "schedule" && !scheduled_now
}

pub fn active_miners<'a>(windows: impl IntoIterator<Item = Window<'a>>, now: LocalMoment) -> ActiveOccurrences {
    let mut active = ActiveOccurrences::new();
    for window in windows {
        if let Some(occurrence) = active_occurrence(window, now) {
            for id in window.miners {
                active.entry(id.clone()).or_default().insert(occurrence.clone());
            }
        }
    }
    active
}

/// Manual Stop has precedence for all schedule occurrences active at the moment
/// of the click. New days/new windows are not permanently disabled. Overlapping
/// windows cannot undo a Stop while any of the stopped occurrences is active.
#[derive(Debug, Default, Clone)]
pub struct ManualScheduleControl {
    stopped: StopSnapshot,
}

impl ManualScheduleControl {
    pub fn from_snapshot(stopped: StopSnapshot) -> Self { Self { stopped } }
    pub fn snapshot(&self) -> &StopSnapshot { &self.stopped }

    pub fn pause(&mut self, id: &str, active: &ActiveOccurrences) -> bool {
        let Some(occurrences) = active.get(id).filter(|x| !x.is_empty()) else { return false; };
        if self.stopped.get(id) == Some(occurrences) { return false; }
        self.stopped.insert(id.to_owned(), occurrences.clone());
        true
    }

    pub fn pause_all(&mut self, active: &ActiveOccurrences) -> bool {
        let mut changed = false;
        // Include eligible but currently stopped/failed profiles, not only live
        // processes: Stop all must also prevent the next automatic retry.
        for id in active.keys() { changed |= self.pause(id, active); }
        changed
    }

    pub fn resume(&mut self, id: &str) -> bool { self.stopped.remove(id).is_some() }

    pub fn is_paused(&self, id: &str, active: &ActiveOccurrences) -> bool {
        match (self.stopped.get(id), active.get(id)) {
            (Some(stopped), Some(current)) => !stopped.is_disjoint(current),
            _ => false,
        }
    }

    pub fn prune(&mut self, active: &ActiveOccurrences) -> bool {
        let before = self.stopped.clone();
        self.stopped.retain(|id, occurrences| {
            if let Some(current) = active.get(id) {
                occurrences.retain(|key| current.contains(key));
                !occurrences.is_empty()
            } else { false }
        });
        before != self.stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now(day: i32, weekday: u32, hour: u32, minute: u32) -> LocalMoment {
        LocalMoment { day, weekday, second: hour * 3600 + minute * 60 }
    }
    fn daily<'a>(id: &'a str, start: &'a str, end: &'a str, miners: &'a [String]) -> Window<'a> {
        Window { id, enabled: true, days: &[1, 2, 3, 4, 5, 6, 7], start, end, miners }
    }
    #[test]
    fn manual_stop_wins_over_repeated_scheduler_ticks() {
        let miners = vec!["gpu".into()];
        let rule = daily("solar", "08:30", "16:00", &miners);
        let active = active_miners([rule], now(100, 1, 10, 0));
        let mut control = ManualScheduleControl::default();
        assert!(control.pause("gpu", &active));
        for _ in 0..100 { assert!(control.is_paused("gpu", &active)); }
    }
    #[test]
    fn manual_start_clears_pause_immediately() {
        let miners = vec!["gpu".into()];
        let active = active_miners([daily("a", "08:00", "16:00", &miners)], now(100, 1, 9, 0));
        let mut control = ManualScheduleControl::default();
        control.pause("gpu", &active);
        assert!(control.resume("gpu"));
        assert!(!control.is_paused("gpu", &active));
    }
    #[test]
    fn next_day_resumes_even_if_no_tick_observed_window_end() {
        let miners = vec!["gpu".into()];
        let rule = daily("a", "08:30", "16:00", &miners);
        let mut control = ManualScheduleControl::default();
        control.pause("gpu", &active_miners([rule], now(100, 1, 10, 0)));
        let tomorrow = active_miners([rule], now(101, 2, 10, 0));
        assert!(!control.is_paused("gpu", &tomorrow));
        assert!(control.prune(&tomorrow));
        assert!(control.snapshot().is_empty());
    }
    #[test]
    fn backend_restart_restores_same_occurrence_pause() {
        let miners = vec!["gpu".into()];
        let active = active_miners([daily("a", "08:00", "16:00", &miners)], now(100, 1, 9, 0));
        let mut original = ManualScheduleControl::default();
        original.pause("gpu", &active);
        let restarted = ManualScheduleControl::from_snapshot(original.snapshot().clone());
        assert!(restarted.is_paused("gpu", &active));
    }
    #[test]
    fn stop_all_includes_eligible_profiles_without_processes() {
        let miners = vec!["gpu0".into(), "gpu1".into(), "failed".into()];
        let active = active_miners([daily("a", "08:00", "16:00", &miners)], now(100, 1, 9, 0));
        let mut control = ManualScheduleControl::default();
        assert!(control.pause_all(&active));
        for id in &miners { assert!(control.is_paused(id, &active)); }
    }
    #[test]
    fn one_profile_stop_does_not_pause_other_profiles() {
        let miners = vec!["a".into(), "b".into()];
        let active = active_miners([daily("s", "08:00", "16:00", &miners)], now(100, 1, 9, 0));
        let mut control = ManualScheduleControl::default();
        control.pause("a", &active);
        assert!(!control.is_paused("b", &active));
    }
    #[test]
    fn overlapping_schedules_do_not_undo_manual_stop() {
        let miners = vec!["gpu".into()];
        let a = daily("a", "08:00", "16:00", &miners);
        let b = daily("b", "09:00", "17:00", &miners);
        let mut control = ManualScheduleControl::default();
        control.pause("gpu", &active_miners([a, b], now(100, 1, 10, 0)));
        let after_a = active_miners([a, b], now(100, 1, 16, 30));
        control.prune(&after_a);
        assert!(control.is_paused("gpu", &after_a));
        assert!(control.prune(&active_miners([a, b], now(100, 1, 17, 0))));
    }
    #[test]
    fn a_new_overlapping_window_cannot_restart_before_stopped_window_ends() {
        let miners = vec!["gpu".into()];
        let a = daily("a", "08:00", "16:00", &miners);
        let b = daily("b", "12:00", "18:00", &miners);
        let mut control = ManualScheduleControl::default();
        control.pause("gpu", &active_miners([a, b], now(100, 1, 10, 0)));
        assert!(control.is_paused("gpu", &active_miners([a, b], now(100, 1, 13, 0))));
        assert!(!control.is_paused("gpu", &active_miners([a, b], now(100, 1, 16, 0))));
    }
    #[test]
    fn adjacent_window_is_not_suppressed() {
        let miners = vec!["gpu".into()];
        let rules = [daily("a", "08:00", "10:00", &miners), daily("b", "10:00", "12:00", &miners)];
        let mut control = ManualScheduleControl::default();
        control.pause("gpu", &active_miners(rules, now(100, 1, 9, 0)));
        assert!(!control.is_paused("gpu", &active_miners(rules, now(100, 1, 10, 0))));
    }
    #[test]
    fn overnight_pause_survives_midnight() {
        let miners = vec!["gpu".into()];
        let mut rule = daily("night", "22:00", "07:00", &miners);
        rule.days = &[7];
        let sunday = active_miners([rule], now(106, 7, 23, 0));
        let monday = active_miners([rule], now(107, 1, 6, 59));
        assert_eq!(sunday, monday);
        let mut control = ManualScheduleControl::default();
        control.pause("gpu", &sunday);
        assert!(control.is_paused("gpu", &monday));
        assert!(active_miners([rule], now(107, 1, 7, 0)).is_empty());
    }
    #[test]
    fn boundary_start_inclusive_end_exclusive() {
        let miners = vec!["gpu".into()];
        let rule = daily("solar", "08:30", "16:00", &miners);
        assert!(active_occurrence(rule, now(100, 1, 8, 29)).is_none());
        assert!(active_occurrence(rule, now(100, 1, 8, 30)).is_some());
        assert!(active_occurrence(rule, now(100, 1, 16, 0)).is_none());
    }
    #[test]
    fn disabled_invalid_and_empty_windows_are_inactive() {
        let miners = vec!["gpu".into()];
        let mut rule = daily("a", "08:00", "16:00", &miners);
        rule.enabled = false;
        assert!(active_occurrence(rule, now(100, 1, 9, 0)).is_none());
        for (start, end) in [("25:00", "16:00"), ("08:00", "09:70"), ("08:00", "08:00")] {
            assert!(active_occurrence(daily("a", start, end, &miners), now(100, 1, 9, 0)).is_none());
        }
    }
    #[test]
    fn manual_session_survives_schedule_end() {
        assert!(!scheduler_should_stop_session(true, "manual", false));
    }

    #[test]
    fn scheduler_session_stops_at_schedule_end() {
        assert!(scheduler_should_stop_session(true, "schedule", false));
        assert!(!scheduler_should_stop_session(true, "schedule", true));
        assert!(!scheduler_should_stop_session(false, "schedule", false));
    }

    #[test]
    fn stop_outside_schedule_does_not_block_future_work() {
        let miners = vec!["gpu".into()];
        let rule = daily("a", "08:00", "16:00", &miners);
        let mut control = ManualScheduleControl::default();
        assert!(!control.pause("gpu", &active_miners([rule], now(100, 1, 17, 0))));
        assert!(!control.is_paused("gpu", &active_miners([rule], now(101, 2, 9, 0))));
    }
}
