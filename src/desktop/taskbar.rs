//! 原生窗口行为、快速面板定位与 Windows 任务栏时钟监听。

use crate::*;

/// Desktop widgets are interactive top-level windows, but they are auxiliary
/// TimeHub surfaces rather than six independent taskbar applications. On
/// Windows, TOOLWINDOW is the native contract for this exact behavior.
#[cfg(target_os = "windows")]
pub(crate) fn remove_widget_from_taskbar<C: ComponentHandle>(component: &C) {
    use slint::winit_030::winit::platform::windows::WindowExtWindows;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_APPWINDOW: isize = 0x0004_0000;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowLongPtrW(hwnd: *mut c_void, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: *mut c_void, index: i32, value: isize) -> isize;
        fn SetWindowPos(
            hwnd: *mut c_void,
            insert_after: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    let applied = component.window().with_winit_window(|native| {
        native.set_skip_taskbar(true);
        let Ok(window_handle) = native.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(window_handle) = window_handle.as_raw() else {
            return;
        };
        let hwnd = window_handle.hwnd.get() as *mut c_void;
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW,
            );
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    });
    if applied.is_none() {
        error_reporter::report("桌面挂件窗口尚未就绪", &"等待下一次样式重试");
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn remove_widget_from_taskbar<C: ComponentHandle>(_component: &C) {}

/// Show the quick panel as a real foreground popup. `show()` alone is not
/// sufficient for a non-taskbar, non-pinned tool window: Windows may keep it
/// behind the currently active application after the taskbar clock is clicked.
pub(crate) fn show_and_focus_quick_panel(quick: &QuickPanelWindow) {
    show_and_focus_quick_panel_at(quick, None);
}

pub(crate) fn show_and_focus_quick_panel_at(quick: &QuickPanelWindow, anchor: Option<TaskbarRect>) {
    if let Some(taskbar) = anchor {
        position_quick_panel_at_rect(quick, taskbar);
    } else {
        position_quick_panel_at_taskbar(quick);
    }
    quick.window().set_minimized(false);
    let _ = quick.show();
    remove_widget_from_taskbar(quick);
    let _ = quick
        .window()
        .with_winit_window(|native| native.focus_window());
}

/// 关闭命中测试后，鼠标消息会直接交给挂件下方的窗口。
/// 该状态只能从主窗口的“桌面卡片”管理器恢复，避免卡片本身拦截点击。
pub(crate) fn set_widget_click_through<C: ComponentHandle>(component: &C, enabled: bool) {
    let _ = component.window().with_winit_window(|native| {
        if let Err(error) = native.set_cursor_hittest(!enabled) {
            error_reporter::report("设置桌面挂件鼠标穿透失败", &error);
        }
    });
}

#[derive(Clone, Copy)]
pub(crate) struct TaskbarRect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
    pub(crate) edge: u32,
    pub(crate) scale: f32,
}

#[cfg(target_os = "windows")]
pub(crate) fn windows_taskbar_rect() -> Option<TaskbarRect> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct AppBarData {
        cb_size: u32,
        hwnd: *mut c_void,
        callback_message: u32,
        edge: u32,
        rect: Rect,
        l_param: isize,
    }

    const ABM_GETTASKBARPOS: u32 = 0x0000_0005;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHAppBarMessage(message: u32, data: *mut AppBarData) -> usize;
    }

    let mut data = AppBarData {
        cb_size: std::mem::size_of::<AppBarData>() as u32,
        hwnd: std::ptr::null_mut(),
        callback_message: 0,
        edge: 0,
        rect: Rect::default(),
        l_param: 0,
    };
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } == 0 {
        return None;
    }
    Some(TaskbarRect {
        left: data.rect.left,
        top: data.rect.top,
        right: data.rect.right,
        bottom: data.rect.bottom,
        edge: data.edge,
        scale: windows_primary_scale(),
    })
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn windows_taskbar_rect() -> Option<TaskbarRect> {
    None
}

