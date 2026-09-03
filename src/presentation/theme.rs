//! 主题、字体与无障碍偏好同步。

use crate::*;

/// 设计规范中的 8 色强调色，同时应用到主窗口和桌面挂件的 `Theme` 全局。
pub(crate) fn apply_theme(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    index: i32,
) {
    struct Palette {
        accent: (u8, u8, u8),
        today_bg: (u8, u8, u8),
    }
    const PALETTES: [Palette; 8] = [
        Palette {
            accent: (0x2e, 0x6b, 0xe6),
            today_bg: (0x2e, 0x6b, 0xe6),
        },
        Palette {
            accent: (0x0e, 0x9f, 0x6e),
            today_bg: (0x0e, 0x9f, 0x6e),
        },
        Palette {
            accent: (0xc7, 0x77, 0x00),
            today_bg: (0xc7, 0x77, 0x00),
        },
        Palette {
            accent: (0xd9, 0x3a, 0x49),
            today_bg: (0xd9, 0x3a, 0x49),
        },
        Palette {
            accent: (0x7c, 0x4d, 0xff),
            today_bg: (0x7c, 0x4d, 0xff),
        },
        Palette {
            accent: (0x08, 0x91, 0xb2),
            today_bg: (0x08, 0x91, 0xb2),
        },
        Palette {
            accent: (0xdb, 0x27, 0x77),
            today_bg: (0xdb, 0x27, 0x77),
        },
        Palette {
            accent: (0x65, 0xa3, 0x0d),
            today_bg: (0x65, 0xa3, 0x0d),
        },
    ];
    let palette = &PALETTES[(index.max(0) as usize) % PALETTES.len()];
    let accent = slint::Color::from_rgb_u8(palette.accent.0, palette.accent.1, palette.accent.2);
    let today_bg =
        slint::Color::from_rgb_u8(palette.today_bg.0, palette.today_bg.1, palette.today_bg.2);

    let apply = |theme: Theme<'_>| {
        theme.set_accent(accent);
        theme.set_today_bg(today_bg);
    };
    apply(ui.global::<Theme>());
    apply(widget.global::<Theme>());
    apply(quick_panel.global::<Theme>());
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            apply(windows.calendar.global::<Theme>());
            apply(windows.events.global::<Theme>());
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                apply(editor.global::<Theme>());
            }
            apply(windows.countdown.global::<Theme>());
            apply(windows.clock.global::<Theme>());
            apply(windows.weather.global::<Theme>());
            apply(windows.focus.global::<Theme>());
            apply(windows.todo.global::<Theme>());
            for window in windows.notes.borrow().iter() {
                apply(window.global::<Theme>());
            }
        }
    });
}

/// 同步亮色、暗色和跟随系统模式到所有独立窗口的主题全局。
pub(crate) fn apply_visual_theme(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    mode: i32,
) {
    let mode = mode.clamp(0, 2);
    ui.global::<Theme>().set_theme_mode(mode);
    widget.global::<Theme>().set_theme_mode(mode);
    quick_panel.global::<Theme>().set_theme_mode(mode);
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            windows.calendar.global::<Theme>().set_theme_mode(mode);
            windows.events.global::<Theme>().set_theme_mode(mode);
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                editor.global::<Theme>().set_theme_mode(mode);
            }
            windows.countdown.global::<Theme>().set_theme_mode(mode);
            windows.clock.global::<Theme>().set_theme_mode(mode);
            windows.weather.global::<Theme>().set_theme_mode(mode);
            windows.focus.global::<Theme>().set_theme_mode(mode);
            windows.todo.global::<Theme>().set_theme_mode(mode);
            for window in windows.notes.borrow().iter() {
                window.global::<Theme>().set_theme_mode(mode);
            }
        }
    });
}

pub(crate) fn apply_accessibility_preferences(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    font_size: i32,
    density: i32,
    reduce_motion: bool,
) {
    let font_delta = font_size.clamp(12, 16) - 13;
    let density = density.clamp(0, 2);
    for theme in [
        ui.global::<Theme>(),
        widget.global::<Theme>(),
        quick_panel.global::<Theme>(),
    ] {
        theme.set_font_delta(font_delta);
        theme.set_density_mode(density);
        theme.set_reduce_motion(reduce_motion);
    }
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            for theme in [
                windows.calendar.global::<Theme>(),
                windows.events.global::<Theme>(),
                windows.countdown.global::<Theme>(),
                windows.clock.global::<Theme>(),
                windows.weather.global::<Theme>(),
                windows.focus.global::<Theme>(),
                windows.todo.global::<Theme>(),
            ] {
                theme.set_font_delta(font_delta);
                theme.set_density_mode(density);
                theme.set_reduce_motion(reduce_motion);
            }
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                let theme = editor.global::<Theme>();
                theme.set_font_delta(font_delta);
                theme.set_density_mode(density);
                theme.set_reduce_motion(reduce_motion);
            }
            for window in windows.notes.borrow().iter() {
                let theme = window.global::<Theme>();
                theme.set_font_delta(font_delta);
                theme.set_density_mode(density);
                theme.set_reduce_motion(reduce_motion);
            }
        }
    });
}

pub(crate) fn apply_font_family(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    family: &str,
) {
    for theme in [
        ui.global::<Theme>(),
        widget.global::<Theme>(),
        quick_panel.global::<Theme>(),
    ] {
        theme.set_font_family(family.into());
    }
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            for theme in [
                windows.calendar.global::<Theme>(),
                windows.events.global::<Theme>(),
                windows.countdown.global::<Theme>(),
                windows.clock.global::<Theme>(),
                windows.weather.global::<Theme>(),
                windows.focus.global::<Theme>(),
                windows.todo.global::<Theme>(),
            ] {
                theme.set_font_family(family.into());
            }
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                editor.global::<Theme>().set_font_family(family.into());
            }
            for window in windows.notes.borrow().iter() {
                window.global::<Theme>().set_font_family(family.into());
            }
        }
    });
}
