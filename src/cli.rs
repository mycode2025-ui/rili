//! 命令行接口：`rili <group> <action> ...`，供脚本 / AI agent 自动化调用。
//! 不带子命令直接运行 `rili` 时进入桌面 GUI（见 `main.rs`）。

use crate::{
    almanac, date_calc, db, holidays, ics, integrations, lunar, natural, share, sync, weather,
};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;
use std::fs;

#[derive(Parser)]
#[command(name = "rili", about = "本地优先的日历 / 待办 / 便签工具")]
pub struct Cli {
    /// 由 Windows 登录启动项使用：静默运行到系统托盘
    #[arg(long, hide = true)]
    pub startup: bool,

    /// 以 JSON 格式输出，便于脚本/AI 解析
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// 日程管理
    Events {
        #[command(subcommand)]
        action: EventsAction,
    },
    /// 待办管理
    Todos {
        #[command(subcommand)]
        action: TodosAction,
    },
    /// 便签管理
    Notes {
        #[command(subcommand)]
        action: NotesAction,
    },
    /// 习惯打卡
    Habits {
        #[command(subcommand)]
        action: HabitsAction,
    },
    /// 应用设置（主题 / 一周起始日 / 是否显示周数）
    Settings {
        #[command(subcommand)]
        action: SettingsAction,
    },
    /// 分类日历管理（工作/个人/家庭等带颜色的日程分组）
    Calendars {
        #[command(subcommand)]
        action: CalendarsAction,
    },
    /// ICS（iCalendar）导入导出，用于跟 Outlook / Google 日历 / Apple 日历互通
    Ics {
        #[command(subcommand)]
        action: IcsAction,
    },
    /// 天气预报（基于 Open-Meteo 免费接口，无需 API Key）
    Weather {
        #[command(subcommand)]
        action: WeatherAction,
    },
    /// 日历计算：农历、节假日、调休、日期间隔、老黄历
    Calendar {
        #[command(subcommand)]
        action: CalendarAction,
    },
    /// 搜索日程、待办和便签
    Search {
        #[arg(long)]
        query: String,
    },
    /// 本地 JSON 备份（追加导入，不覆盖现有数据）
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// 排班助手（班次定义、周期生成和本地班表）
    Shifts {
        #[command(subcommand)]
        action: ShiftAction,
    },
    /// ICS URL 订阅（单向拉取到本地日历）
    Subscriptions {
        #[command(subcommand)]
        action: SubscriptionsAction,
    },
    /// 生成或导入渠道无关的日程分享包（JSON + ICS）
    Share {
        #[command(subcommand)]
        action: ShareAction,
    },
    /// 云同步配置与协议状态（不内置特定云厂商）
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// 本地自然语言时间解析（只生成草稿，不直接写入数据库）
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },
}

#[derive(Subcommand)]
pub enum BackupAction {
    /// 导出日程、待办、便签、分类日历、习惯和打卡记录
    Export {
        #[arg(long)]
        path: String,
    },
    /// 导入 JSON 备份；数据会追加到当前数据库
    Import {
        #[arg(long)]
        path: String,
    },
}

