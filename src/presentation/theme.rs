//! 主题、字体与无障碍偏好同步。

use crate::*;

struct AccentPalette {
    accent: (u8, u8, u8),
    today_bg: (u8, u8, u8),
}

const ACCENT_PALETTES: [AccentPalette; 8] = [
    AccentPalette {
        accent: (0x2e, 0x6b, 0xe6),
        today_bg: (0x2e, 0x6b, 0xe6),
    },
    AccentPalette {
        accent: (0x0e, 0x9f, 0x6e),
        today_bg: (0x0e, 0x9f, 0x6e),
    },
    AccentPalette {
        accent: (0xc7, 0x77, 0x00),
        today_bg: (0xc7, 0x77, 0x00),
    },
    AccentPalette {
        accent: (0xd9, 0x3a, 0x49),
        today_bg: (0xd9, 0x3a, 0x49),
    },
    AccentPalette {
        accent: (0x7c, 0x4d, 0xff),
        today_bg: (0x7c, 0x4d, 0xff),
    },
    AccentPalette {
        accent: (0x08, 0x91, 0xb2),
        today_bg: (0x08, 0x91, 0xb2),
    },
    AccentPalette {
        accent: (0xdb, 0x27, 0x77),
        today_bg: (0xdb, 0x27, 0x77),
    },
    AccentPalette {
        accent: (0x65, 0xa3, 0x0d),
        today_bg: (0x65, 0xa3, 0x0d),
    },
];

pub(crate) fn set_theme_accent(theme: Theme<'_>, index: i32) {
    let palette = &ACCENT_PALETTES[(index.max(0) as usize) % ACCENT_PALETTES.len()];
    theme.set_accent(slint::Color::from_rgb_u8(
        palette.accent.0,
        palette.accent.1,
        palette.accent.2,
    ));
    theme.set_today_bg(slint::Color::from_rgb_u8(
        palette.today_bg.0,
        palette.today_bg.1,
        palette.today_bg.2,
    ));
}

/// 设计规范中的 8 色强调色，同时应用到主窗口和桌面挂件的 `Theme` 全局。
pub(crate) fn apply_theme(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    index: i32,
) {
    let apply = |theme: Theme<'_>| set_theme_accent(theme, index);
    apply(ui.global::<Theme>());
    apply(widget.global::<Theme>());
    apply(quick_panel.global::<Theme>());
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            let apply_widget = |theme: Theme<'_>| {
                if theme.get_widget_accent_override() < 0 {
                    apply(theme);
                }
            };
            apply_widget(windows.calendar.global::<Theme>());
            apply_widget(windows.events.global::<Theme>());
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                apply(editor.global::<Theme>());
            }
            if let Some(editor) = windows.appearance_editor.borrow().as_ref() {
                apply(editor.global::<Theme>());
            }
            apply_widget(windows.countdown.global::<Theme>());
            apply_widget(windows.clock.global::<Theme>());
            apply_widget(windows.weather.global::<Theme>());
            apply_widget(windows.focus.global::<Theme>());
            apply_widget(windows.todo.global::<Theme>());
            for window in windows.notes.borrow().iter() {
                apply_widget(window.global::<Theme>());
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
            let apply_widget = |theme: Theme<'_>| {
                if theme.get_widget_theme_override() == 0 {
                    theme.set_theme_mode(mode);
                }
            };
            apply_widget(windows.calendar.global::<Theme>());
            apply_widget(windows.events.global::<Theme>());
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                editor.global::<Theme>().set_theme_mode(mode);
            }
            if let Some(editor) = windows.appearance_editor.borrow().as_ref() {
                editor.global::<Theme>().set_theme_mode(mode);
            }
            apply_widget(windows.countdown.global::<Theme>());
            apply_widget(windows.clock.global::<Theme>());
            apply_widget(windows.weather.global::<Theme>());
            apply_widget(windows.focus.global::<Theme>());
            apply_widget(windows.todo.global::<Theme>());
            for window in windows.notes.borrow().iter() {
                apply_widget(window.global::<Theme>());
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
            if let Some(editor) = windows.appearance_editor.borrow().as_ref() {
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
            if let Some(editor) = windows.appearance_editor.borrow().as_ref() {
                editor.global::<Theme>().set_font_family(family.into());
            }
            for window in windows.notes.borrow().iter() {
                window.global::<Theme>().set_font_family(family.into());
            }
        }
    });
}

