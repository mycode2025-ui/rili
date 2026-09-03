use super::WidgetControllerContext;
use crate::*;

pub(crate) fn register_widget_manager_callbacks(context: WidgetControllerContext<'_>) {
    let WidgetControllerContext {
        ui,
        widget,
        quick_panel,
        desktop_widgets,
        state,
        visibility: desktop_widget_visibility,
        click_through: desktop_click_through,
        shown: widget_shown,
    } = context;
    // -------- 打开/关闭桌面挂件 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let desktop_widgets = desktop_widgets.clone();
        let widget_shown = widget_shown.clone();
        let desktop_widget_visibility = desktop_widget_visibility.clone();
        let desktop_click_through = desktop_click_through.clone();
        let state = state.clone();
        ui.on_toggle_widget(move || {
            let show = !widget_shown.get();
            widget_shown.set(show);
            if let Some(widget) = widget_weak.upgrade() {
                if show {
                    desktop_widgets.sync_from(&widget);
                    desktop_widgets.show_configured(
                        *desktop_widget_visibility.borrow(),
                        desktop_click_through.get(),
                    );
                } else {
                    desktop_widgets.hide_all();
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_widget_visible(show);
            }
            let state = state.borrow();
            if let Err(error) = db::set_setting(
                &state.conn,
                "desktop_widgets_visible",
                if show { "1" } else { "0" },
            ) {
                error_reporter::report("保存桌面挂件显示状态失败", &error);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let desktop_widgets = desktop_widgets.clone();
        let widget_shown = widget_shown.clone();
        let desktop_widget_visibility = desktop_widget_visibility.clone();
        let desktop_click_through = desktop_click_through.clone();
        let state = state.clone();
        ui.on_set_desktop_widget_visible(move |kind, visible| {
            let kind = kind.to_string();
            let should_refresh_notes = kind == "notes" && visible;
            if should_refresh_notes {
                let s = state.borrow();
                if db::list_notes(&s.conn).unwrap_or_default().is_empty() {
                    if let Err(error) = db::create_note(&s.conn, "新便签", "") {
                        error_reporter::report("创建首个便签卡片失败", &error);
                    }
                }
            }
            {
                desktop_widget_visibility.borrow_mut().set(&kind, visible);
            }
            let configuration = *desktop_widget_visibility.borrow();
            let any_visible = configuration.any();
            widget_shown.set(any_visible);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                if should_refresh_notes {
                    refresh_notes(&ui, &widget, &state);
                } else {
                    desktop_widgets.sync_from(&widget);
                }
                desktop_widgets.show_configured(configuration, desktop_click_through.get());
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_widget_visible(any_visible);
            }
            let state = state.borrow();
            let key = format!("widget_{kind}_visible");
            if let Err(error) = db::set_setting(&state.conn, &key, if visible { "1" } else { "0" })
            {
                error_reporter::report("保存桌面卡片状态失败", &error);
            }
            if let Err(error) = db::set_setting(
                &state.conn,
                "desktop_widgets_visible",
                if any_visible { "1" } else { "0" },
            ) {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_action_message(format!("保存桌面卡片总开关失败：{error}").into());
                }
            }
        });
    }
    {
        let desktop_widgets = desktop_widgets.clone();
        let desktop_widget_visibility = desktop_widget_visibility.clone();
        let desktop_click_through = desktop_click_through.clone();
        let state = state.clone();
        ui.on_set_desktop_click_through(move |enabled| {
            desktop_click_through.set(enabled);
            desktop_widgets.apply_click_through(*desktop_widget_visibility.borrow(), enabled);
            let state = state.borrow();
            if let Err(error) = db::set_setting(
                &state.conn,
                "desktop_widgets_click_through",
                if enabled { "1" } else { "0" },
            ) {
                error_reporter::report("保存桌面挂件鼠标穿透状态失败", &error);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_open_quick_panel(move || {
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                sync_quick_panel(&quick, &ui, &widget, &state);
                show_and_focus_quick_panel(&quick);
            }
        });
    }
}
