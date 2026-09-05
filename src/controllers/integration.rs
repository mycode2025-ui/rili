use crate::*;

pub(crate) fn register_integration_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_configure_caldav(move |provider, server_url, username, password| {
            let Some(ui) = ui_weak.upgrade() else {
                return false;
            };
            if ui.get_sync_busy() {
                return false;
            }
            let provider = provider.to_string();
            let server_url = server_url.trim().to_string();
            let username = username.trim().to_string();
            let password = password.to_string();
            let (subscription_name, calendar_name, color) = match provider.as_str() {
                "dingtalk" => ("钉钉日历", "钉钉会议", "#1677ff"),
                "feishu" => ("飞书日历", "飞书日历", "#3370ff"),
                _ => ("CalDAV", "CalDAV", "#8b5cf6"),
            };
            let result = {
                let s = state.borrow();
                s.conn
                    .unchecked_transaction()
                    .map_err(anyhow::Error::from)
                    .and_then(|tx| {
                        let calendar = integrations::get_or_create_integration_calendar(
                            &tx,
                            calendar_name,
                            color,
                        )?;
                        let subscription = db::create_or_update_caldav_subscription(
                            &tx,
                            subscription_name,
                            &server_url,
                            &username,
                            &password,
                            calendar.id,
                        )?;
                        tx.commit()?;
                        Ok(subscription)
                    })
            };
            let subscription = match result {
                Ok(subscription) => subscription,
                Err(error) => {
                    let text = format!("{subscription_name}配置失败：{error}");
                    ui.set_integration_status(text.clone().into());
                    ui.set_action_message(text.into());
                    return false;
                }
            };

            if let Some(widget) = widget_weak.upgrade() {
                refresh_all(&ui, &widget, &state);
            }
            let pending = format!("{subscription_name}账号已安全保存，正在验证并首次同步…");
            ui.set_integration_status(pending.clone().into());
            ui.set_action_message(pending.into());
            ui.set_sync_busy(true);

            let ui_weak = ui.as_weak();
            std::thread::spawn(move || {
                let text = match db::open().and_then(|conn| {
                    integrations::sync_ics_subscription_with_result(&conn, &subscription)
                }) {
                    Ok(report) => format!(
                        "{}连接成功：同步 {} 条日程，清理 {} 条已取消日程",
                        subscription.name, report.imported_events, report.removed_cancelled_events
                    ),
                    Err(error) => format!("{}连接失败：{error}", subscription.name),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_sync_busy(false);
                        ui.set_integration_status(text.clone().into());
                        ui.set_action_message(text.into());
                        ui.invoke_refresh_external_data();
                    }
                });
            });
            true
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_subscription(move |name, url| {
            if ui_weak.upgrade().is_some_and(|ui| ui.get_sync_busy()) {
                return;
            }
            let name = name.trim().to_string();
            let url = url.trim().to_string();
            let (already_exists, result) = {
                let s = state.borrow();
                let already_exists = db::get_subscription_by_url(&s.conn, &url)
                    .ok()
                    .flatten()
                    .is_some();
                (
                    already_exists,
                    db::create_subscription(&s.conn, &name, &url, 1),
                )
            };
            let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) else {
                return;
            };
            let subscription = match result {
                Ok(subscription) => subscription,
                Err(error) => {
                    let text = format!("添加失败：{error}");
                    ui.set_integration_status(text.clone().into());
                    ui.set_action_message(text.into());
                    return;
                }
            };

            {
                let mut s = state.borrow_mut();
                s.subscription_name.clear();
                s.subscription_url.clear();
            }
            refresh_all(&ui, &widget, &state);
            let pending = if already_exists {
                format!("“{}”已存在，正在重新同步…", subscription.name)
            } else {
                format!("已添加“{}”，正在首次同步…", subscription.name)
            };
            ui.set_integration_status(pending.clone().into());
            ui.set_action_message(pending.into());
            ui.set_sync_busy(true);

            let ui_weak = ui.as_weak();
            std::thread::spawn(move || {
                let text = match db::open().and_then(|conn| {
                    integrations::sync_ics_subscription_with_result(&conn, &subscription)
                }) {
                    Ok(report) => format!(
                        "“{}”同步成功：导入 {} 条日程，清理 {} 条已取消日程",
                        subscription.name, report.imported_events, report.removed_cancelled_events
                    ),
                    Err(error) => format!("“{}”同步失败：{error}", subscription.name),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_sync_busy(false);
                        ui.set_integration_status(text.clone().into());
                        ui.set_action_message(text.into());
                        ui.invoke_refresh_external_data();
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_subscription(move |id| {
            if ui_weak.upgrade().is_some_and(|ui| ui.get_sync_busy()) {
                return;
            }
            let result = {
                let s = state.borrow();
                db::delete_subscription(&s.conn, i64::from(id))
            };
            let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) else {
                return;
            };
            let text = match result {
                Ok(report) if report.deleted_connection => {
                    let calendar_note = if report.deleted_calendar {
                        format!("，已移除空日历“{}”", report.calendar_name)
                    } else {
                        String::new()
                    };
                    format!(
                        "同步连接已删除，已清空 {} 条对应日程{}",
                        report.deleted_events, calendar_note
                    )
                }
                Ok(_) => "该同步连接已不存在".to_string(),
                Err(error) => format!("删除同步连接失败：{error}"),
            };
            refresh_all(&ui, &widget, &state);
            ui.set_integration_status(text.clone().into());
            ui.set_action_message(text.into());
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_sync_subscriptions(move || {
            if let Some(ui) = ui_weak.upgrade() {
                if ui.get_sync_busy() {
                    return;
                }
                ui.set_sync_busy(true);
                ui.set_integration_status("正在同步全部日历订阅…".into());
            }
            let ui_weak = ui_weak.clone();
            std::thread::spawn(move || {
                let text = match db::open().and_then(|conn| integrations::sync_all_ics(&conn)) {
                    Ok(report) if report.errors.is_empty() => format!(
                        "同步成功：{} 个订阅，导入 {} 条日程，清理 {} 条已取消日程",
                        report.subscriptions,
                        report.imported_events,
                        report.removed_cancelled_events
                    ),
                    Ok(report) => format!(
                        "同步完成：{} 个订阅，{} 条日程，清理 {} 条已取消日程；失败：{}",
                        report.subscriptions,
                        report.imported_events,
                        report.removed_cancelled_events,
                        report.errors.join("；")
                    ),
                    Err(error) => format!("同步失败：{error}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_sync_busy(false);
                        ui.set_integration_status(text.clone().into());
                        ui.set_action_message(text.into());
                        ui.invoke_refresh_external_data();
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_edit_subscription(move |id, name, password| {
            let Some(ui) = ui_weak.upgrade() else {
                return false;
            };
            if ui.get_sync_busy() {
                return false;
            }
            let result =
                db::edit_subscription(&state.borrow().conn, i64::from(id), &name, &password);
            match result {
                Ok(()) => {
                    ui.set_integration_status("连接已更新，可点击该连接的同步按钮验证".into());
                    ui.invoke_refresh_external_data();
                    true
                }
                Err(error) => {
                    ui.set_integration_status(
                        format!("保存失败：{}", integrations::safe_error(&error.to_string()))
                            .into(),
                    );
                    false
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_sync_subscription(move |id| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if ui.get_sync_busy() {
                return;
            }
            let result = db::get_subscription(&state.borrow().conn, i64::from(id));
            let subscription = match result {
                Ok(Some(subscription)) => subscription,
                _ => {
                    ui.set_integration_status("无法读取该连接，请刷新后重试".into());
                    return;
                }
            };
            ui.set_sync_busy(true);
            ui.set_integration_status(format!("正在同步“{}”…", subscription.name).into());
            let ui_weak = ui.as_weak();
            std::thread::spawn(move || {
                let result = db::open().and_then(|conn| {
                    integrations::sync_ics_subscription_with_result(&conn, &subscription)
                });
                let message = match result {
                    Ok(report) => format!(
                        "“{}”同步成功：导入 {} 条，清理取消日程 {} 条",
                        subscription.name, report.imported_events, report.removed_cancelled_events
                    ),
                    Err(error) => format!(
                        "“{}”同步失败：{}",
                        subscription.name,
                        integrations::safe_error(&error.to_string())
                    ),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_sync_busy(false);
                        ui.set_integration_status(message.into());
                        ui.invoke_refresh_external_data();
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_refresh_external_data(move || {
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
}
