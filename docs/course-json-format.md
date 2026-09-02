# TimeHub 课程表 JSON 兼容格式

TimeHub 的本地备份是 UTF-8 JSON。备份根对象中的 `courses` 保存课程，`course_schema` 保存同一份文件所使用的字段说明。第三方软件应优先检查：

```text
course_schema.schema_id == "timehub.course/v1"
```

未知的根字段和课程字段应忽略，以便兼容后续版本。导入 TimeHub 时，课程会追加到本地数据，不使用外部 `id` 覆盖现有记录。

## 最小课程对象

第三方软件生成课程时，建议至少提供以下字段：

```json
{
  "title": "高等数学",
  "teacher": "陈老师",
  "location": "A101",
  "weekday": 1,
  "start_period": 1,
  "period_count": 2,
  "start_week": 1,
  "end_week": 16,
  "color_index": 0,
  "created_at": "2026-09-02 09:00:00",
  "updated_at": "2026-09-02 09:00:00"
}
```

## 字段规则

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `id` | integer | TimeHub 本地主键。跨软件交换时可忽略，导入方可重新生成。 |
| `title` | string | 必填，课程名称。 |
| `teacher` | string | 教师姓名；未知时使用空字符串。 |
| `location` | string | 教室或地点；未知时使用空字符串。 |
| `weekday` | integer | `1` 周一，`2` 周二，…，`7` 周日。 |
| `start_period` | integer | 开始节次，范围 `1..12`。 |
| `period_count` | integer | 连续节数，范围 `1..6`；结束节次为 `start_period + period_count - 1`。 |
| `start_week` | integer | 开始周，范围 `1..30`，包含端点。 |
| `end_week` | integer | 结束周，范围 `1..30`，包含端点，不能小于 `start_week`。 |
| `color_index` | integer | `0..7`，对应 `course_schema.color_palette`。 |
| `created_at` | string | `YYYY-MM-DD HH:MM:SS`，没有来源时间时可填导出时间。 |
| `updated_at` | string | `YYYY-MM-DD HH:MM:SS`。 |

同一星期的两门课程，如果周次区间和节次区间都相交，TimeHub 会判定为冲突。

## 完整备份片段

```json
{
  "format_version": 1,
  "course_schema": {
    "schema_id": "timehub.course/v1",
    "version": 1,
    "encoding": "UTF-8",
    "description": "TimeHub 周课程表。星期采用 ISO 顺序，节次和周次均从 1 开始；区间端点均包含。",
    "weekday_values": [
      "1=Monday/周一",
      "2=Tuesday/周二",
      "3=Wednesday/周三",
      "4=Thursday/周四",
      "5=Friday/周五",
      "6=Saturday/周六",
      "7=Sunday/周日"
    ],
    "period_range": [1, 12],
    "week_range": [1, 30],
    "color_palette": [
      "#2e6be6",
      "#0e9f6e",
      "#c77700",
      "#d93a49",
      "#7c4dff",
      "#0891b2",
      "#db2777",
      "#65a30d"
    ],
    "fields": []
  },
  "courses": [
    {
      "id": 1,
      "title": "高等数学",
      "teacher": "陈老师",
      "location": "A101",
      "weekday": 1,
      "start_period": 1,
      "period_count": 2,
      "start_week": 1,
      "end_week": 16,
      "color_index": 0,
      "created_at": "2026-09-02 09:00:00",
      "updated_at": "2026-09-02 09:00:00"
    }
  ]
}
```

实际导出的 `course_schema.fields` 会包含每个字段的 `name`、`json_type`、`required` 和 `description`；上例省略该数组只是为了缩短示例。

## 导出与导入

```powershell
rili backup export --path .\rili-backup.json --json
rili backup import --path .\rili-backup.json --json
```

旧版备份没有 `course_schema` 或 `courses` 时仍可导入，两项会按空值处理。