#[derive(Subcommand)]
pub enum ShiftAction {
    /// 列出班次定义
    Types,
    /// 新建班次定义；休息班次可以不填起止时间
    CreateType {
        #[arg(long)]
        name: String,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long, default_value = "#2e6be6")]
        color: String,
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        rest: bool,
        #[arg(long, default_value_t = 0)]
        duration: i64,
    },
    /// 按班次名称生成周期班表，例如 "白班,白班,夜班,夜班,休息,休息"
    Generate {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        sequence: String,
    },
    /// 列出日期范围内的班表
    List {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// 清除一条已生成的班次
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum SubscriptionsAction {
    /// 添加一个 ICS URL 订阅源
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        url: String,
        #[arg(long, default_value_t = 1)]
        calendar_id: i64,
    },
    /// 列出所有订阅及最近同步状态
    List,
    /// 同步指定订阅；不传 id 则同步所有启用订阅
    Sync {
        #[arg(long)]
        id: Option<i64>,
    },
    /// 删除订阅及其导入的日程
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum ShareAction {
    /// 导出日期范围内的基准日程为分享包
    Export {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        path: String,
        #[arg(long, default_value = "rili 日程分享")]
        name: String,
    },
    /// 导入分享包中的 ICS 日程
    Import {
        #[arg(long)]
        path: String,
        #[arg(long, default_value_t = 1)]
        calendar_id: i64,
    },
}

#[derive(Subcommand)]
pub enum SyncAction {
    /// 查看本地同步配置和接入状态
    Status,
    /// 配置自建同步服务地址；认证凭据不写入普通备份
    Configure {
        #[arg(long)]
        endpoint: String,
        #[arg(long, default_value = "")]
        account: String,
    },
    /// 清除同步地址和账号
    Clear,
    /// 查看 rili-sync/v1 协议约定
    Protocol,
    /// 显式上传本地 JSON 快照；认证 token 从 RILI_SYNC_TOKEN 读取
    Push,
    /// 显式追加远端快照；必须传 --append，避免误操作
    Pull {
        #[arg(long)]
        append: bool,
    },
}

#[derive(Subcommand)]
pub enum AiAction {
    /// 解析一句中文日程描述
    Parse {
        #[arg(long)]
        text: String,
    },
}

#[derive(Subcommand)]
pub enum EventsAction {
    /// 按日期范围列出日程（基准记录，不展开重复规则）
    List {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// 按日期范围列出日程的每一次具体发生（重复日程会展开成多条）
    Occurrences {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// 获取单条日程
    Get {
        #[arg(long)]
        id: i64,
    },
    /// 新建日程
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        date: String,
        #[arg(long)]
        time: Option<String>,
        #[arg(long, default_value = "")]
        note: String,
        /// 重复规则：none/daily/weekly/weekly:1,3,5（每周一三五）/monthly/monthly-nth:3:4（每月第3个周五）/yearly
        #[arg(long, default_value = "none")]
        repeat: String,
        /// 提前提醒（分钟），逗号分隔，如 "0,30,1440"；留空表示不提醒
        #[arg(long)]
        reminder: Option<String>,
        /// 分类：event（普通日程）/birthday（生日）/anniversary（纪念日）/countdown（倒数日）
        #[arg(long, default_value = "event")]
        category: String,
        /// 挂到哪个分类日历（工作/个人/家庭…），不传则用默认日历（id=1）
        #[arg(long, default_value_t = 1)]
        calendar_id: i64,
    },
    /// 更新日程（只传要修改的字段）
    Update {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        time: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        repeat: Option<String>,
        #[arg(long)]
        reminder: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        calendar_id: Option<i64>,
        #[arg(long)]
        duration_minutes: Option<i64>,
    },
    /// 删除日程
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum TodosAction {
    /// 列出待办（可按截止日期/完成状态过滤）
    List {
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        done: Option<bool>,
    },
    /// 列出全部待办（供看板/四象限视图使用，不按日期过滤）
    ListAll,
    Get {
        #[arg(long)]
        id: i64,
    },
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        due: Option<String>,
        #[arg(long, default_value_t = 0)]
        priority: i64,
    },
    /// 切换完成/未完成状态
    Toggle {
        #[arg(long)]
        id: i64,
    },
    /// 设置"重要"标记（四象限视图用）
    SetImportant {
        #[arg(long)]
        id: i64,
        #[arg(long, action = clap::ArgAction::Set)]
        important: bool,
    },
    /// 设置看板列状态：todo（待办）/doing（进行中）/done（已完成）
    SetStatus {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        status: String,
    },
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum NotesAction {
    List,
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        content: String,
    },
    Update {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        content: String,
    },
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum HabitsAction {
    /// 列出习惯（含连续打卡天数、今天是否已打卡）
    List {
        #[arg(long, default_value_t = false)]
        include_archived: bool,
    },
    Create {
        #[arg(long)]
        title: String,
    },
    /// 打卡/取消打卡某一天（默认今天，再次调用即取消）
    Toggle {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        date: Option<String>,
    },
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum SettingsAction {
    /// 读取一项设置；未设置过时返回 default
    Get {
        #[arg(long)]
        key: String,
        #[arg(long, default_value = "")]
        default: String,
    },
    Set {
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
    },
}

#[derive(Subcommand)]
pub enum CalendarsAction {
    /// 列出所有分类日历
    List,
    /// 新建分类日历
    Create {
        #[arg(long)]
        name: String,
        /// #RRGGBB 十六进制颜色
        #[arg(long)]
        color: String,
    },
    /// 显示/隐藏某个分类日历（隐藏后该分类下的日程不出现在任何视图）
    Toggle {
        #[arg(long)]
        id: i64,
        #[arg(long, action = clap::ArgAction::Set)]
        visible: bool,
    },
    /// 删除分类日历（该分类下的日程会改挂到默认日历，不会被删除）
    Delete {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum IcsAction {
    /// 导出所有日程为 .ics 文件（可在 Outlook/Google/Apple 日历里"导入"或"订阅"该文件实现互通）
    Export {
        #[arg(long)]
        path: String,
        #[arg(long, default_value = "rili 日历")]
        name: String,
    },
    /// 从 .ics 文件导入日程（best-effort 解析 SUMMARY/DTSTART/RRULE，复杂重复规则会退化为不重复）
    Import {
        #[arg(long)]
        path: String,
        /// 导入的日程挂到哪个分类日历，默认挂到"默认"日历（id=1）
        #[arg(long, default_value_t = 1)]
        calendar_id: i64,
    },
}

#[derive(Subcommand)]
pub enum WeatherAction {
    /// 读取缓存的天气（不发网络请求；GUI 也是读这份缓存）
    Show,
    /// 设置天气所在城市（保存后由后台线程在 30 分钟内刷新一次；传空字符串可关闭天气功能）
    SetCity {
        #[arg(long)]
        city: String,
    },
    /// 立即强制刷新一次天气（会发起真实网络请求，需要能访问 open-meteo.com）
    Refresh,
}

#[derive(Subcommand)]
pub enum CalendarAction {
    /// 今天的公历/农历/节假日状态
    Today,
    /// 指定公历日期对应的农历
    Lunar {
        #[arg(long)]
        date: String,
    },
    /// 指定区间内的法定节假日/调休安排
    Holidays {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// 指定日期是否需要上班（工作日/周末/假期/调休）
    WorkState {
        #[arg(long)]
        date: String,
    },
    /// 两个日期之间的自然日间隔 + 工作日间隔
    Diff {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// 从某天起，往后/往前数 N 个工作日，返回落在哪一天
    Workdays {
        #[arg(long)]
        date: String,
        #[arg(long)]
        n: i64,
    },
    /// 从某天起，往后/往前数 N 个自然日，返回落在哪一天
    Shift {
        #[arg(long)]
        date: String,
        #[arg(long)]
        days: i64,
    },
    /// 老黄历摘要（年柱干支+生肖、日柱干支、农历日期）；不含宜忌/吉凶（见 almanac.rs 说明）
    Almanac {
        #[arg(long)]
        date: String,
    },
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")?)
}

#[derive(Serialize)]
struct Envelope<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn emit<T: Serialize + std::fmt::Debug>(json_mode: bool, result: Result<T>) {
    match result {
        Ok(data) => {
            if json_mode {
                let env = Envelope {
                    ok: true,
                    data: Some(&data),
                    error: None,
                };
                match serde_json::to_string_pretty(&env) {
                    Ok(output) => println!("{output}"),
                    Err(error) => eprintln!("序列化 JSON 输出失败: {error}"),
                }
            } else {
                println!("{data:#?}");
            }
        }
        Err(e) => {
            if json_mode {
                let env: Envelope<()> = Envelope {
                    ok: false,
                    data: None,
                    error: Some(e.to_string()),
                };
                match serde_json::to_string_pretty(&env) {
                    Ok(output) => println!("{output}"),
                    Err(error) => eprintln!("序列化 JSON 错误信息失败: {error}"),
                }
            } else {
                eprintln!("错误: {e}");
            }
            std::process::exit(1);
        }
    }
}

pub fn run(cli: Cli) -> Result<()> {
    let command = cli
        .command
        .context("缺少命令；桌面模式不应进入 CLI 执行器")?;
    let mut conn = db::open()?;

    match command {
        Command::Events { action } => match action {
            EventsAction::List { start, end } => {
                let r = (|| -> Result<_> {
                    db::list_events(&conn, parse_date(&start)?, parse_date(&end)?)
                })();
                emit(cli.json, r);
            }
            EventsAction::Occurrences { start, end } => {
                let r = (|| -> Result<_> {
                    db::list_event_occurrences(&conn, parse_date(&start)?, parse_date(&end)?)
                })();
                emit(cli.json, r);
            }
            EventsAction::Get { id } => {
                let r = db::get_event(&conn, id);
                emit(cli.json, r);
            }
            EventsAction::Create {
                title,
                date,
                time,
                note,
                repeat,
                reminder,
                category,
                calendar_id,
            } => {
                let r = (|| -> Result<_> {
                    let reminder = reminder
                        .map(|value| value.trim().to_string())
                        .unwrap_or(db::default_event_reminder(&conn)?);
                    db::create_event(
                        &conn,
                        db::NewEvent {
                            title: &title,
                            date: parse_date(&date)?,
                            time: time.as_deref(),
                            note: &note,
                            repeat_rule: &repeat,
                            reminder_offsets: &reminder,
                            category: &category,
                            calendar_id,
                        },
                    )
                })();
                emit(cli.json, r);
            }
            EventsAction::Update {
                id,
                title,
                date,
                time,
                note,
                repeat,
                reminder,
                category,
                calendar_id,
                duration_minutes,
            } => {
                let r = (|| -> Result<_> {
                    let date = date.map(|d| parse_date(&d)).transpose()?;
                    db::update_event(
                        &conn,
                        id,
                        db::EventUpdate {
                            title: title.as_deref(),
                            date,
                            time: time.as_deref().map(Some),
                            note: note.as_deref(),
                            repeat_rule: repeat.as_deref(),
                            reminder_offsets: reminder.as_deref(),
                            category: category.as_deref(),
                            calendar_id,
                            duration_minutes,
                        },
                    )
                })();
                emit(cli.json, r);
            }
            EventsAction::Delete { id } => {
                let r = db::delete_event(&conn, id);
                emit(cli.json, r);
            }
        },
        Command::Todos { action } => match action {
            TodosAction::List { due, done } => {
                let r = (|| -> Result<_> {
                    let due = due.map(|d| parse_date(&d)).transpose()?;
                    db::list_todos(&conn, due, done)
                })();
                emit(cli.json, r);
            }
            TodosAction::Get { id } => emit(cli.json, db::get_todo(&conn, id)),
            TodosAction::ListAll => emit(cli.json, db::list_all_todos(&conn)),
            TodosAction::Create {
                title,
                due,
                priority,
            } => {
                let r = (|| -> Result<_> {
                    let due = due.map(|d| parse_date(&d)).transpose()?;
                    db::create_todo(&conn, &title, due, priority)
                })();
                emit(cli.json, r);
            }
            TodosAction::Toggle { id } => emit(cli.json, db::toggle_todo(&conn, id)),
            TodosAction::SetImportant { id, important } => {
                emit(cli.json, db::set_todo_important(&conn, id, important))
            }
            TodosAction::SetStatus { id, status } => {
                emit(cli.json, db::set_todo_status(&conn, id, &status))
            }
            TodosAction::Delete { id } => emit(cli.json, db::delete_todo(&conn, id)),
        },
        Command::Notes { action } => match action {
            NotesAction::List => emit(cli.json, db::list_notes(&conn)),
            NotesAction::Create { title, content } => {
                emit(cli.json, db::create_note(&conn, &title, &content))
            }
            NotesAction::Update { id, title, content } => {
                emit(cli.json, db::update_note(&conn, id, &title, &content))
            }
            NotesAction::Delete { id } => emit(cli.json, db::delete_note(&conn, id)),
        },
        Command::Search { query } => emit(cli.json, db::search(&conn, &query, 50)),
        Command::Backup { action } => match action {
            BackupAction::Export { path } => {
                let r = (|| -> Result<_> {
                    let backup = db::export_backup(&conn)?;
                    let text = serde_json::to_string_pretty(&backup)?;
                    fs::write(&path, text).with_context(|| format!("写入备份失败：{path}"))?;
                    Ok(json!({ "path": path, "format_version": backup.format_version }))
                })();
                emit(cli.json, r);
            }
            BackupAction::Import { path } => {
                let r = (|| -> Result<_> {
                    let text = fs::read_to_string(&path)
                        .with_context(|| format!("读取备份失败：{path}"))?;
                    let backup: db::LocalBackup =
                        serde_json::from_str(&text).context("解析 JSON 备份失败")?;
                    db::import_backup(&mut conn, &backup)
                })();
                emit(cli.json, r);
            }
        },
        Command::Shifts { action } => match action {
            ShiftAction::Types => emit(cli.json, db::list_shift_types(&conn)),
            ShiftAction::CreateType {
                name,
                start,
                end,
                color,
                rest,
                duration,
            } => emit(
                cli.json,
                db::create_shift_type(
                    &conn,
                    &name,
                    start.as_deref(),
                    end.as_deref(),
                    &color,
                    rest,
                    duration,
                ),
            ),
            ShiftAction::Generate {
                start,
                end,
                sequence,
            } => {
                let r = (|| -> Result<_> {
                    let start = parse_date(&start)?;
                    let end = parse_date(&end)?;
                    let types = db::list_shift_types(&conn)?;
                    let ids: Vec<i64> = sequence
                        .split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(|name| {
                            types
                                .iter()
                                .find(|shift| shift.name == name)
                                .map(|shift| shift.id)
                                .with_context(|| format!("找不到班次：{name}"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    db::generate_shift_assignments(&conn, start, end, &ids)
                })();
                emit(cli.json, r);
            }
            ShiftAction::List { start, end } => {
                let r = (|| -> Result<_> {
                    db::list_shift_assignments(&conn, parse_date(&start)?, parse_date(&end)?)
                })();
                emit(cli.json, r);
            }
            ShiftAction::Delete { id } => emit(cli.json, db::delete_shift_assignment(&conn, id)),
        },
        Command::Subscriptions { action } => match action {
            SubscriptionsAction::Add {
                name,
                url,
                calendar_id,
            } => {
                emit(
                    cli.json,
                    db::create_subscription(&conn, &name, &url, calendar_id),
                );
            }
            SubscriptionsAction::List => emit(cli.json, db::list_subscriptions(&conn)),
            SubscriptionsAction::Sync { id } => {
                let r = if let Some(id) = id {
                    match db::get_subscription(&conn, id)? {
                        Some(subscription) => {
                            integrations::sync_ics_subscription_with_result(&conn, &subscription)
                                .map(|report| {
                                    json!({
                                        "subscriptions": 1,
                                        "imported_events": report.imported_events,
                                        "removed_cancelled_events": report.removed_cancelled_events,
                                        "errors": []
                                    })
                                })
                        }
                        None => Err(anyhow::anyhow!("订阅不存在：{id}")),
                    }
                } else {
                    integrations::sync_all_ics(&conn).map(|report| json!(report))
                };
                emit(cli.json, r);
            }
            SubscriptionsAction::Delete { id } => {
                emit(cli.json, db::delete_subscription(&conn, id))
            }
        },
        Command::Share { action } => match action {
            ShareAction::Export {
                start,
                end,
                path,
                name,
            } => {
                let r = (|| -> Result<_> {
                    let bundle =
                        share::export_range(&conn, parse_date(&start)?, parse_date(&end)?, &name)?;
                    fs::write(&path, serde_json::to_string_pretty(&bundle)?)
                        .with_context(|| format!("写入分享包失败：{path}"))?;
                    Ok(
                        json!({ "path": path, "events": bundle.events.len(), "format_version": bundle.format_version }),
                    )
                })();
                emit(cli.json, r);
            }
            ShareAction::Import { path, calendar_id } => {
                let r = (|| -> Result<_> {
                    let text = fs::read_to_string(&path)
                        .with_context(|| format!("读取分享包失败：{path}"))?;
                    let bundle: share::ShareBundle =
                        serde_json::from_str(&text).context("解析分享包失败")?;
                    share::import_bundle(&conn, &bundle, calendar_id)
                })();
                emit(cli.json, r);
            }
        },
        Command::Sync { action } => match action {
            SyncAction::Status => emit(cli.json, Ok(sync::status(&conn))),
            SyncAction::Configure { endpoint, account } => {
                emit(cli.json, sync::configure(&conn, &endpoint, &account))
            }
            SyncAction::Clear => emit(cli.json, sync::clear(&conn)),
            SyncAction::Protocol => emit(cli.json, Ok(sync::protocol_description().to_string())),
            SyncAction::Push => emit(cli.json, sync::push(&conn)),
            SyncAction::Pull { append } => emit(cli.json, sync::pull(&mut conn, append)),
        },
        Command::Ai { action } => match action {
            AiAction::Parse { text } => emit(
                cli.json,
                natural::parse(&text, chrono::Local::now().date_naive()),
            ),
        },
        Command::Habits { action } => match action {
            HabitsAction::List { include_archived } => {
                emit(cli.json, db::list_habits(&conn, include_archived))
            }
            HabitsAction::Create { title } => emit(cli.json, db::create_habit(&conn, &title)),
            HabitsAction::Toggle { id, date } => {
                let r = (|| -> Result<_> {
                    let d = date
                        .map(|d| parse_date(&d))
                        .transpose()?
                        .unwrap_or_else(|| chrono::Local::now().date_naive());
                    db::toggle_habit_log(&conn, id, d)
                })();
                emit(cli.json, r);
            }
            HabitsAction::Delete { id } => emit(cli.json, db::delete_habit(&conn, id)),
        },
        Command::Settings { action } => match action {
            SettingsAction::Get { key, default } => {
                emit(cli.json, db::get_setting(&conn, &key, &default))
            }
            SettingsAction::Set { key, value } => {
                let r = db::set_setting(&conn, &key, &value)
                    .map(|_| json!({ "key": key, "value": value }));
                emit(cli.json, r);
            }
        },
        Command::Calendars { action } => match action {
            CalendarsAction::List => emit(cli.json, db::list_calendars(&conn)),
            CalendarsAction::Create { name, color } => {
                emit(cli.json, db::create_calendar(&conn, &name, &color))
            }
            CalendarsAction::Toggle { id, visible } => {
                let r = db::set_calendar_visible(&conn, id, visible)
                    .map(|_| json!({ "id": id, "visible": visible }));
                emit(cli.json, r);
            }
            CalendarsAction::Delete { id } => emit(cli.json, db::delete_calendar(&conn, id)),
        },
        Command::Ics { action } => match action {
            IcsAction::Export { path, name } => {
                let r = (|| -> Result<_> {
                    // 导出的是很宽的日期区间（前后各若干年），配合 RRULE 让导入方软件自己展开重复日程。
                    let far_start = NaiveDate::from_ymd_opt(1970, 1, 1).context("固定起始日期")?;
                    let far_end = NaiveDate::from_ymd_opt(2100, 1, 1).context("固定结束日期")?;
                    let events = db::list_events(&conn, far_start, far_end)?;
                    // 先把每条日程的重复规则解析出来存进一个和 events 等长、生命周期覆盖整个函数的 Vec，
                    // 避免在下面的 map 闭包里创建临时值又想借用它（不合法）。
                    let rules: Vec<crate::recurrence::RepeatRule> = events
                        .iter()
                        .map(|e| crate::recurrence::RepeatRule::parse(&e.repeat_rule))
                        .collect();
                    let export_events: Vec<ics::ExportEvent> = events
                        .iter()
                        .zip(rules.iter())
                        .map(|(e, rule)| ics::ExportEvent {
                            uid: format!("rili-event-{}@local", e.id),
                            title: &e.title,
                            date: NaiveDate::parse_from_str(&e.date, "%Y-%m-%d")
                                .unwrap_or(far_start),
                            time: e.time.as_deref(),
                            note: &e.note,
                            repeat_rule: rule,
                        })
                        .collect();
                    let ics_text = ics::export_ics(&name, &export_events);
                    std::fs::write(&path, ics_text)?;
                    Ok(json!({ "path": path, "count": export_events.len() }))
                })();
                emit(cli.json, r);
            }
            IcsAction::Import { path, calendar_id } => {
                let r = (|| -> Result<_> {
                    let imported = ics::parse_ics_file(std::path::Path::new(&path))?;
                    let default_reminder = db::default_event_reminder(&conn)?;
                    let mut created = 0;
                    for ev in &imported {
                        if ev.cancelled {
                            continue;
                        }
                        let repeat_rule = ev.repeat_rule.to_string();
                        db::create_event(
                            &conn,
                            db::NewEvent {
                                title: &ev.title,
                                date: ev.date,
                                time: ev.time.as_deref(),
                                note: &ev.note,
                                repeat_rule: &repeat_rule,
                                reminder_offsets: &default_reminder,
                                category: "event",
                                calendar_id,
                            },
                        )?;
                        created += 1;
                    }
                    Ok(json!({ "imported": created }))
                })();
                emit(cli.json, r);
            }
        },
        Command::Weather { action } => match action {
            WeatherAction::Show => {
                let r = weather::cached(&conn)
                    .context("暂无缓存的天气数据（后台线程还没刷新过，或城市未设置）");
                emit(cli.json, r);
            }
            WeatherAction::SetCity { city } => {
                let r =
                    db::set_setting(&conn, "weather_city", &city).map(|_| json!({ "city": city }));
                emit(cli.json, r);
            }
            WeatherAction::Refresh => {
                let r = weather::refresh_once(&conn);
                emit(cli.json, r);
            }
        },
        Command::Calendar { action } => match action {
            CalendarAction::Today => {
                let today = chrono::Local::now().date_naive();
                let data = json!({
                    "date": today.to_string(),
                    "lunar": lunar::full_text(today),
                    "work_state": format!("{:?}", holidays::work_state(today)),
                    "holiday_name": holidays::holiday_name(today),
                });
                emit(cli.json, Ok::<_, anyhow::Error>(data));
            }
            CalendarAction::Lunar { date } => {
                let r = (|| -> Result<_> {
                    let d = parse_date(&date)?;
                    Ok(json!({ "date": d.to_string(), "lunar": lunar::full_text(d) }))
                })();
                emit(cli.json, r);
            }
            CalendarAction::Holidays { start, end } => {
                let r = (|| -> Result<_> {
                    let start = parse_date(&start)?;
                    let end = parse_date(&end)?;
                    let mut out = Vec::new();
                    let mut d = start;
                    while d <= end {
                        if let Some(name) = holidays::holiday_name(d) {
                            out.push(json!({ "date": d.to_string(), "name": name }));
                        }
                        if holidays::is_makeup_workday(d) {
                            out.push(json!({ "date": d.to_string(), "name": "调休上班" }));
                        }
                        let Some(next) = d.succ_opt() else {
                            break;
                        };
                        d = next;
                    }
                    Ok(out)
                })();
                emit(cli.json, r);
            }
            CalendarAction::WorkState { date } => {
                let r = (|| -> Result<_> {
                    let d = parse_date(&date)?;
                    Ok(
                        json!({ "date": d.to_string(), "work_state": format!("{:?}", holidays::work_state(d)) }),
                    )
                })();
                emit(cli.json, r);
            }
            CalendarAction::Diff { start, end } => {
                let r = (|| -> Result<_> {
                    Ok(date_calc::diff(parse_date(&start)?, parse_date(&end)?))
                })();
                emit(cli.json, r);
            }
            CalendarAction::Workdays { date, n } => {
                let r = (|| -> Result<_> {
                    let d = parse_date(&date)?;
                    let result = date_calc::add_workdays(d, n)?;
                    Ok(json!({ "date": d.to_string(), "n": n, "result": result.to_string() }))
                })();
                emit(cli.json, r);
            }
            CalendarAction::Shift { date, days } => {
                let r = (|| -> Result<_> {
                    let d = parse_date(&date)?;
                    let result = date_calc::add_calendar_days(d, days)?;
                    Ok(json!({ "date": d.to_string(), "days": days, "result": result.to_string() }))
                })();
                emit(cli.json, r);
            }
            CalendarAction::Almanac { date } => {
                let r = (|| -> Result<_> {
                    let d = parse_date(&date)?;
                    let info = almanac::describe(d);
                    Ok(json!({
                        "date": info.solar_date,
                        "lunar_full_text": info.lunar_full_text,
                        "day_ganzhi": info.day_ganzhi,
                        "note": "不含宜忌/吉凶时辰：这类内容源自各家黄历的经验数据表，没有统一算法标准，本工具不采集也不编造这类数据"
                    }))
                })();
                emit(cli.json, r);
            }
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_flag_selects_hidden_gui_mode() {
        let cli = Cli::try_parse_from(["rili", "--startup"]).expect("startup flag should parse");
        assert!(cli.startup);
        assert!(cli.command.is_none());
    }
}
