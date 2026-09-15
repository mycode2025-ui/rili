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

/// Whether navigation needs the expensive calendar aggregate. Non-calendar
/// pages load their own data and must never rebuild the calendar as a side effect.
pub fn navigation_refresh_needed(previous_mode: i32, next_mode: i32, anchor_changed: bool) -> bool {
    matches!(next_mode, 0 | 1 | 2 | 3 | 5)
        && (anchor_changed
            || (previous_mode == 3) != (next_mode == 3)
            || (previous_mode != next_mode && next_mode == 5))
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

/// 将窗口系统返回的物理像素尺寸转换成可跨显示器保存的逻辑尺寸。
pub fn physical_to_logical_size(width: u32, height: u32, scale_factor: f32) -> (f32, f32) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (width as f32 / scale, height as f32 / scale)
}

/// A one-second UI refresh is useful only while a surface displaying live time
/// is visible. Hidden applications fall back to a sparse maintenance check.
pub fn realtime_refresh_needed(
    main_visible: bool,
    quick_panel_visible: bool,
    events_visible: bool,
    clock_visible: bool,
    focus_visible: bool,
) -> bool {
    main_visible || quick_panel_visible || events_visible || clock_visible || focus_visible
}

pub fn idle_maintenance_due(second: u32) -> bool {
    second.is_multiple_of(10)
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
        assert!(!navigation_refresh_needed(0, 4, false));
        assert!(navigation_refresh_needed(0, 5, false));
        assert!(!navigation_refresh_needed(0, 12, false));
        assert!(navigation_refresh_needed(0, 2, true));
        assert!(navigation_refresh_needed(2, 3, false));
        assert!(navigation_refresh_needed(3, 2, false));
        for previous in 0..=12 {
            for page in [4, 6, 7, 8, 9, 10, 11, 12] {
                assert!(!navigation_refresh_needed(previous, page, true));
            }
        }
    }

    #[test]
    fn quick_panel_is_anchored_above_bottom_taskbar() {
        let taskbar = ScreenRect {
            left: 0,
            top: 1040,
            right: 1920,
            bottom: 1080,
        };
        assert_eq!(taskbar_panel_position(taskbar, 3, 424, 580, 0), (1496, 460));
    }

    #[test]
    fn quick_panel_tracks_each_taskbar_edge() {
        let taskbar = ScreenRect {
            left: 0,
            top: 0,
            right: 48,
            bottom: 1080,
        };
        assert_eq!(taskbar_panel_position(taskbar, 0, 424, 580, 0), (48, 500));
        assert_eq!(taskbar_panel_position(taskbar, 2, 424, 580, 0), (-424, 500));
    }

    #[test]
    fn widget_logical_size_stays_constant_across_monitor_dpi() {
        assert_eq!(physical_to_logical_size(304, 244, 1.0), (304.0, 244.0));
        assert_eq!(physical_to_logical_size(456, 366, 1.5), (304.0, 244.0));
        assert_eq!(physical_to_logical_size(608, 488, 2.0), (304.0, 244.0));
        assert_eq!(physical_to_logical_size(304, 244, 0.0), (304.0, 244.0));
    }

    #[test]
    fn hidden_surfaces_skip_realtime_work_but_keep_sparse_maintenance() {
        assert!(!realtime_refresh_needed(false, false, false, false, false));
        assert!(realtime_refresh_needed(false, false, false, true, false));
        assert!(!idle_maintenance_due(9));
        assert!(idle_maintenance_due(10));
    }
}
