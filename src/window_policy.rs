//! 与 GUI 工具包无关的窗口导航策略，便于在无图形环境中做回归测试。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowPlan {
    pub show_hidden_window: bool,
    pub restore_minimized_window: bool,
}

pub fn show_plan(visible: bool, minimized: bool) -> ShowPlan {
    ShowPlan {
        show_hidden_window: !visible,
        restore_minimized_window: minimized,
    }
}

/// 大部分页面切换只需切换已有模型。只有时间轴跨度发生变化，或进入周/日视图时
/// 锚点确实改变，才需要重新构造日期模型。
pub fn navigation_refresh_needed(previous_mode: i32, next_mode: i32, anchor_changed: bool) -> bool {
    const LAZY_DATA_MODES: [i32; 7] = [4, 5, 6, 7, 8, 9, 12];
    anchor_changed
        || (previous_mode == 3) != (next_mode == 3)
        || (previous_mode != next_mode && LAZY_DATA_MODES.contains(&next_mode))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// 计算固定贴靠任务栏的面板位置。边缘值沿用 Windows AppBar：
/// 0 左、1 上、2 右、3 下。
pub fn taskbar_panel_position(
    taskbar: ScreenRect,
    edge: u32,
    panel_width: i32,
    panel_height: i32,
    gap: i32,
) -> (i32, i32) {
    match edge {
        0 => (taskbar.right + gap, taskbar.bottom - panel_height - gap),
        1 => (taskbar.right - panel_width - gap, taskbar.bottom + gap),
        2 => (
            taskbar.left - panel_width - gap,
            taskbar.bottom - panel_height - gap,
        ),
        _ => (
            taskbar.right - panel_width - gap,
            taskbar.top - panel_height - gap,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_window_needs_no_display_state_transition() {
        assert_eq!(
            show_plan(true, false),
            ShowPlan {
                show_hidden_window: false,
                restore_minimized_window: false,
            }
        );
    }

    #[test]
    fn hidden_and_minimized_states_have_independent_actions() {
        assert!(show_plan(false, false).show_hidden_window);
        assert!(!show_plan(false, false).restore_minimized_window);
        assert!(!show_plan(true, true).show_hidden_window);
        assert!(show_plan(true, true).restore_minimized_window);
    }

    #[test]
    fn ordinary_view_switch_reuses_existing_models() {
        assert!(!navigation_refresh_needed(0, 2, false));
        assert!(navigation_refresh_needed(0, 4, false));
        assert!(navigation_refresh_needed(0, 5, false));
        assert!(navigation_refresh_needed(0, 12, false));
        assert!(navigation_refresh_needed(0, 2, true));
        assert!(navigation_refresh_needed(2, 3, false));
        assert!(navigation_refresh_needed(3, 2, false));
    }

    #[test]
    fn quick_panel_is_anchored_above_bottom_taskbar() {
        let taskbar = ScreenRect {
            left: 0,
            top: 1040,
            right: 1920,
            bottom: 1080,
        };
        assert_eq!(taskbar_panel_position(taskbar, 3, 460, 640, 8), (1452, 392));
    }

    #[test]
    fn quick_panel_tracks_each_taskbar_edge() {
        let taskbar = ScreenRect {
            left: 0,
            top: 0,
            right: 48,
            bottom: 1080,
        };
        assert_eq!(taskbar_panel_position(taskbar, 0, 460, 640, 8), (56, 432));
        assert_eq!(taskbar_panel_position(taskbar, 2, 460, 640, 8), (-468, 432));
    }
}