/// Windows 11 在副屏上使用 `Shell_SecondaryTrayWnd`，且时钟通常是 XAML
/// 元素而不是传统 `TrayClockWClass`。枚举所有任务栏顶层窗口，才能让右下角
/// 时间入口在任意显示器上都可用。
#[cfg(target_os = "windows")]
pub(crate) fn windows_taskbar_rects() -> Vec<TaskbarRect> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }
    type EnumWindowsProc = Option<unsafe extern "system" fn(*mut c_void, isize) -> i32>;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(callback: EnumWindowsProc, l_param: isize) -> i32;
        fn GetClassNameW(hwnd: *mut c_void, name: *mut u16, count: i32) -> i32;
        fn GetWindowRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn MonitorFromWindow(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
        fn GetDpiForWindow(hwnd: *mut c_void) -> u32;
    }

    unsafe extern "system" fn collect(hwnd: *mut c_void, l_param: isize) -> i32 {
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut class_name = [0u16; 64];
        let length = unsafe { GetClassNameW(hwnd, class_name.as_mut_ptr(), 64) };
        if length <= 0 {
            return 1;
        }
        let class_name = String::from_utf16_lossy(&class_name[..length as usize]);
        if class_name != "Shell_TrayWnd" && class_name != "Shell_SecondaryTrayWnd" {
            return 1;
        }

        let mut rect = Rect::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return 1;
        }
        let monitor = unsafe { MonitorFromWindow(hwnd, 2) }; // MONITOR_DEFAULTTONEAREST
        let mut monitor_info = MonitorInfo {
            cb_size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
        };
        let has_monitor =
            !monitor.is_null() && unsafe { GetMonitorInfoW(monitor, &mut monitor_info) } != 0;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let edge = if width >= height {
            if has_monitor
                && (rect.top - monitor_info.monitor.top).abs()
                    <= (monitor_info.monitor.bottom - rect.bottom).abs()
            {
                1 // top
            } else {
                3 // bottom
            }
        } else if has_monitor
            && (rect.left - monitor_info.monitor.left).abs()
                <= (monitor_info.monitor.right - rect.right).abs()
        {
            0 // left
        } else {
            2 // right
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let taskbars = unsafe { &mut *(l_param as *mut Vec<TaskbarRect>) };
        taskbars.push(TaskbarRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            edge,
            scale: dpi as f32 / 96.0,
        });
        1
    }

    let mut taskbars = Vec::new();
    unsafe {
        EnumWindows(
            Some(collect),
            &mut taskbars as *mut Vec<TaskbarRect> as isize,
        );
    }
    taskbars
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn windows_taskbar_rects() -> Vec<TaskbarRect> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub(crate) fn windows_primary_scale() -> f32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForSystem() -> u32;
    }
    (unsafe { GetDpiForSystem() }.max(96) as f32) / 96.0
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn windows_primary_scale() -> f32 {
    1.0
}

pub(crate) fn position_quick_panel_at_taskbar(quick: &QuickPanelWindow) {
    let Some(taskbar) = windows_taskbar_rect() else {
        return;
    };
    position_quick_panel_at_rect(quick, taskbar);
}

pub(crate) fn position_quick_panel_at_rect(quick: &QuickPanelWindow, taskbar: TaskbarRect) {
    let scale = taskbar.scale;
    let panel_width = (460.0 * scale).round() as i32;
    let panel_height = (640.0 * scale).round() as i32;
    let gap = (8.0 * scale).round() as i32;
    let (x, y) = rili::window_policy::taskbar_panel_position(
        rili::window_policy::ScreenRect {
            left: taskbar.left,
            top: taskbar.top,
            right: taskbar.right,
            bottom: taskbar.bottom,
        },
        taskbar.edge,
        panel_width,
        panel_height,
        gap,
    );
    quick
        .window()
        .set_position(slint::PhysicalPosition::new(x, y));
}

