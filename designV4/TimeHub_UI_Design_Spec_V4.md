# TimeHub Desktop · UI 设计规格 V4.0

> 版本：V4.0（2026-09-01） · 基线：V3.0（双主题挂件系统）
> 主题：**全卡片「新建」入口** —— 桌面挂件卡片内直接创建内容，无需跳转主界面
> 技术栈对齐：Rust + Slint（`ui/widget.slint` · `ui/desktop-widgets.slint` · `src/main.rs`）

---

## 1. V4 变更摘要

| # | 变更 | 范围 |
|---|---|---|
| 1 | 所有桌面卡片头部右侧新增「+」新建按钮（置顶/设置左侧），平时三级色低调呈现，悬停时强调色反白反馈 | 6 款挂件 × 亮/暗双形态 |
| 2 | 新增「新建面板」内联创建场景：6 张卡片各自的面板打开态（就绪 / 校验失败 / 空标题禁用 / 单字段输入中） | s8 新增 2 个场景（深/浅壁纸） |
| 3 | 新增「统一交互规范」条：入口统一 / 原地创建 / 键盘闭环 / 校验内联 / 实时同步 | s8 底部 |
| 4 | Slint 实现侧同步升级：`WidgetCreatePanel` 自动聚焦 + Enter/Esc 键盘流 + 操作提示行；`HeaderButton` 强调色悬停 | ui/widget.slint |
| 5 | 修复 V3 遗留渲染 bug：专注卡 `.task`/`.st` 类名与四象限/状态屏全局类冲突导致白色块；面板字段类 `.fld` 与新建日程对话框冲突导致文本居中 | mockup.html |

## 2. 统一新建交互契约（6 张卡片完全一致）

| 环节 | 行为 | Slint 实现 |
|---|---|---|
| 入口 | 卡片头部右侧固定「+」按钮（22×22，圆角 6px），icon 13px；默认 `widget-text-3`，悬停 `widget-accent-bg` 底 + `widget-accent` 图标 | `CardTitle.show-add` + `HeaderButton.accent-hover` |
| 打开 | 面板在原卡片内联展开（覆盖内容区，头部保留可拖拽），不跳转主界面、不弹独立窗口 | `if create-open: WidgetCreatePanel` |
| 聚焦 | 打开即自动聚焦标题输入框，光标就绪 | `init => { title-input.focus(); }`（面板为条件创建，init 每次打开执行） |
| 键盘 | 标题框 Enter → 有明细字段跳明细，无明细直接保存；明细框 Enter → 保存；Esc 任意位置取消 | `TextInput.accepted` + `FocusScope.key-pressed(Escape)` |
| 校验 | 标题为空 → 保存按钮禁用（opacity .45）；格式错误 → 面板内联红字提示且**不关闭**，修正可重试 | Rust 侧 `anyhow::bail!` → `set_create_status` |
| 保存 | 写库成功 → 关面板、清空草稿、主界面与全部挂件同帧刷新 | `db::create_*` → `refresh_all(ui, widget, state)` |

## 3. 各卡片新建字段（操作路径最短）

| 卡片 | 面板标题 | 字段 1 | 字段 2（明细） | 落库 |
|---|---|---|---|---|
| W1 月历 | 新建日程 · {选中日期} | 日程标题 | 时间（默认 09:00） | 所选日期日程 |
| W2 今日日程 | 新建今日日程 | 日程标题 | 时间 | 今日日程 |
| W3 倒数日 | 新建倒数日 | 倒数日名称 | 日期 | 倒数日事件 |
| W4 时钟 | 新建提醒 | 提醒内容 | 时间 | 今日提醒 |
| W5 专注计时 | 新建专注任务 | 任务名称 | 时长（默认 25 分钟） | 保存并开始 |
| W6 今日待办 | 新建今日待办 | 待办标题（**仅单字段**） | — | 今日待办 |

双入口特例：W6 待办除头部「+」外，列表底部「虚线框 添加待办」同样打开面板（用户习惯兼容）。

## 4. 面板视觉规格（WidgetCreatePanel）

| 属性 | 值 | Token |
|---|---|---|
| 面板定位 | `left/right 10px · top 42px · bottom 10px`（卡片内边距内） | — |
| 表面 | 亮色 `#FFFFFF` / 暗色 `#171B24` | `Theme.bg-surface` |
| 描边/圆角 | 1px `border-default` · 12px | `Theme.r-lg` |
| 投影 | blur 16 · offsetY 6 | `widget-shadow` |
| 面板标题 | 11px / 600 / text-1 | `font-11` |
| 输入框 | 高 26px · 圆角 8px · 底 `bg-surface-2` · 聚焦描边 `widget-accent` | `r-md` |
| 占位/标签 | 占位 10.5px text-3 · 明细标签 10.5px text-2（宽 72px） | `font-10-5` |
| 输入文本 | 11px text-1 · 选区 accent 反白 | `font-11` |
| 错误提示 | 10px `widget-danger`，单行省略 | `font-10` |
| 键盘提示 | 10px text-3 居中「Enter 保存 · Esc 取消」 | `font-10` |
| 按钮 | 高 24px 等宽双键：取消（ghost，`widget-row` 底）/ 保存（primary，`widget-accent` 底 600 字重）；禁用 opacity .45 | `r-md` |

字体阶梯（与挂件整体协调）：面板标题 11 / 输入 11 / 标签·占位 10.5 / 提示·错误 10 —— 全部沿用 `Theme.font-*` 刻度，随全局 `font-delta` 缩放。

## 5. 性能与实现说明（Slint + Rust）

- **声明式条件创建**：面板用 `if create-open:` 声明式挂载/销毁，关闭时零渲染开销；打开即新建元素，`init` 钩子完成自动聚焦，无命令式 DOM 操作。
- **明细字段常驻 + 高度折叠**：`detail-wrap` 用 `height: show-detail ? 26px : 0px` 而非条件创建，保证 `detail-input` 的 id 在组件作用域可引用（标题框 Enter → `detail-input.focus()` 可编译），同时避免反复创建销毁。
- **键盘冒泡**：Esc 由面板根 `FocusScope.key-pressed` 统一捕获（TextInput 不消费的按键向上冒泡），无需每个输入框单独绑定。
- **数据一致性**：保存回调在 Rust 侧同步写 SQLite，成功后 `refresh_all` 一次性刷新主窗口与 6 个挂件窗口属性，UI 由 Slint 绑定系统自动重绘，无双写、无脏状态。

## 6. 交付物

| 文件 | 说明 |
|---|---|
| `designV4/mockup.html` | V4.0 单文件高保真原型（10 屏，0 外部依赖） |
| `designV4/shots/s1–s10.png` + `*_dark.png` | 20 张终图（每屏亮/暗双主题，s8 为 1560×3050 全场景长图） |
| `designV4/TimeHub_UI_Design_Spec_V4.md` | 本文档 |
| `ui/widget.slint` | WidgetCreatePanel 键盘流/自动聚焦/提示行；HeaderButton accent-hover |
| `ui/desktop-widgets.slint` | 6 个独立挂件窗口均已接入统一新建面板（V3.5 完成，V4 验证） |

历史版本：V1 `design/` · V2 `designV2/` · V3 `designV3/`（存档未动）。
