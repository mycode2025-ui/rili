# rili（日历）

一个用 Rust + Slint 实现的本地优先桌面日历 / 待办 / 便签工具。这是**自主设计的同类产品**（配色、交互、代码均为原创），
用来验证"用 Rust + Slint 做一款类似『优效日历』这类桌面日历软件"的可行性，**不包含**、也不打算复刻任何第三方软件的
界面美术、图标、文案或品牌名称。

## 当前功能

- 月视图 + 周视图 + 日 / 三日 / 年视图，可一键切换；月视图标注今天 / 周末 / 法定节假日 / 调休上班日，
  可选显示 ISO 周数，支持"周一起始"或"周日起始"两种一周起始方式
- **分类日历**（类似"工作/个人/家庭"）：每个分类带独立颜色，月视图每个日期格直接显示当天日程的彩色标签预览
  （超出显示上限的以"还有 N 项"折叠），侧边栏可勾选显示/隐藏某个分类，隐藏后该分类的日程从月/周视图、
  当日详情、桌面挂件里同时消失；内置"默认/工作/个人/家庭"四个分类，可自行新建更多
- 每个日期格显示农历短文本（初一显示为月份名，如"正月"），选中日详情额外显示当日干支（老黄历日柱，
  算法可通过两个公开可查证的参考日期验证，见 [src/almanac.rs](src/almanac.rs)）
- **记录中心**：独立管理生日、纪念日、倒数日和习惯追踪；生日/纪念日自动按年重复，倒数日显示剩余天数，习惯支持今日打卡与连续天数
- **排班助手**：内置白班/夜班/休息/培训班次，按自定义日期范围和班次名称序列循环生成本地班表，同一天重复生成会更新而不是重复插入
- **全局搜索**：搜索日程标题/备注、待办标题、便签标题/内容以及课程名称/教师/地点，点击结果可回到对应页面
- 选中某天后，右侧面板管理当日日程、当日待办、便签内容、习惯打卡（增删/编辑/勾选完成）；桌面挂件也可快速查看今日待办和最近便签
- 日程支持分类：普通日程 / 生日 / 纪念日 / 倒数日，生日与纪念日默认按年重复，倒数日显示剩余天数
- 日程支持重复规则：不重复 / 每天 / 每周 / 每月 / 每年，以及复杂规则"每周固定几天"（如每周一三五）和
  "每月第 N 个星期几"（如每月第三个周五）；重复日程会在月/周视图和当日列表里按实际发生日期展开
- 多级提前提醒（如"准时 + 提前30分钟 + 提前1天"），到点后台线程检查并弹出 Windows 系统通知（Toast）；
  未打包的 Win32 应用发通知有已知限制，失败时只记日志、不影响其他功能
- 习惯打卡：记录每天打卡状态，自动计算连续打卡天数（streak）
- 日期计算器（CLI + GUI 工具页）：两个日期间的自然日/工作日间隔，从某天起推算 N 个自然日/工作日后是哪天
- 个性化设置：3 套原创配色主题（默认蓝/暖阳橙/森野绿）、一周起始日、是否显示周数，均持久化保存
- 系统托盘图标：右键菜单可"显示主窗口 / 打开-关闭桌面挂件 / 退出"
- 开机启动：可在“设置 → 隐私”中开启或关闭，使用当前用户 Windows 登录启动项，无需管理员权限
- 任务栏时钟入口：保留 Windows 原生时钟显示，单击任务栏时钟后在对应任务栏上沿打开固定快速面板
- 桌面挂件：无边框、置顶、支持深浅主题的小窗口，提供月历、日程、倒数日、时钟、天气（当前天气与五日预报）、专注、待办和便签卡片，可独立于主窗口开关
- 数据全部保存在本机 SQLite（`%LOCALAPPDATA%/rili/rili/data/rili.db`），不上传云端
- 命令行接口 `rili <group> <action> ... [--json]`，方便脚本或 AI agent 自动化调用
- 本地 JSON 备份/恢复：`backup export` 导出日程、待办、便签、课程表、分类日历、习惯和打卡记录；课程数据自带 `timehub.course/v1` 字段说明，详见 [课程表 JSON 兼容格式](docs/course-json-format.md)；`backup import` 以追加方式恢复
- 云同步协议客户端：`sync configure/status/protocol` 管理自建服务边界，显式 `sync push` 上传；`sync pull` 必须带 `--append`，认证 token 从 `RILI_SYNC_TOKEN` 读取且不进入普通备份
- ICS URL 订阅：可配置多个订阅源，按 UID 同步更新本地日程，网络请求带超时并记录最近错误
- 渠道无关分享包：`share export` 输出结构化 JSON + RFC 5545 ICS，可交给微信小程序/网页后台；本地不依赖微信 SDK
- 本地自然语言助手：`ai parse` 或工具页将中文时间描述解析成可确认草稿，不调用云端模型

