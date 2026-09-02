# TimeHub V2 UI 验收报告

验收基准：`designV2/mockup.html` 与 `designV2/shots/s1-s9.png`。

## 1. 基准与画布

- 原型展示截图为 `1560 × 980`，其中包含黑色展示页外框和顶部原型导航。
- 产品主窗口的真实设计画布为 `1280 × 820`。
- s5 快速面板为 `460 × 640`，s8 桌面挂件画布为 `1280 × 700`，s9 状态组件画布为 `1280 × 280`。
- 验收截图直接渲染产品组件，不把展示页外框误实现到软件中。

## 2. Token 与图标覆盖

- 从原型提取 CSS 自定义变量 62 个。
- `ui/theme.slint` 集中定义亮/暗主题、41 个语义颜色、8 个日历类别色、20 档字号、5 档圆角、3 组阴影、7 档间距和产品固定度量。
- s8 桌面挂件和设置预览使用从原型值提升出的专用语义 Token，避免亮暗主题串色。
- 原型 SVG sprite 已等价拆分为 49 个 SVG 文件，通过 `ui/icons.slint` 统一引用。
- 未使用 Emoji、字体图标或容易显示为方框的装饰字符。

## 3. 九屏实现状态

| 屏幕 | 实现组件 | 亮色截图 | 暗色截图 | 状态 |
| --- | --- | --- | --- | --- |
| s1 月视图 | `AppWindow` / `MonthView` / `SidePanel` | `implementation-shots/s1-light-final.png` | `implementation-shots/s1-dark-final.png` | 通过 |
| s2 周视图 | `WeekView` | `implementation-shots/s2-light-final.png` | `implementation-shots/s2-dark-final.png` | 通过 |
| s3 今天 | `TodayView` | `implementation-shots/s3-light-final.png` | `implementation-shots/s3-dark-final.png` | 通过 |
| s4 待办四象限 | `TodoBoard` | `implementation-shots/s4-light-final.png` | `implementation-shots/s4-dark-final.png` | 通过 |
| s5 快速面板 | `QuickPanelWindow` | `implementation-shots/s5-light-final.png` | `implementation-shots/s5-dark-final.png` | 通过 |
| s6 新建日程 | `NewEventDialog` | `implementation-shots/s6-light-final.png` | `implementation-shots/s6-dark-final.png` | 通过 |
| s7 设置外观 | `SettingsView` | `implementation-shots/s7-light-final.png` | `implementation-shots/s7-dark-final.png` | 通过 |
| s8 桌面挂件 | `WidgetWindow` | `implementation-shots/s8-light-final.png` | `implementation-shots/s8-dark-final.png` | 通过，平台毛玻璃偏差见清单 |
| s9 状态一览 | `StateGalleryWindow` | `implementation-shots/s9-light-final.png` | `implementation-shots/s9-dark-final.png` | 通过 |

总览：`implementation-shots/contact-light.png`、`implementation-shots/contact-dark.png`。

## 4. 本轮重点修复

- 周视图按原型使用 64px 时间栏、40px/小时、62px 日期头和 12:00 初始视口。
- 当前时间红线和 `22:53` 标签按分钟位置计算；重叠事件分栏显示。
- 月视图日历列表使用 14×14 类别色勾选框及原型 SVG 勾号。
- 新建日程弹窗使用 Token 化 `TextInput`，消除暗色主题下系统 `LineEdit` 漏白。
- 设置页“浅色 / 深色 / 跟随系统”预览保持静态含义，不随当前主题错误反转。
- 桌面挂件使用专用深色卡片 Token，亮色应用主题下仍保持原型桌面挂件外观。
- 状态页按钮恢复内容宽度，不再横向拉伸。

## 5. 自动检查

| 检查项 | 结果 |
| --- | --- |
| `slint-viewer --check ui/app.slint` | 通过，无诊断 |
| `cargo check` | 通过 |
| `cargo fmt --check` | 通过 |
| `cargo test` | 通过，2/2 |
| `cargo build --release` | 通过；`target/release/rili.exe` 生成于 2026-09-01 09:52:08 |
| UI 硬编码色扫描（排除 `theme.slint`） | 0 项 |
| Emoji / 高风险符号扫描 | 0 项 |
| 亮/暗主题截图 | 18 张均已生成 |

## 6. 事件三色规则

事件块使用用户日历类别基色计算：

- 背景：类别色 15% 透明 tint；
- 边框：类别基色；
- 标题：类别色深化 25%。

该规则不依赖固定类别，可用于用户自定义日历颜色。

## 7. 已知平台偏差

仅保留需要 Windows/macOS 原生窗口能力的偏差，详见 `TimeHub_UI_Deviation_List.md`。这些偏差均已显式记录，没有静默降级。

## 8. 本机运行核验说明

- release 程序已启动，进程路径为 `target/release/rili.exe`，主窗口标题为“日历”。
- Windows.Graphics.Capture 在验收时因整机提交内存不足返回 `0x8007000E`；按自动化安全规则重试一次后停止，没有关闭用户正在运行的 MATLAB 等无关程序。
- 该限制不影响前述 Slint 离屏渲染的 18 张验收截图，也不影响已启动的应用窗口。
