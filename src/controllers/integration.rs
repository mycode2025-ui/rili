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
        ui.on_add_subscription(move |name, url| {
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
        ui.on_sync_subscriptions(move || {
            if let Some(ui) = ui_weak.upgrade() {
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
        ui.on_refresh_external_data(move || {
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
}