## 尚未实现（需要外部账号/服务，或技术风险过高，特意搁置）

- 多端云同步账号体系、跨设备/微信分享日程（需要自建后端）
- RSS 资讯（需要接入第三方 API；天气已接入 Open-Meteo 并带超时和缓存）
- AI 文本转待办、截图/语音/划词识别日程（需要接入大模型 API）
- 月视图纵向无限滚动、跨设备无缝横向切换（当前提供月/周/日/三日/年视图和导航）
- 老黄历的"宜/忌""吉凶时辰"（这类内容源自各家黄历的经验数据表，没有统一算法标准，故意不采集/不编造）

节假日数据表（[src/holidays.rs](src/holidays.rs)）是国务院每年单独发文公布的，**无法算法推导**，目前只内置了 2026
年的公开安排，跨年后需要手工更新。

重复日程目前是"整条规则"级别的简化实现：编辑或删除会作用于整条重复序列，暂不支持"仅此一次"的单独例外。

## 构建 & 运行

```powershell
cargo build --release
.\target\release\rili.exe          # 启动 GUI
.\target\release\rili.exe calendar today --json   # CLI 模式
```

## 测试与发布

```powershell
cargo fmt --all -- --check
cargo check --all-targets -j 1
cargo clippy --all-targets -j 1 -- -D warnings
cargo test --lib -j 1
```

核心逻辑编译为独立库目标，GUI 二进制不生成庞大的测试链接目标，适合在内存有限的机器和 CI 中验证。
推送 `v*` 标签会通过 GitHub Actions 构建 Windows 版本，并把 `TimeHub.exe` 发布到 GitHub Releases；
二进制不再提交进源码仓库。

## 代码结构

- `src/main.rs`：只负责进程入口、窗口装配、初始状态和定时器生命周期。
- `src/controllers/`：按课程表、日程导航、时间轴、同步、设置、桌面卡片、快速面板等功能注册 UI 回调。
- `src/desktop.rs`：桌面卡片窗口管理；`src/desktop/taskbar.rs` 单独负责 Windows 任务栏时钟监听和快速面板定位。
- `src/presentation/`：日期导航、主题、数据库模型映射、统一刷新和实时状态更新，各自独立。
- `src/app_state.rs`：GUI 会话状态；SQLite 仍是持久化数据的唯一来源。
- `ui/app.slint`：主窗口壳层；页面视图继续拆分在 `ui/*-view.slint`，壳层复用组件位于 `ui/app-shell-components.slint`。

## CLI 示例

