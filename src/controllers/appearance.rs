use crate::*;

pub(crate) fn register_appearance_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    state: &Rc<RefCell<AppState>>,
    initial_font_family: String,
) {
    let selected_font_family = Rc::new(RefCell::new(initial_font_family));
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_visual_theme(move |mode| {
            let mode = mode.clamp(0, 2);
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_visual_theme())
                .unwrap_or(1);
            let result = db::set_setting(&state.borrow().conn, "visual_theme", &mode.to_string());
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                match result {
                    Ok(()) => {
                        ui.set_visual_theme(mode);
                        apply_visual_theme(&ui, &widget, &quick, mode);
                    }
                    Err(error) => {
                        ui.set_visual_theme(previous);
                        apply_visual_theme(&ui, &widget, &quick, previous);
                        ui.set_action_message(format!("保存主题设置失败：{error}").into());
                    }
                }
            }
        });
    }
    macro_rules! wire_widget_style_setting {
        ($callback:ident, $property:ident, $suffix:literal, $min:expr, $max:expr) => {{
            let ui_weak = ui.as_weak();
            let state = state.clone();
            ui.$callback(move |value| {
                let value = value.clamp($min, $max);
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                let key = format!("desktop_widget_{}", $suffix);
                match db::set_setting(&state.borrow().conn, &key, &value.to_string()) {
                    Ok(()) => {
                        ui.$property(value);
                        apply_all_desktop_widget_styles(
                            &state.borrow().conn,
                            ui.get_visual_theme(),
                            ui.get_theme_index(),
                        );
                    }
                    Err(error) => ui.set_action_message(
                        format!("保存桌面卡片外观失败：{error}").into(),
                    ),
                }
            });
        }};
    }
    wire_widget_style_setting!(
        on_set_widget_style_opacity,
        set_widget_style_opacity,
        "opacity",
        35,
        100
    );
    wire_widget_style_setting!(
        on_set_widget_style_theme,
        set_widget_style_theme,
        "theme",
        0,
        2
    );
    wire_widget_style_setting!(
        on_set_widget_style_accent,
        set_widget_style_accent,
        "accent",
        -1,
        7
    );
    {
        let ui_weak = ui.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_quick_panel_pinned(move |pinned| {
            let result = db::set_setting(
                &state.borrow().conn,
                "quick_panel_pinned",
                if pinned { "1" } else { "0" },
            );
            if let (Some(ui), Some(quick)) = (ui_weak.upgrade(), quick_weak.upgrade()) {
                match result {
                    Ok(()) => {
                        ui.set_quick_panel_pinned(pinned);
                        quick.set_pinned(pinned);
                    }
                    Err(error) => {
                        ui.set_quick_panel_pinned(!pinned);
                        ui.set_action_message(format!("保存快速面板设置失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        let selected_font_family = selected_font_family.clone();
        ui.on_set_interface_font_family(move |choice| {
            let Some(family) = font_settings::family_from_choice(choice.as_str()) else {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_action_message("无法识别所选字体".into());
                }
                return;
            };
            let result = db::set_settings(
                &mut state.borrow_mut().conn,
                &[("interface_font_family", family), ("custom_font_path", "")],
            );
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                match result {
                    Ok(()) => {
                        *selected_font_family.borrow_mut() = family.to_string();
                        ui.set_interface_font_family(
                            font_settings::choice_from_family(family).into(),
                        );
                        apply_font_family(&ui, &widget, &quick, family);
                        ui.set_action_message(format!("界面字体已切换为：{choice}").into());
                    }
                    Err(error) => {
                        let previous = selected_font_family.borrow();
                        ui.set_interface_font_family(
                            font_settings::choice_from_family(&previous).into(),
                        );
                        ui.set_action_message(format!("保存字体设置失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        let selected_font_family = selected_font_family.clone();
        ui.on_choose_custom_font(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let path = match font_settings::pick_font_file() {
                Ok(Some(path)) => path,
                Ok(None) => return,
                Err(error) => {
                    ui.set_action_message(format!("打开字体文件失败：{error}").into());
                    return;
                }
            };
            let family = match font_settings::register_custom_font(&path) {
                Ok(family) => family,
                Err(error) => {
                    ui.set_action_message(format!("加载字体失败：{error}").into());
                    return;
                }
            };
            let path_text = path.to_string_lossy().into_owned();
            let result = db::set_settings(
                &mut state.borrow_mut().conn,
                &[
                    ("interface_font_family", family.as_str()),
                    ("custom_font_path", path_text.as_str()),
                ],
            );
            let (Some(widget), Some(quick)) = (widget_weak.upgrade(), quick_weak.upgrade()) else {
                return;
            };
            match result {
                Ok(()) => {
                    *selected_font_family.borrow_mut() = family.clone();
                    ui.set_interface_font_family(font_settings::choice_from_family(&family).into());
                    apply_font_family(&ui, &widget, &quick, &family);
                    ui.set_action_message(format!("已加载自定义字体：{family}").into());
                }
                Err(error) => {
                    ui.set_action_message(format!("保存自定义字体设置失败：{error}").into());
                }
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
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_interface_font_size())
                .unwrap_or(13);
            let result = db::set_setting(
                &state.borrow().conn,
                "interface_font_size",
                &font_size.to_string(),
            );
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                if let Err(error) = result {
                    ui.set_interface_font_size(previous);
                    ui.set_action_message(format!("保存界面字号失败：{error}").into());
                } else {
                    ui.set_interface_font_size(font_size);
                }
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
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
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_interface_density())
                .unwrap_or(1);
            let result = db::set_setting(
                &state.borrow().conn,
                "interface_density",
                &density.to_string(),
            );
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                if let Err(error) = result {
                    ui.set_interface_density(previous);
                    ui.set_action_message(format!("保存界面密度失败：{error}").into());
                } else {
                    ui.set_interface_density(density);
                }
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
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
        ui.on_set_reduce_motion(move |reduce_motion| {
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_reduce_motion())
                .unwrap_or(!reduce_motion);
            let result = db::set_setting(
                &state.borrow().conn,
                "reduce_motion",
                if reduce_motion { "1" } else { "0" },
            );
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                if let Err(error) = result {
                    ui.set_reduce_motion(previous);
                    ui.set_action_message(format!("保存动效设置失败：{error}").into());
                } else {
                    ui.set_reduce_motion(reduce_motion);
                }
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
                    ui.get_interface_density(),
                    ui.get_reduce_motion(),
                );
            }
        });
    }
}
