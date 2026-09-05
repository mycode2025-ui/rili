use crate::*;

pub(crate) fn register_widget_content_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
    desktop_widget_visibility: &Rc<RefCell<DesktopWidgetVisibility>>,
    widget_shown: &Rc<Cell<bool>>,
) {
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_toggle_todo(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::toggle_todo(&s.conn, id as i64) {
                    error_reporter::report("挂件更新待办失败", &e);
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_todos(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_add_note(move |title: SharedString| {
            let title = title.trim().to_string();
            if !title.is_empty() {
                let s = state.borrow();
                if let Err(e) = db::create_note(&s.conn, &title, "") {
                    error_reporter::report("挂件新建便签失败", &e);
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_notes(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_create_widget_note(move || {
            {
                let s = state.borrow();
                if let Err(error) = db::create_note(&s.conn, "新便签", "") {
                    error_reporter::report("新建便签卡片失败", &error);
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_notes(&ui, &widget, &state);
            }
        });
    }
    {
        let state = state.clone();
        widget.on_save_widget_note(move |id, title, content| {
            let title = super::data_actions::normalized_note_title(&title, &content);
            {
                let s = state.borrow();
                if let Err(error) = db::update_note(&s.conn, id as i64, &title, content.as_str()) {
                    error_reporter::report("保存便签卡片失败", &error);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        let visibility = desktop_widget_visibility.clone();
        let shown = widget_shown.clone();
        widget.on_delete_widget_note(move |id| {
            let no_notes_left = {
                let s = state.borrow();
                if let Err(error) = db::delete_note(&s.conn, id as i64) {
                    error_reporter::report("删除便签卡片失败", &error);
                }
                db::list_notes(&s.conn).unwrap_or_default().is_empty()
            };
            if no_notes_left {
                visibility.borrow_mut().notes = false;
                let configuration = *visibility.borrow();
                shown.set(configuration.any());
                let s = state.borrow();
                if let Err(error) = db::set_setting(&s.conn, "widget_notes_visible", "0") {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_action_message(format!("保存便签卡片状态失败：{error}").into());
                    }
                }
                if let Some(ui) = ui_weak.upgrade() {
                    sync_desktop_visibility_to_ui(&ui, configuration);
                    ui.set_widget_visible(configuration.any());
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_notes(&ui, &widget, &state);
            }
        });
    }
}
