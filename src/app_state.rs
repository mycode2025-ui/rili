//! GUI 会话状态。数据库仍是业务数据的唯一持久化来源。

use crate::*;

pub(crate) struct AppState {
    pub(crate) conn: Connection,
    pub(crate) year: i32,
    pub(crate) month: u32, // 1..=12
    pub(crate) selected_day: u32,
    pub(crate) week_starts_sunday: bool,
    pub(crate) show_week_numbers: bool,
    /// 0=月视图 1=周视图 2=日视图 3=三日视图 4=待办 5=年视图 6=工具 7=搜索 8=记录 9=排班 10=设置 11=便签 12=课程表
    pub(crate) view_mode: i32,
    /// 0=看板 1=四象限（仅在 view_mode==4 时使用）
    pub(crate) todo_board_mode: i32,
    pub(crate) calculator_start: String,
    pub(crate) calculator_end: String,
    pub(crate) calculator_offset: String,
    pub(crate) calculator_result: String,
    pub(crate) stopwatch_elapsed_secs: u64,
    pub(crate) stopwatch_started_at: Option<Instant>,
    pub(crate) pomodoro_remaining_secs: u64,
    pub(crate) pomodoro_total_secs: u64,
    pub(crate) pomodoro_end_at: Option<Instant>,
    pub(crate) focus_task: String,
    pub(crate) focus_round: i32,
    pub(crate) search_query: String,
    pub(crate) shift_start_date: String,
    pub(crate) shift_end_date: String,
    pub(crate) shift_sequence: String,
    pub(crate) shift_result: String,
    pub(crate) ai_input: String,
    pub(crate) ai_draft: String,
    pub(crate) ai_draft_title: String,
    pub(crate) ai_draft_date: String,
    pub(crate) ai_draft_time: String,
    pub(crate) ai_draft_reminder: String,
    pub(crate) default_event_reminder: String,
    pub(crate) subscription_name: String,
    pub(crate) subscription_url: String,
    /// 周视图当前显示的一周里的任意一天（用于计算这一周的起止日期）；与 (year, month, selected_day)
    /// 分开维护，因为一周可能跨两个月，导航时两者需要保持同步但含义不同。
    pub(crate) week_anchor: NaiveDate,
    /// 日/三日视图当前显示的第一天。
    pub(crate) timeline_anchor: NaiveDate,
    /// 课程表学期第一周的周一，以及当前正在浏览的周次。
    pub(crate) course_term_start: NaiveDate,
    pub(crate) course_week: i32,
}
