use crate::*;

pub(crate) fn register_appearance_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    state: &Rc<RefCell<AppState>>,
) {
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_visual_theme(move |mode| {
            let mode = mode.clamp(0, 2);
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "visual_theme", &mode.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_visual_theme(mode);
                apply_visual_theme(&ui, &widget, &quick, mode);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_interface_font_size(move |font_size| {
            let font_size = font_size.clamp(12, 16);
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "interface_font_size", &font_size.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_interface_font_size(font_size);
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    font_size,
                    ui.get_interface_density(),
                    ui.get_reduce_motion(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_interface_density(move |density| {
            let density = density.clamp(0, 2);
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "interface_density", &density.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_interface_density(density);
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
                    density,
                    ui.get_reduce_motion(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_reduce_motion(move |reduce_motion| {
            {
                let s = state.borrow();
                let _ = db::set_setting(
                    &s.conn,
                    "reduce_motion",
                    if reduce_motion { "1" } else { "0" },
                );
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_reduce_motion(reduce_motion);
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
                    ui.get_interface_density(),
                    reduce_motion,
                );
            }
        });
    }
}