```powershell
rili events list --start 2026-08-01 --end 2026-08-31 --json
rili events occurrences --start 2026-08-01 --end 2026-08-31 --json   # 重复日程按发生日期展开
rili events create --title "周会" --date 2026-09-01 --time 09:30 --repeat weekly --reminder "0,30" --json
rili events create --title "健身" --date 2026-09-01 --repeat "weekly:0,2,4" --json          # 每周一三五
rili events create --title "读书会" --date 2026-09-18 --repeat "monthly-nth:3:4" --json      # 每月第3个周五
rili events create --title "妈妈生日" --date 1970-05-20 --category birthday --json
rili calendars list --json                                  # 列出分类日历（默认/工作/个人/家庭…）
rili calendars create --name "项目" --color "#20b0b0" --json
rili calendars toggle --id 2 --visible false                # 隐藏某个分类（从所有视图消失）
rili events create --title "站会" --date 2026-09-01 --calendar-id 2 --json   # 挂到指定分类日历
rili todos create --title "写周报" --due 2026-09-01
rili todos list --done false --json
rili habits create --title "每天喝水"
rili habits toggle --id 1                              # 打卡/取消今天的打卡
rili habits list --json
rili notes update --id 1 --title "会议记录" --content "补充结论和行动项"
rili search --query "周会" --json
rili backup export --path .\rili-backup.json --json
rili backup import --path .\rili-backup.json --json
rili subscriptions add --name "团队日历" --url "https://example.com/team.ics" --json
rili subscriptions sync --json
rili sync configure --endpoint "https://sync.example.com/api" --account "me" --json
rili sync push --json                         # 显式上传，token 通过 RILI_SYNC_TOKEN 提供
rili sync pull --append --json                # 显式确认后追加远端快照
rili share export --start 2026-09-01 --end 2026-09-30 --path .\share.json --json
rili ai parse --text "明天下午两点和供应商开会，提前一天提醒我" --json
rili shifts types --json
rili shifts generate --start 2026-09-01 --end 2026-09-30 --sequence "白班,白班,夜班,夜班,休息,休息" --json
rili shifts list --start 2026-09-01 --end 2026-09-30 --json
rili settings set --key theme --value 1
rili settings get --key theme --default 0
rili calendar holidays --start 2026-01-01 --end 2026-12-31 --json
rili calendar lunar --date 2026-09-25 --json
rili calendar almanac --date 2026-09-25 --json          # 老黄历：农历 + 日柱干支
rili calendar diff --start 2026-01-01 --end 2026-12-31 --json      # 自然日/工作日间隔
rili calendar workdays --date 2026-08-31 --n 10 --json  # 往后数 10 个工作日
rili calendar shift --date 2026-08-31 --days 10 --json  # 往后数 10 个自然日
```

## 目录结构

- `ui/` — Slint 界面（`app.slint` 主窗口，`widget.slint` 深色桌面挂件，`calendar-view.slint` 月视图，
  `week-view.slint` 周视图，`side-panel.slint` 侧边栏，`widgets.slint` 自绘按钮/下拉框/复选框组件，
  `design-system.slint` 配色/字号令牌）
- `src/db.rs` — SQLite 建表与增删改查（日程/待办/便签/习惯/分类日历/设置）
- `src/recurrence.rs` — 重复日程规则展开（含每周多天、每月第N个星期几）
- `src/reminders.rs` — 后台提醒扫描线程 + Windows Toast 通知
- `src/autostart.rs` — Windows 当前用户开机启动项的读取与切换
- `src/lunar.rs` — 农历换算（基于 `chinese-lunisolar-calendar`）
- `src/almanac.rs` — 老黄历日柱干支换算（含单元测试验证）
- `src/date_calc.rs` — 日期计算器（自然日/工作日间隔与推算）
- `ui/search-view.slint` — 日程、待办、便签全局搜索
- `ui/records-view.slint` — 生日/纪念日/倒数日/习惯追踪记录中心
- `ui/shift-view.slint` — 本地排班助手
- `src/db.rs` — 同时提供本地 JSON 备份/恢复数据结构与追加导入逻辑
- `src/integrations.rs` — ICS URL 订阅网络适配器与外部能力状态
- `src/sync.rs` — 自建云同步协议配置、显式 push/pull 客户端
- `src/share.rs` — 渠道无关的 JSON + ICS 日程分享包
- `src/natural.rs` — 离线中文自然语言时间草稿解析
- `src/holidays.rs` — 法定节假日 / 调休数据表
- `src/cli.rs` — 命令行子命令
- `src/main.rs` — GUI 入口，装配 Slint 窗口、系统托盘图标与回调
