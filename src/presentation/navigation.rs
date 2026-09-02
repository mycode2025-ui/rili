//! 日期与课程周导航。

use crate::*;

pub(crate) fn shift_month(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    if delta < 0 {
        if s.month == 1 {
            s.month = 12;
            s.year -= 1;
        } else {
            s.month -= 1;
        }
    } else if s.month == 12 {
        s.month = 1;
        s.year += 1;
    } else {
        s.month += 1;
    }
    s.selected_day = 1;
}

pub(crate) fn shift_year(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    s.year += delta;
    while NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).is_none() {
        s.selected_day = s.selected_day.saturating_sub(1);
    }
    s.week_anchor =
        NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).unwrap_or(s.week_anchor);
    s.timeline_anchor =
        NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).unwrap_or(s.timeline_anchor);
}

pub(crate) fn goto_today(state: &Rc<RefCell<AppState>>) {
    let today = Local::now().date_naive();
    let mut s = state.borrow_mut();
    s.year = today.year();
    s.month = today.month();
    s.selected_day = today.day();
    s.week_anchor = today;
    s.timeline_anchor = today;
}

pub(crate) fn shift_week(state: &Rc<RefCell<AppState>>, delta_weeks: i64) {
    let mut s = state.borrow_mut();
    let new_anchor = s.week_anchor + chrono::Duration::days(7 * delta_weeks);
    s.week_anchor = new_anchor;
    s.year = new_anchor.year();
    s.month = new_anchor.month();
    s.selected_day = new_anchor.day();
}

pub(crate) fn select_full_date(state: &Rc<RefCell<AppState>>, date: NaiveDate) {
    let mut s = state.borrow_mut();
    s.year = date.year();
    s.month = date.month();
    s.selected_day = date.day();
    s.week_anchor = date;
    s.timeline_anchor = date;
}

/// 日/三日视图的翻页：日视图每次移动 1 天，三日视图每次移动 3 天（跟"这组显示了哪几天"保持一致）。
pub(crate) fn shift_timeline(state: &Rc<RefCell<AppState>>, delta: i64) {
    let mut s = state.borrow_mut();
    let step = if s.view_mode == 3 { 3 } else { 1 };
    let new_anchor = s.timeline_anchor + chrono::Duration::days(step * delta);
    s.timeline_anchor = new_anchor;
    s.year = new_anchor.year();
    s.month = new_anchor.month();
    s.selected_day = new_anchor.day();
}

/// 一周的起始日期（周一或周日，取决于设置），`anchor` 是这一周里任意一天。
pub(crate) fn week_start_of(anchor: NaiveDate, week_starts_sunday: bool) -> NaiveDate {
    let leading = if week_starts_sunday {
        anchor.weekday().num_days_from_sunday()
    } else {
        anchor.weekday().num_days_from_monday()
    };
    anchor - chrono::Duration::days(leading as i64)
}

pub(crate) fn course_week_for_date(term_start: NaiveDate, date: NaiveDate) -> i32 {
    if date < term_start {
        1
    } else {
        ((date - term_start).num_days() / 7 + 1).clamp(1, 30) as i32
    }
}

pub(crate) fn course_week_range(term_start: NaiveDate, week: i32) -> (NaiveDate, NaiveDate) {
    let start = term_start + chrono::Duration::days((week.clamp(1, 30) - 1) as i64 * 7);
    (start, start + chrono::Duration::days(6))
}