fn with_desktop_widget_theme(kind: &str, mut action: impl FnMut(Theme<'_>)) -> bool {
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        let Some(windows) = slot.borrow().as_ref().cloned() else {
            return false;
        };
        let base_kind = kind.split(':').next().unwrap_or(kind);
        match base_kind {
            "calendar" => action(windows.calendar.global::<Theme>()),
            "events" => action(windows.events.global::<Theme>()),
            "countdown" => action(windows.countdown.global::<Theme>()),
            "clock" => action(windows.clock.global::<Theme>()),
            "weather" => action(windows.weather.global::<Theme>()),
            "focus" => action(windows.focus.global::<Theme>()),
            "todo" => action(windows.todo.global::<Theme>()),
            "notes" => {
                for note in windows.notes.borrow().iter() {
                    action(note.global::<Theme>());
                }
            }
            note_kind if note_kind.starts_with("note_") => {
                let id = note_kind.trim_start_matches("note_").parse::<i32>().ok();
                if let Some(note) = windows
                    .notes
                    .borrow()
                    .iter()
                    .find(|note| Some(note.get_note_id()) == id)
                {
                    action(note.global::<Theme>());
                }
            }
            _ => return false,
        }
        true
    })
}

pub(crate) fn apply_desktop_widget_style(
    kind: &str,
    opacity: i32,
    theme_override: i32,
    accent_override: i32,
    app_theme: i32,
    app_accent: i32,
) -> bool {
    with_desktop_widget_theme(kind, |theme| {
        let theme_override = theme_override.clamp(0, 2);
        let accent_override = accent_override.clamp(-1, 7);
        theme.set_widget_opacity(opacity.clamp(35, 100));
        theme.set_widget_theme_override(theme_override);
        theme.set_widget_accent_override(accent_override);
        theme.set_theme_mode(if theme_override == 0 {
            app_theme.clamp(0, 2)
        } else {
            theme_override
        });
        set_theme_accent(
            theme,
            if accent_override < 0 {
                app_accent
            } else {
                accent_override
            },
        );
    })
}

pub(crate) fn desktop_widget_style(conn: &Connection, kind: &str) -> (i32, i32, i32) {
    let (global_opacity, global_theme, global_accent) = desktop_widget_global_style(conn);
    let (opacity_override, theme_override, accent_override) =
        desktop_widget_instance_overrides(conn, kind);
    (
        if opacity_override < 0 {
            global_opacity
        } else {
            opacity_override
        },
        if theme_override < 0 {
            global_theme
        } else {
            theme_override
        },
        if accent_override < 0 {
            global_accent
        } else {
            accent_override
        },
    )
}

pub(crate) fn desktop_widget_global_style(conn: &Connection) -> (i32, i32, i32) {
    // Treat the former calendar-scoped values as a one-time-compatible default
    // so users do not see their chosen opacity jump after this model change.
    let legacy_opacity =
        db::get_setting(conn, "widget_calendar_opacity", "80").unwrap_or_else(|_| "80".to_string());
    let legacy_theme =
        db::get_setting(conn, "widget_calendar_theme", "0").unwrap_or_else(|_| "0".to_string());
    let legacy_accent =
        db::get_setting(conn, "widget_calendar_accent", "-1").unwrap_or_else(|_| "-1".to_string());
    let opacity = db::get_setting(conn, "desktop_widget_opacity", &legacy_opacity)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(80)
        .clamp(35, 100);
    let theme = db::get_setting(conn, "desktop_widget_theme", &legacy_theme)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
        .clamp(0, 2);
    let accent = db::get_setting(conn, "desktop_widget_accent", &legacy_accent)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(-1)
        .clamp(-1, 7);
    (opacity, theme, accent)
}

pub(crate) fn desktop_widget_instance_overrides(
    conn: &Connection,
    instance_key: &str,
) -> (i32, i32, i32) {
    let prefix = format!("widget_instance_{instance_key}");
    let read = |suffix: &str, minimum: i32, maximum: i32| {
        db::get_setting(conn, &format!("{prefix}_{suffix}"), "-1")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .map(|value| {
                if value < 0 {
                    -1
                } else {
                    value.clamp(minimum.max(0), maximum)
                }
            })
            .unwrap_or(-1)
    };
    (
        read("opacity", -1, 100),
        read("theme", -1, 2),
        read("accent", -1, 7),
    )
}

pub(crate) fn apply_all_desktop_widget_styles(conn: &Connection, app_theme: i32, app_accent: i32) {
    let mut keys = vec![
        "calendar:1".to_string(),
        "events:1".to_string(),
        "countdown:1".to_string(),
        "clock:1".to_string(),
        "weather:1".to_string(),
        "focus:1".to_string(),
        "todo:1".to_string(),
    ];
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            keys.extend(
                windows
                    .notes
                    .borrow()
                    .iter()
                    .map(|note| format!("note_{}", note.get_note_id())),
            );
        }
    });
    for key in keys {
        let (opacity, theme, accent) = desktop_widget_style(conn, &key);
        apply_desktop_widget_style(&key, opacity, theme, accent, app_theme, app_accent);
    }
}
