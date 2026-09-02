//! 系统托盘图标与菜单。

use crate::*;

/// 托盘图标 + 右键菜单（显示主窗口 / 开关桌面挂件 / 退出）。
pub(crate) struct TrayHandles {
    pub(crate) _tray: TrayIcon,
    pub(crate) show_id: tray_icon::menu::MenuId,
    pub(crate) widget_id: tray_icon::menu::MenuId,
    pub(crate) quit_id: tray_icon::menu::MenuId,
}

pub(crate) fn build_tray_icon() -> Result<TrayHandles> {
    let icon = build_tray_icon_image().context("生成托盘图标失败")?;

    let menu = Menu::new();
    let show_item = MenuItem::new("显示主窗口", true, None);
    let widget_item = MenuItem::new("打开/关闭桌面挂件", true, None);
    let quit_item = MenuItem::new("退出", true, None);
    menu.append(&show_item).context("添加托盘菜单项失败")?;
    menu.append(&widget_item).context("添加托盘菜单项失败")?;
    menu.append(&PredefinedMenuItem::separator())
        .context("添加托盘菜单分隔线失败")?;
    menu.append(&quit_item).context("添加托盘菜单项失败")?;

    let show_id = show_item.id().clone();
    let widget_id = widget_item.id().clone();
    let quit_id = quit_item.id().clone();

    let tray = TrayIconBuilder::new()
        .with_tooltip("日历")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .context("创建系统托盘图标失败")?;

    Ok(TrayHandles {
        _tray: tray,
        show_id,
        widget_id,
        quit_id,
    })
}

pub(crate) fn build_tray_icon_image() -> Result<Icon> {
    let (width, height) = (32u32, 32u32);
    let rgba = include_bytes!("../assets/timehub-logo-32.rgba").to_vec();
    Icon::from_rgba(rgba, width, height).context("RGBA 转换为托盘图标失败")
}
