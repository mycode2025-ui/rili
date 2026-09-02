fn main() {
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/timehub.ico")
        .set("ProductName", "TimeHub Desktop")
        .set("FileDescription", "TimeHub Desktop Calendar")
        .compile()
        .expect("嵌入 TimeHub Windows 图标失败");

    // 主题明暗由 AppWindow 内 Palette.color-scheme 与设计 Token 同步控制。
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).unwrap();

    for path in [
        "ui/app.slint",
        "ui/design-system.slint",
        "ui/theme.slint",
        "ui/icons.slint",
        "ui/widgets.slint",
        "ui/calendar-view.slint",
        "ui/week-view.slint",
        "ui/day-view.slint",
        "ui/year-view.slint",
        "ui/tools-view.slint",
        "ui/search-view.slint",
        "ui/records-view.slint",
        "ui/shift-view.slint",
        "ui/settings-view.slint",
        "ui/todo-board.slint",
        "ui/side-panel.slint",
        "ui/widget.slint",
        "ui/desktop-widgets.slint",
        "ui/quick-panel.slint",
        "ui/taskbar-clock.slint",
        "ui/new-event-dialog.slint",
        "ui/date-time-picker.slint",
        "ui/state-gallery.slint",
        "ui/theme-comparison.slint",
        "ui/notes-view.slint",
        "ui/course-view.slint",
        "ui/today-view.slint",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=ui/icons");
    println!("cargo:rerun-if-changed=assets/timehub-logo.svg");
    println!("cargo:rerun-if-changed=assets/timehub.ico");
}
