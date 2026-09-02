//! 主窗口显示策略。
//!
//! 页面导航和原生窗口几何必须彼此独立。尤其不能在切换 Slint 条件布局后
//! 再次调用 `set_maximized(true)`，否则 Windows 会出现一次可见的恢复/最大化动画。

use crate::AppWindow;
use rili::window_policy::show_plan;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;

/// 切换页面只更新页面属性，绝不读写窗口尺寸、位置或最大化状态。
pub(crate) fn set_main_view_mode(ui: &AppWindow, mode: i32) {
    ui.set_view_mode(mode);
}

/// 显示并聚焦主窗口。仅隐藏窗口需要 `show()`，仅最小化窗口需要恢复；
/// 已显示且最大化的窗口不会经过任何几何状态切换。
pub(crate) fn show_and_focus_main_window(ui: &AppWindow) {
    let plan = show_plan(ui.window().is_visible(), ui.window().is_minimized());
    if plan.show_hidden_window {
        let _ = ui.show();
    }
    if plan.restore_minimized_window {
        ui.window().set_minimized(false);
    }
    let _ = ui
        .window()
        .with_winit_window(|native| native.focus_window());
}
