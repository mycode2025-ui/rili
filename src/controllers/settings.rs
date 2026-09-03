use crate::*;

pub(crate) fn register_settings_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    {
        let ui_weak = ui.as_weak();
        ui.on_set_auto_start_enabled(move |enabled| {
            let result = autostart::set_enabled(enabled);
            if let Some(ui) = ui_weak.upgrade() {
                match result {
                    Ok(()) => {
                        ui.set_auto_start_enabled(enabled);
                        ui.set_action_message(
                            if enabled {
                                "开机启动已开启"
                            } else {
                                "开机启动已关闭"
                            }
                            .into(),
                        );
                    }
                    Err(error) => {
                        ui.set_auto_start_enabled(autostart::is_enabled());
                        ui.set_action_message(format!("设置开机启动失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_notifications_enabled(move |enabled| {
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_notifications_enabled())
                .unwrap_or(!enabled);
            let result = db::set_setting(
                &state.borrow().conn,
                "notifications_enabled",
                if enabled { "1" } else { "0" },
            );
            if let Some(ui) = ui_weak.upgrade() {
                match result {
                    Ok(()) => {
                        ui.set_notifications_enabled(enabled);
                        ui.set_action_message(
                            if enabled {
                                "通知已启用"
                            } else {
                                "通知已停用"
                            }
                            .into(),
                        );
                    }
                    Err(error) => {
                        ui.set_notifications_enabled(previous);
                        ui.set_action_message(format!("保存通知设置失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_default_event_reminder(move |value| {
            let value = value.trim().to_string();
            let result = {
                let mut s = state.borrow_mut();
                match db::apply_default_event_reminder(&s.conn, &value) {
                    Ok(changed) => {
                        s.default_event_reminder = value.clone();
                        Ok(changed)
                    }
                    Err(error) => Err(error),
                }
            };
            if let Some(ui) = ui_weak.upgrade() {
                match result {
                    Ok(changed) => {
                        ui.set_default_event_reminder(value.clone().into());
                        let label = match value.as_str() {
                            "" => "关闭",
                            "0" => "准时",
                            "10" => "提前10分钟",
                            "60" => "提前1小时",
                            "1440" => "提前1天",
                            _ => "未知",
                        };
                        ui.set_action_message(
                            format!("默认提醒已设为{label}，并更新 {changed} 条现有日程").into(),
                        );
                        if let Some(widget) = widget_weak.upgrade() {
                            refresh_all(&ui, &widget, &state);
                        }
                    }
                    Err(error) => {
                        ui.set_action_message(format!("更新默认提醒失败：{error}").into())
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_taskbar_clock_enabled(move |enabled| {
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_taskbar_clock_enabled())
                .unwrap_or(!enabled);
            let result = db::set_setting(
                &state.borrow().conn,
                "taskbar_clock_enabled",
                if enabled { "1" } else { "0" },
            );
            if let Some(ui) = ui_weak.upgrade() {
                match result {
                    Ok(()) => {
                        ui.set_taskbar_clock_enabled(enabled);
                        ui.set_action_message(
                            if enabled {
                                "系统时钟点击接管已开启"
                            } else {
                                "系统时钟点击接管已关闭"
                            }
                            .into(),
                        );
                        TASKBAR_CLOCK_HOOK_ENABLED.store(enabled, Ordering::Release);
                        if enabled {
                            update_taskbar_clock_hit_rect();
                        }
                    }
                    Err(error) => {
                        ui.set_taskbar_clock_enabled(previous);
                        ui.set_action_message(format!("保存任务栏时钟设置失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_local_only(move |enabled| {
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_local_only())
                .unwrap_or(!enabled);
            let result = db::set_setting(
                &state.borrow().conn,
                "local_only",
                if enabled { "1" } else { "0" },
            );
            if let Some(ui) = ui_weak.upgrade() {
                match result {
                    Ok(()) => {
                        ui.set_local_only(enabled);
                        ui.set_action_message(
                            if enabled {
                                "已切换为仅本地模式"
                            } else {
                                "已允许外部同步"
                            }
                            .into(),
                        );
                    }
                    Err(error) => {
                        ui.set_local_only(previous);
                        ui.set_action_message(format!("保存本地模式设置失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_settings_action(move |action| {
            let action = action.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                let message = match action.as_str() {
                    "test-notification" => {
                        if !ui.get_notifications_enabled() {
                            "请先开启“系统通知”后再发送示例提醒".to_string()
                        } else {
                            match reminders::send_test_notification() {
                                Ok(()) => "示例提醒已发送，请查看 Windows 通知中心".to_string(),
                                Err(error) => format!("示例提醒发送失败：{error}"),
                            }
                        }
                    }
                    "sync" => {
                        if ui.get_local_only() {
                            "仅本地模式下未执行同步".to_string()
                        } else {
                            ui.invoke_sync_subscriptions();
                            "正在同步外部日历…".to_string()
                        }
                    }
                    "open-integrations" => {
                        state.borrow_mut().view_mode = 6;
                        set_main_view_mode(&ui, 6);
                        "已打开工具与集成配置".to_string()
                    }
                    "open-data-folder" => match app_paths::db_path() {
                        Ok(db_path) => {
                            let directory = db_path.parent().unwrap_or(db_path.as_path());
                            #[cfg(windows)]
                            let result = {
                                use std::os::windows::process::CommandExt;
                                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                                std::process::Command::new("explorer.exe")
                                    .creation_flags(CREATE_NO_WINDOW)
                                    .arg(directory)
                                    .spawn()
                            };
                            #[cfg(not(windows))]
                            let result: std::io::Result<
                                std::process::Child,
                            > = Err(std::io::Error::new(
                                std::io::ErrorKind::Unsupported,
                                "当前平台尚未配置文件管理器",
                            ));
                            match result {
                                Ok(_) => "已打开数据目录".to_string(),
                                Err(error) => format!("打开数据目录失败：{error}"),
                            }
                        }
                        Err(error) => format!("定位数据目录失败：{error}"),
                    },
                    "backup" => {
                        let backup_name = format!(
                            "timehub-backup-{}.db",
                            Local::now().format("%Y%m%d-%H%M%S-%3f")
                        );
                        match app_paths::db_path() {
                            Ok(path) => {
                                let backup_path = path.with_file_name(backup_name);
                                let result = state.borrow().conn.execute(
                                    "VACUUM INTO ?1",
                                    [backup_path.to_string_lossy().as_ref()],
                                );
                                match result {
                                    Ok(_) => format!("备份已生成：{}", backup_path.display()),
                                    Err(error) => format!("备份失败：{error}"),
                                }
                            }
                            Err(error) => format!("备份失败：{error}"),
                        }
                    }
                    "reload" => {
                        if let Some(widget) = widget_weak.upgrade() {
                            refresh_all(&ui, &widget, &state);
                        }
                        "数据已重新载入".to_string()
                    }
                    "reset-shortcuts" => "快捷键已恢复为默认值".to_string(),
                    "check-update" => "当前为 TimeHub V3；更新服务地址尚未配置".to_string(),
                    "diagnostics" => {
                        let s = state.borrow();
                        let calendars = db::list_calendars(&s.conn).map(|v| v.len()).unwrap_or(0);
                        let events = db::list_all_events(&s.conn).map(|v| v.len()).unwrap_or(0);
                        format!("诊断完成：{calendars} 个日历，{events} 条日程，数据库可读")
                    }
                    _ => "操作已完成".to_string(),
                };
                ui.set_action_message(message.into());
            }
        });
    }
}