/// Place a notification against the current screen work area's taskbar edge.
/// For the usual bottom taskbar this is the screen's bottom-right corner,
/// immediately above the taskbar. Unlike the quick panel, showing a
/// notification must not steal keyboard focus from the user's current app.
pub(crate) fn show_screen_notification(notification: &NotificationWindow) {
    if let Some(taskbar) = windows_taskbar_rect() {
        let scale = taskbar.scale;
        let width = (410.0 * scale).round() as i32;
        let height = (76.0 * scale).round() as i32;
        let gap = (12.0 * scale).round() as i32;
        let (x, y) = rili::window_policy::taskbar_panel_position(
            rili::window_policy::ScreenRect {
                left: taskbar.left,
                top: taskbar.top,
                right: taskbar.right,
                bottom: taskbar.bottom,
            },
            taskbar.edge,
            width,
            height,
            gap,
        );
        notification
            .window()
            .set_position(slint::PhysicalPosition::new(x, y));
    }
    notification.window().set_minimized(false);
    let _ = notification.show();
    remove_widget_from_taskbar(notification);
}

pub(crate) static TASKBAR_CLOCK_HOOK_ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_CLICK_HANDLER: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> =
    std::sync::OnceLock::new();
#[cfg(target_os = "windows")]
const MAX_TASKBAR_CLOCKS: usize = 8;
#[cfg(target_os = "windows")]
struct AtomicTaskbarClockRect {
    pub(crate) left: AtomicI32,
    pub(crate) top: AtomicI32,
    pub(crate) right: AtomicI32,
    pub(crate) bottom: AtomicI32,
    pub(crate) edge: AtomicI32,
    scale_milli: AtomicI32,
}
#[cfg(target_os = "windows")]
impl AtomicTaskbarClockRect {
    const fn new() -> Self {
        Self {
            left: AtomicI32::new(0),
            top: AtomicI32::new(0),
            right: AtomicI32::new(0),
            bottom: AtomicI32::new(0),
            edge: AtomicI32::new(3),
            scale_milli: AtomicI32::new(1000),
        }
    }
}
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_RECTS: [AtomicTaskbarClockRect; MAX_TASKBAR_CLOCKS] =
    [const { AtomicTaskbarClockRect::new() }; MAX_TASKBAR_CLOCKS];
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_RECT_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_CLICKED_INDEX: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "windows")]
pub(crate) fn last_clicked_taskbar_anchor() -> Option<TaskbarRect> {
    let index = TASKBAR_CLOCK_CLICKED_INDEX.load(Ordering::Acquire);
    if index >= TASKBAR_CLOCK_RECT_COUNT.load(Ordering::Acquire) {
        return None;
    }
    let anchor = &TASKBAR_CLOCK_RECTS[index];
    Some(TaskbarRect {
        left: anchor.left.load(Ordering::Relaxed),
        top: anchor.top.load(Ordering::Relaxed),
        right: anchor.right.load(Ordering::Relaxed),
        bottom: anchor.bottom.load(Ordering::Relaxed),
        edge: anchor.edge.load(Ordering::Relaxed).max(0) as u32,
        scale: anchor.scale_milli.load(Ordering::Relaxed).max(1) as f32 / 1000.0,
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn set_taskbar_clock_click_handler(handler: impl Fn() + Send + Sync + 'static) {
    let _ = TASKBAR_CLOCK_CLICK_HANDLER.set(Box::new(handler));
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn set_taskbar_clock_click_handler(_handler: impl Fn() + Send + Sync + 'static) {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn last_clicked_taskbar_anchor() -> Option<TaskbarRect> {
    None
}

/// Keeps the Windows clock itself visible and only takes over its left-click entry.
/// The hit area follows the actual taskbar edge and DPI instead of drawing a covering window.
#[cfg(target_os = "windows")]
pub(crate) fn update_taskbar_clock_hit_rect() {
    let mut taskbars = windows_taskbar_rects();
    if taskbars.is_empty() {
        if let Some(taskbar) = windows_taskbar_rect() {
            taskbars.push(taskbar);
        }
    }
    let count = taskbars.len().min(MAX_TASKBAR_CLOCKS);
    for (index, taskbar) in taskbars.into_iter().take(count).enumerate() {
        let clock_width = (112.0 * taskbar.scale).round() as i32;
        let clock_height = (64.0 * taskbar.scale).round() as i32;
        let horizontal = taskbar.edge == 1 || taskbar.edge == 3;
        let (left, top, right, bottom) = if horizontal {
            (
                (taskbar.right - clock_width).max(taskbar.left),
                taskbar.top,
                taskbar.right,
                taskbar.bottom,
            )
        } else {
            (
                taskbar.left,
                (taskbar.bottom - clock_height).max(taskbar.top),
                taskbar.right,
                taskbar.bottom,
            )
        };
        let target = &TASKBAR_CLOCK_RECTS[index];
        target.left.store(left, Ordering::Relaxed);
        target.top.store(top, Ordering::Relaxed);
        target.right.store(right, Ordering::Relaxed);
        target.bottom.store(bottom, Ordering::Relaxed);
        target.edge.store(taskbar.edge as i32, Ordering::Relaxed);
        target
            .scale_milli
            .store((taskbar.scale * 1000.0).round() as i32, Ordering::Relaxed);
    }
    TASKBAR_CLOCK_RECT_COUNT.store(count, Ordering::Release);
}

#[cfg(target_os = "windows")]
pub(crate) fn spawn_taskbar_clock_click_hook() {
    use std::ffi::c_void;

    const WH_MOUSE_LL: i32 = 14;
    const WM_LBUTTONDOWN: usize = 0x0201;
    const WM_LBUTTONUP: usize = 0x0202;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct MouseHookData {
        point: Point,
        mouse_data: u32,
        flags: u32,
        time: u32,
        extra_info: usize,
    }
    #[repr(C)]
    struct Message {
        hwnd: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        point: Point,
        private_data: u32,
    }
    type HookProc = Option<unsafe extern "system" fn(i32, usize, isize) -> isize>;

    unsafe extern "system" fn mouse_proc(code: i32, w_param: usize, l_param: isize) -> isize {
        if code >= 0 && TASKBAR_CLOCK_HOOK_ENABLED.load(Ordering::Relaxed) {
            let data = unsafe { &*(l_param as *const MouseHookData) };
            let count = TASKBAR_CLOCK_RECT_COUNT.load(Ordering::Acquire);
            for (index, rect) in TASKBAR_CLOCK_RECTS.iter().enumerate().take(count) {
                let inside = data.point.x >= rect.left.load(Ordering::Relaxed)
                    && data.point.x < rect.right.load(Ordering::Relaxed)
                    && data.point.y >= rect.top.load(Ordering::Relaxed)
                    && data.point.y < rect.bottom.load(Ordering::Relaxed);
                if inside && (w_param == WM_LBUTTONDOWN || w_param == WM_LBUTTONUP) {
                    if w_param == WM_LBUTTONUP {
                        TASKBAR_CLOCK_CLICKED_INDEX.store(index, Ordering::Relaxed);
                        if let Some(handler) = TASKBAR_CLOCK_CLICK_HANDLER.get() {
                            handler();
                        }
                    }
                    return 1;
                }
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param) }
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowsHookExW(
            hook_id: i32,
            callback: HookProc,
            module: *mut c_void,
            thread_id: u32,
        ) -> *mut c_void;
        fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: isize) -> isize;
        fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
        fn GetMessageW(message: *mut Message, hwnd: *mut c_void, min: u32, max: u32) -> i32;
        fn TranslateMessage(message: *const Message) -> i32;
        fn DispatchMessageW(message: *const Message) -> isize;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }

    std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        if hook.is_null() {
            error_reporter::report("Windows 系统时钟点击接管启动失败", &"系统挂钩初始化失败");
            return;
        }
        let mut message: Message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
    });
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn update_taskbar_clock_hit_rect() {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn spawn_taskbar_clock_click_hook() {}
