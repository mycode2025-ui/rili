//! 高频局部刷新。
//!
//! 新增、勾选和编辑待办/便签/习惯/课程时，只重建对应模型，避免每次都重新
//! 查询 42 天月历、周视图、时间轴、搜索、订阅、天气及所有桌面卡片数据。

use crate::*;

pub(crate) fn refresh_todos(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let (selected, today_items, board_items) = {
        let s = state.borrow();
        let today = Local::now().date_naive();
        let selected_date = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day);
        let selected = selected_date
            .and_then(|date| db::list_todos(&s.conn, Some(date), None).ok())
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect::<Vec<_>>();
        let today_items = db::list_todos(&s.conn, Some(today), None)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect::<Vec<_>>();
        let board_items = db::list_all_todos(&s.conn).unwrap_or_default();
        (selected, today_items, board_items)
    };

    let today = Local::now().date_naive();
    let todo = board_items
        .iter()
        .filter(|item| item.status == "todo")
        .map(|item| to_board_item(item, today))
        .collect::<Vec<_>>();
    let doing = board_items
        .iter()
        .filter(|item| item.status == "doing")
        .map(|item| to_board_item(item, today))
        .collect::<Vec<_>>();
    let done = board_items
        .iter()
        .filter(|item| item.status == "done")
        .map(|item| to_board_item(item, today))
        .collect::<Vec<_>>();
    let active = board_items
        .iter()
        .filter(|item| item.status != "done")
        .collect::<Vec<_>>();
    let quadrant = |important, urgent| {
        active
            .iter()
            .filter(|item| {
                item.important == important && todo_is_urgent(&item.due_date, today) == urgent
            })
            .map(|item| to_board_item(item, today))
            .collect::<Vec<_>>()
    };

    ui.set_todos_for_day(ModelRc::new(VecModel::from(selected)));
    ui.set_board_todo_items(ModelRc::new(VecModel::from(todo)));
    ui.set_board_doing_items(ModelRc::new(VecModel::from(doing)));
    ui.set_board_done_items(ModelRc::new(VecModel::from(done)));
    ui.set_board_urgent_important(ModelRc::new(VecModel::from(quadrant(true, true))));
    ui.set_board_not_urgent_important(ModelRc::new(VecModel::from(quadrant(true, false))));
    ui.set_board_urgent_not_important(ModelRc::new(VecModel::from(quadrant(false, true))));
    ui.set_board_not_urgent_not_important(ModelRc::new(VecModel::from(quadrant(false, false))));
    widget.set_todo_completed_count(today_items.iter().filter(|item| item.done).count() as i32);
    widget.set_today_todos(ModelRc::new(VecModel::from(today_items)));
    sync_desktop_widgets(widget);
}

pub(crate) fn refresh_notes(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let notes = db::list_notes(&state.borrow().conn)
        .unwrap_or_default()
        .into_iter()
        .map(to_ui_note)
        .collect::<Vec<_>>();
    ui.set_notes(ModelRc::new(VecModel::from(notes.clone())));
    widget.set_recent_notes(ModelRc::new(VecModel::from(notes)));
    sync_desktop_widgets(widget);
}

pub(crate) fn refresh_habits(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let habits = db::list_habits(&state.borrow().conn, false)
        .unwrap_or_default()
        .into_iter()
        .map(to_ui_habit)
        .collect::<Vec<_>>();
    ui.set_habits(ModelRc::new(VecModel::from(habits)));
    update_tool_status(ui, widget, state);
}

pub(crate) fn refresh_courses(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    let s = state.borrow();
    let today = Local::now().date_naive();
    let courses = db::list_courses(&s.conn).unwrap_or_default();
    let active_count = courses
        .iter()
        .filter(|course| {
            course.start_week <= s.course_week as i64 && s.course_week as i64 <= course.end_week
        })
        .count() as i32;
    let actual_week = course_week_for_date(s.course_term_start, today);
    let (week_start, week_end) = course_week_range(s.course_term_start, s.course_week);
    ui.set_course_items(ModelRc::new(VecModel::from(
        courses.into_iter().map(to_ui_course).collect::<Vec<_>>(),
    )));
    ui.set_course_week(s.course_week);
    ui.set_course_actual_week(actual_week);
    ui.set_course_term_start(s.course_term_start.to_string().into());
    ui.set_course_active_count(active_count);
    ui.set_course_week_title(
        format!(
            "{}月{}日 – {}月{}日",
            week_start.month(),
            week_start.day(),
            week_end.month(),
            week_end.day()
        )
        .into(),
    );
    update_tool_status(ui, widget, state);
}

pub(crate) fn refresh_search(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let results = db::search(&s.conn, &s.search_query, 50)
        .unwrap_or_default()
        .into_iter()
        .map(|hit| SearchItem {
            id: hit.id as i32,
            kind: hit.kind.into(),
            title: hit.title.into(),
            meta: hit.meta.into(),
            date: hit.date.into(),
        })
        .collect::<Vec<_>>();
    ui.set_search_query(s.search_query.clone().into());
    ui.set_search_results(ModelRc::new(VecModel::from(results)));
}
