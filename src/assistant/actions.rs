//! Closed action vocabulary. Model output is data, never executable code or SQL.
use crate::{
    api::{self, Employee},
    error::{ApiError, ApiResult},
    util,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use std::collections::BTreeSet;

type Tx<'a> = Transaction<'a, Postgres>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    CreateTask {
        title: String,
        #[serde(default)]
        description: String,
        assignee_id: Option<i64>,
        due: String,
        priority: String,
    },
    UpdateTask {
        id: i64,
        changes: TaskChanges,
    },
    DeleteTask {
        id: i64,
    },
    AddComment {
        id: i64,
        text: String,
    },
    SetReminder {
        id: i64,
        before: String,
        channel: String,
    },
    PlanShifts {
        shifts: Vec<ShiftSpec>,
    },
    DeleteShift {
        id: i64,
    },
    PublishSchedule {
        week_start: NaiveDate,
    },
    RejectSchedule {
        week_start: NaiveDate,
    },
    ReadNotifications {
        ids: Vec<i64>,
    },
    ReadAllNotifications {},
    UpdateSettings {
        changes: SettingsChanges,
    },
    CreateEmployee {
        name: String,
        position: String,
        department: String,
    },
    CreateMeeting {
        title: String,
        notes: String,
        department: String,
        date: String,
    },
    DeleteMeeting {
        id: i64,
    },
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear_assignee: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShiftSpec {
    pub employee_id: i64,
    pub date: NaiveDate,
    pub time: String,
    #[serde(default)]
    pub note: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autonomous: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_tasks: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_reminders: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<QuietHours>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    pub enabled: bool,
    pub from: String,
    pub to: String,
}

fn text(s: &str, max: usize, required: bool) -> ApiResult<()> {
    if s.chars().count() > max || (required && s.trim().is_empty()) {
        return Err(ApiError::bad_request(format!(
            "Текст должен содержать {}–{} символов",
            if required { 1 } else { 0 },
            max
        )));
    }
    Ok(())
}
fn due(s: &str) -> ApiResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| ApiError::bad_request("Нужны точные дата, время и часовой пояс срока"))
}
fn priority(s: &str) -> ApiResult<()> {
    if !["Обычный", "Высокий", "Срочный"].contains(&s) {
        return Err(ApiError::bad_request("Неизвестный приоритет"));
    }
    Ok(())
}
pub fn validate(a: &Action) -> ApiResult<()> {
    use Action::*;
    match a {
        CreateTask {
            title,
            description,
            assignee_id,
            due: d,
            priority: p,
        } => {
            text(title, 300, true)?;
            text(description, 10000, false)?;
            due(d)?;
            priority(p)?;
            if assignee_id.is_some_and(|id| id <= 0) {
                return Err(ApiError::bad_request("Неверный сотрудник"));
            }
        }
        UpdateTask { id, changes: c } => {
            if *id <= 0 || serde_json::to_value(c).unwrap() == json!({}) {
                return Err(ApiError::bad_request("Нет изменений задачи"));
            }
            if let Some(t) = &c.title {
                text(t, 300, true)?;
            }
            if let Some(t) = &c.description {
                text(t, 10000, false)?;
            }
            if let Some(d) = &c.due {
                due(d)?;
            }
            if let Some(p) = &c.priority {
                priority(p)?;
            }
            if c.assignee_id.is_some_and(|i| i <= 0)
                || (c.clear_assignee && c.assignee_id.is_some())
            {
                return Err(ApiError::bad_request("Неверный ответственный"));
            }
            if let Some(s) = &c.status {
                if !["planned", "in_progress", "review", "done"].contains(&s.as_str()) {
                    return Err(ApiError::bad_request("Неизвестный статус"));
                }
            }
        }
        AddComment { id, text: t } => {
            if *id <= 0 {
                return Err(ApiError::bad_request("Неверная задача"));
            }
            text(t, 5000, true)?;
        }
        SetReminder {
            id,
            before,
            channel,
        } => {
            if *id <= 0
                || !["15m", "1h", "1d", "0"].contains(&before.as_str())
                || !["Telegram и почта", "Только Telegram", "Только почта"]
                    .contains(&channel.as_str())
            {
                return Err(ApiError::bad_request("Некорректное напоминание"));
            }
        }
        PlanShifts { shifts } => {
            if shifts.is_empty() || shifts.len() > 140 {
                return Err(ApiError::bad_request(
                    "Предложение должно содержать от 1 до 140 смен",
                ));
            }
            let mut seen = BTreeSet::new();
            for s in shifts {
                if !(1900..=2200).contains(&s.date.year())
                    || s.employee_id <= 0
                    || !seen.insert((s.employee_id, s.date))
                    || util::parse_time_range(&s.time).is_none()
                {
                    return Err(ApiError::bad_request("Некорректная или повторная смена"));
                }
                text(&s.note, 255, false)?;
            }
            let min = shifts.iter().map(|s| s.date).min().unwrap();
            let max = shifts.iter().map(|s| s.date).max().unwrap();
            if (max - min).num_days() > 31 {
                return Err(ApiError::bad_request(
                    "График составляется не более чем на 32 дня за один запрос",
                ));
            }
        }
        PublishSchedule { week_start } | RejectSchedule { week_start } => {
            if !(1900..=2200).contains(&week_start.year())
                || week_start.weekday().num_days_from_monday() != 0
            {
                return Err(ApiError::bad_request(
                    "Начало недели должно быть понедельником",
                ));
            }
        }
        ReadNotifications { ids } => {
            if ids.is_empty() || ids.len() > 100 || ids.iter().any(|i| *i <= 0) {
                return Err(ApiError::bad_request("Неверные уведомления"));
            }
        }
        UpdateSettings { changes: c } => {
            if serde_json::to_value(c).unwrap() == json!({}) {
                return Err(ApiError::bad_request("Нет изменений настроек"));
            }
            if let Some(t) = &c.company {
                text(t, 200, true)?;
            }
            if let Some(t) = &c.timezone {
                if ![
                    "Калининград · UTC+2",
                    "Самара · UTC+4",
                    "Москва · UTC+3",
                    "Минск · UTC+3",
                    "Екатеринбург · UTC+5",
                    "Новосибирск · UTC+7",
                    "Владивосток · UTC+10",
                    "UTC · UTC+0",
                ]
                .contains(&t.as_str())
                {
                    return Err(ApiError::bad_request("Неизвестный часовой пояс"));
                }
            }
            if let Some(q) = &c.quiet_hours {
                for t in [&q.from, &q.to] {
                    if chrono::NaiveTime::parse_from_str(t, "%H:%M").is_err() {
                        return Err(ApiError::bad_request("Некорректные тихие часы"));
                    }
                }
            }
        }
        CreateEmployee {
            name,
            position,
            department,
        } => {
            text(name, 255, true)?;
            text(position, 255, false)?;
            text(department, 100, false)?;
        }
        CreateMeeting {
            title,
            notes,
            department,
            date,
        } => {
            text(title, 500, true)?;
            text(notes, 30000, true)?;
            text(department, 100, false)?;
            due(date)?;
        }
        DeleteTask { id } | DeleteShift { id } | DeleteMeeting { id } => {
            if *id <= 0 {
                return Err(ApiError::bad_request("Неверный идентификатор"));
            }
        }
        ReadAllNotifications {} => {}
    }
    Ok(())
}

impl Action {
    pub fn manager_only(&self) -> bool {
        matches!(
            self,
            Self::DeleteTask { .. }
                | Self::PlanShifts { .. }
                | Self::DeleteShift { .. }
                | Self::PublishSchedule { .. }
                | Self::RejectSchedule { .. }
                | Self::UpdateSettings { .. }
                | Self::CreateEmployee { .. }
                | Self::DeleteMeeting { .. }
        )
    }
    pub fn title(&self) -> &'static str {
        match self {
            Self::CreateTask { .. } => "Создать задачу",
            Self::UpdateTask { .. } => "Изменить задачу",
            Self::DeleteTask { .. } => "Удалить задачу",
            Self::AddComment { .. } => "Добавить комментарий",
            Self::SetReminder { .. } => "Настроить напоминание",
            Self::PlanShifts { .. } => "Сохранить смены без публикации",
            Self::DeleteShift { .. } => "Удалить смену",
            Self::PublishSchedule { .. } => "Опубликовать график",
            Self::RejectSchedule { .. } => "Отклонить предложение графика",
            Self::ReadNotifications { .. } | Self::ReadAllNotifications {} => {
                "Прочитать уведомления"
            }
            Self::UpdateSettings { .. } => "Изменить настройки",
            Self::CreateEmployee { .. } => "Добавить сотрудника",
            Self::CreateMeeting { .. } => "Создать встречу и обработать текст",
            Self::DeleteMeeting { .. } => "Удалить встречу",
        }
    }
    fn employees(&self) -> Vec<i64> {
        match self {
            Self::CreateTask { assignee_id, .. } => assignee_id.iter().copied().collect(),
            Self::UpdateTask { changes, .. } => changes.assignee_id.iter().copied().collect(),
            Self::PlanShifts { shifts } => shifts.iter().map(|s| s.employee_id).collect(),
            _ => vec![],
        }
    }
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
/// Ollama grammar mirrors the serde vocabulary; unsupported fields are rejected again by Rust.
pub fn schema() -> Value {
    let string = json!({"type":"string"});
    let integer = json!({"type":"integer"});
    let mut variants = Vec::new();
    for (kind, fields, required) in [
        (
            "create_task",
            json!({"title":string,"description":string,"assigneeId":{"type":["integer","null"]},"due":string,"priority":{"enum":["Обычный","Высокий","Срочный"]}}),
            vec!["title", "description", "assigneeId", "due", "priority"],
        ),
        (
            "update_task",
            json!({"id":integer,"changes":object(json!({"title":string,"description":string,"assigneeId":integer,"clearAssignee":{"type":"boolean"},"due":string,"status":{"enum":["planned","in_progress","review","done"]},"priority":{"enum":["Обычный","Высокий","Срочный"]}}),&[])}),
            vec!["id", "changes"],
        ),
        ("delete_task", json!({"id":integer}), vec!["id"]),
        (
            "add_comment",
            json!({"id":integer,"text":string}),
            vec!["id", "text"],
        ),
        (
            "set_reminder",
            json!({"id":integer,"before":{"enum":["15m","1h","1d","0"]},"channel":{"enum":["Telegram и почта","Только Telegram","Только почта"]}}),
            vec!["id", "before", "channel"],
        ),
        (
            "plan_shifts",
            json!({"shifts":{"type":"array","maxItems":140,"items":object(json!({"employeeId":integer,"date":string,"time":string,"note":string}),&["employeeId","date","time","note"])}}),
            vec!["shifts"],
        ),
        ("delete_shift", json!({"id":integer}), vec!["id"]),
        (
            "publish_schedule",
            json!({"weekStart":string}),
            vec!["weekStart"],
        ),
        (
            "reject_schedule",
            json!({"weekStart":string}),
            vec!["weekStart"],
        ),
        (
            "read_notifications",
            json!({"ids":{"type":"array","items":integer}}),
            vec!["ids"],
        ),
        ("read_all_notifications", json!({}), vec![]),
        (
            "update_settings",
            json!({"changes":object(json!({"company":string,"timezone":string,"autonomous":{"type":"boolean"},"createTasks":{"type":"boolean"},"sendReminders":{"type":"boolean"},"quietHours":object(json!({"enabled":{"type":"boolean"},"from":string,"to":string}),&["enabled","from","to"])}),&[])}),
            vec!["changes"],
        ),
        (
            "create_employee",
            json!({"name":string,"position":string,"department":string}),
            vec!["name", "position", "department"],
        ),
        (
            "create_meeting",
            json!({"title":string,"notes":string,"department":string,"date":string}),
            vec!["title", "notes", "department", "date"],
        ),
        ("delete_meeting", json!({"id":integer}), vec!["id"]),
    ] {
        let mut props = fields;
        props["type"] = json!({"const":kind});
        let mut required = required;
        required.push("type");
        variants.push(object(props, &required));
    }
    json!({"oneOf":variants})
}

async fn record(tx: &mut Tx<'_>, table: &str, id: i64) -> ApiResult<Value> {
    // table is a Rust constant from the closed match below, never model input.
    let value: Option<Value> =
        sqlx::query_scalar(&format!("SELECT to_jsonb(t) FROM {table} t WHERE id=$1"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    value.ok_or_else(|| ApiError::not_found("Запись не найдена"))
}
async fn array(tx: &mut Tx<'_>, sql: &str, ids: &[i64]) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(sql)
        .bind(ids)
        .fetch_all(&mut **tx)
        .await?;
    Ok(json!(rows))
}
/// Persist the exact resources reviewed, then compare them again while workspace writes are locked.
pub async fn snapshot(tx: &mut Tx<'_>, a: &Action, employee: i64) -> ApiResult<Value> {
    let ids = a.employees();
    let mut people = array(
        tx,
        "SELECT to_jsonb(e) FROM employees e WHERE id=ANY($1) ORDER BY id",
        &ids,
    )
    .await?;
    let found: BTreeSet<i64> = people
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_i64())
        .collect();
    if ids.iter().any(|id| !found.contains(id)) {
        return Err(ApiError::bad_request("Сотрудник не найден"));
    }
    let resource=match a {
        Action::UpdateTask{id,..}|Action::DeleteTask{id}|Action::AddComment{id,..}|Action::SetReminder{id,..}=>{
            let task=record(tx,"tasks",*id).await?;
            let reminders=array(tx,"SELECT to_jsonb(r) FROM reminders r WHERE task_id=ANY($1) ORDER BY id",&[*id]).await?;
            let mut resource=json!({"task":task,"reminders":reminders});
            if matches!(a,Action::DeleteTask{..}) {
                resource["comments"]=array(tx,"SELECT jsonb_build_object('id',id,'version',md5(to_jsonb(c)::text)) FROM task_comments c WHERE task_id=ANY($1) ORDER BY id",&[*id]).await?;
                resource["history"]=array(tx,"SELECT jsonb_build_object('id',id,'version',md5(to_jsonb(h)::text)) FROM task_history h WHERE task_id=ANY($1) ORDER BY id",&[*id]).await?;
            }
            resource
        }
        Action::DeleteShift{id}=>record(tx,"shifts",*id).await?,
        Action::PlanShifts{shifts}=>{
            let start=shifts.iter().map(|s|s.date).min().unwrap()-Duration::days(1);
            let end=shifts.iter().map(|s|s.date).max().unwrap()+Duration::days(1);
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM shifts s WHERE employee_id=ANY($1) AND shift_date BETWEEN $2 AND $3 ORDER BY id").bind(&ids).bind(start).bind(end).fetch_all(&mut **tx).await?;
            let weeks:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(w) FROM schedule_weeks w WHERE week_start BETWEEN $1 AND $2 ORDER BY week_start").bind(monday(start+Duration::days(1))).bind(monday(end-Duration::days(1))).fetch_all(&mut **tx).await?;
            json!({"shifts":rows,"weeks":weeks})
        }
        Action::PublishSchedule{week_start}|Action::RejectSchedule{week_start}=>{
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s) FROM shifts s WHERE shift_date BETWEEN $1 AND $2 ORDER BY id").bind(*week_start-Duration::days(1)).bind(*week_start+Duration::days(7)).fetch_all(&mut **tx).await?;
            let week:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(w) FROM schedule_weeks w WHERE week_start=$1").bind(week_start).fetch_optional(&mut **tx).await?;
            json!({"shifts":rows,"week":week})
        }
        Action::DeleteMeeting{id}=>record(tx,"meetings",*id).await?,
        Action::UpdateSettings{..}=>sqlx::query_scalar::<_,Value>("SELECT data FROM app_settings WHERE id=1").fetch_optional(&mut **tx).await?.unwrap_or(Value::Null),
        Action::ReadNotifications{ids}=>array(tx,"SELECT to_jsonb(n) FROM notifications n WHERE id=ANY($1) ORDER BY id",ids).await?,
        Action::ReadAllNotifications{}=>array(tx,"SELECT to_jsonb(n) FROM notifications n WHERE recipient_id=ANY($1) AND NOT is_read ORDER BY id",&[employee]).await?,
        Action::CreateEmployee{name,..}=>{
            let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM employees e WHERE LOWER(name)=LOWER($1) ORDER BY id").bind(name.trim()).fetch_all(&mut **tx).await?;
            if !rows.is_empty() {return Err(ApiError::conflict("Сотрудник с таким именем уже существует; уточните имя"));} json!([])
        }
        _=>Value::Null
    };
    if matches!(
        a,
        Action::PublishSchedule { .. } | Action::RejectSchedule { .. }
    ) {
        let people_ids: Vec<i64> = resource["shifts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|shift| shift["employee_id"].as_i64())
            .collect();
        people = array(
            tx,
            "SELECT to_jsonb(e) FROM employees e WHERE id=ANY($1) ORDER BY id",
            &people_ids,
        )
        .await?;
    }
    if let Action::ReadNotifications { ids } = a {
        if resource.as_array().unwrap().len() != ids.iter().collect::<BTreeSet<_>>().len()
            || resource
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["recipient_id"].as_i64() != Some(employee))
        {
            return Err(ApiError::not_found("Уведомление не найдено"));
        }
    }
    Ok(json!({"people":people,"resource":resource}))
}

fn field(label: &str, value: impl Into<String>) -> Value {
    json!({"label":label,"value":value.into()})
}
fn readable(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "Включено".into()
            } else {
                "Выключено".into()
            }
        }
        _ => v.to_string(),
    }
}
fn name(snapshot: &Value, id: i64) -> String {
    snapshot["people"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id)
        .map(|e| format!("{} (№{})", e["name"].as_str().unwrap_or(""), id))
        .unwrap_or_else(|| format!("№{}", id))
}
fn changes_details(value: Value) -> Vec<Value> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            field(
                match k.as_str() {
                    "title" => "Название",
                    "description" => "Описание",
                    "assigneeId" => "Ответственный",
                    "clearAssignee" => "Снять ответственного",
                    "due" => "Срок",
                    "status" => "Статус",
                    "priority" => "Приоритет",
                    "company" => "Компания",
                    "timezone" => "Часовой пояс",
                    "autonomous" => "Автономный режим",
                    "createTasks" => "Автосоздание задач из встреч",
                    "sendReminders" => "Напоминания",
                    "quietHours" => "Тихие часы",
                    _ => k,
                },
                match v.as_str() {
                    Some("planned") => "Запланировано".into(),
                    Some("in_progress") => "В работе".into(),
                    Some("review") => "На проверке".into(),
                    Some("done") => "Завершено".into(),
                    _ => readable(v),
                },
            )
        })
        .collect()
}
pub fn details(a: &Action, s: &Value) -> Vec<Value> {
    use Action::*;
    match a {
        CreateTask {
            title,
            description,
            assignee_id,
            due,
            priority,
        } => vec![
            field("Название", title),
            field("Описание", description),
            field(
                "Ответственный",
                assignee_id
                    .map(|id| name(s, id))
                    .unwrap_or_else(|| "Без ответственного".into()),
            ),
            field("Срок", due),
            field("Приоритет", priority),
        ],
        UpdateTask { id, changes } => {
            let mut c = serde_json::to_value(changes).unwrap();
            if let Some(id) = changes.assignee_id {
                c["assigneeId"] = json!(name(s, id));
            }
            let mut d = vec![field(
                "Задача",
                format!(
                    "№{} · {}",
                    id,
                    s["resource"]["task"]["title"].as_str().unwrap_or("")
                ),
            )];
            d.extend(changes_details(c));
            d
        }
        DeleteTask { id } => vec![
            field(
                "Удалить задачу",
                format!(
                    "№{} · {}",
                    id,
                    s["resource"]["task"]["title"].as_str().unwrap_or("")
                ),
            ),
            field("Последствие", "Комментарии и история задачи будут удалены"),
        ],
        AddComment { id, text } => vec![
            field(
                "Задача",
                format!(
                    "№{} · {}",
                    id,
                    s["resource"]["task"]["title"].as_str().unwrap_or("")
                ),
            ),
            field("Комментарий", text),
        ],
        SetReminder {
            id,
            before,
            channel,
        } => vec![
            field(
                "Задача",
                format!(
                    "№{} · {}",
                    id,
                    s["resource"]["task"]["title"].as_str().unwrap_or("")
                ),
            ),
            field(
                "Когда",
                match before.as_str() {
                    "15m" => "За 15 минут",
                    "1h" => "За час",
                    "1d" => "За сутки",
                    _ => "В момент срока",
                },
            ),
            field("Канал", channel),
            field(
                "Доставка",
                "Сейчас напоминания доставляются уведомлением в приложении",
            ),
        ],
        PlanShifts { shifts } => shifts
            .iter()
            .map(|shift| {
                let old = s["resource"]["shifts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| {
                        v["employee_id"] == shift.employee_id
                            && v["shift_date"] == json!(shift.date)
                    });
                let was = old
                    .map(|v| {
                        util::fmt_range(
                            v["start_min"].as_i64().unwrap() as i32,
                            v["end_min"].as_i64().unwrap() as i32,
                        )
                    })
                    .unwrap_or_else(|| "нет смены".into());
                field(
                    &format!("{} · {}", name(s, shift.employee_id), shift.date),
                    format!(
                        "{} → {}{}",
                        was,
                        shift.time,
                        if shift.note.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", shift.note)
                        }
                    ),
                )
            })
            .collect(),
        DeleteShift { id } => vec![field(
            "Смена",
            format!(
                "№{} · сотрудник №{} · {} · {}",
                id,
                s["resource"]["employee_id"],
                s["resource"]["shift_date"].as_str().unwrap_or(""),
                util::fmt_range(
                    s["resource"]["start_min"].as_i64().unwrap() as i32,
                    s["resource"]["end_min"].as_i64().unwrap() as i32
                )
            ),
        )],
        PublishSchedule { week_start } | RejectSchedule { week_start } => {
            let mut d = vec![field(
                "Неделя",
                format!("{} — {}", week_start, *week_start + Duration::days(6)),
            )];
            for shift in s["resource"]["shifts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| {
                    v["shift_date"].as_str().is_some_and(|t| {
                        t >= week_start.to_string().as_str()
                            && t <= (*week_start + Duration::days(6)).to_string().as_str()
                    })
                })
            {
                let rejecting = matches!(a, RejectSchedule { .. });
                if rejecting && shift["status"] != "ai" {
                    continue;
                }
                let current = util::fmt_range(
                    shift["start_min"].as_i64().unwrap() as i32,
                    shift["end_min"].as_i64().unwrap() as i32,
                );
                let value = if rejecting {
                    match (
                        shift["prev_start_min"].as_i64(),
                        shift["prev_end_min"].as_i64(),
                    ) {
                        (Some(start), Some(end)) => format!(
                            "{} → {}",
                            current,
                            util::fmt_range(start as i32, end as i32)
                        ),
                        _ => format!("{} → удалить смену", current),
                    }
                } else {
                    current
                };
                d.push(field(
                    &format!(
                        "{} · {}",
                        name(s, shift["employee_id"].as_i64().unwrap()),
                        shift["shift_date"].as_str().unwrap()
                    ),
                    value,
                ));
            }
            d
        }
        ReadNotifications { ids } => vec![field(
            "Уведомления",
            ids.iter()
                .map(|i| format!("№{i}"))
                .collect::<Vec<_>>()
                .join(", "),
        )],
        ReadAllNotifications {} => vec![field(
            "Действие",
            format!(
                "Отметить прочитанными {} текущих уведомлений",
                s["resource"].as_array().unwrap().len()
            ),
        )],
        UpdateSettings { changes } => changes_details(serde_json::to_value(changes).unwrap()),
        CreateEmployee {
            name,
            position,
            department,
        } => vec![
            field("Имя", name),
            field("Должность", position),
            field("Отдел", department),
            field("Роль", "Сотрудник"),
        ],
        CreateMeeting {
            title,
            notes,
            department,
            date,
        } => vec![
            field("Название", title),
            field("Дата", date),
            field("Отдел", department),
            field("Текст встречи", notes),
            field(
                "Обработка",
                "Создать протокол; без автоматического создания задач",
            ),
        ],
        DeleteMeeting { id } => vec![
            field(
                "Встреча",
                format!(
                    "№{} · {}",
                    id,
                    s["resource"]["title"].as_str().unwrap_or("")
                ),
            ),
            field(
                "Последствие",
                "Протокол и связанные материалы встречи будут удалены; задачи сохранятся",
            ),
        ],
    }
}

async fn history(tx: &mut Tx<'_>, id: i64, user: &Employee, text: &str) -> ApiResult<()> {
    sqlx::query("INSERT INTO task_history(task_id,text) VALUES($1,$2)")
        .bind(id)
        .bind(format!("{} · AI-ассистент · {}", text, user.name))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn notice(
    tx: &mut Tx<'_>,
    recipient: i64,
    title: &str,
    text: &str,
    category: &str,
    href: &str,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO notifications(recipient_id,title,text,category,action_label,href,from_ai) VALUES($1,$2,$3,$4,'Открыть',$5,TRUE)").bind(recipient).bind(title).bind(text).bind(category).bind(href).execute(&mut **tx).await?;
    Ok(())
}
fn result(id: i64, href: String, text: &str) -> Value {
    json!({"id":id,"href":href,"text":text})
}
fn monday(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}
async fn check_rest(
    tx: &mut Tx<'_>,
    ids: &[i64],
    start: NaiveDate,
    end: NaiveDate,
) -> ApiResult<()> {
    let rows:Vec<(i64,NaiveDate,i32,i32)>=sqlx::query_as("SELECT employee_id,shift_date,start_min,end_min FROM shifts WHERE employee_id=ANY($1) AND shift_date BETWEEN $2 AND $3 ORDER BY employee_id,shift_date").bind(ids).bind(start-Duration::days(1)).bind(end+Duration::days(1)).fetch_all(&mut **tx).await?;
    for pair in rows.windows(2) {
        let (e1, d1, s1, f1) = pair[0];
        let (e2, d2, s2, _) = pair[1];
        if e1 == e2 && d2 == d1 + Duration::days(1) && util::rest_minutes(s1, f1, s2) < 720 {
            return Err(ApiError::conflict(format!(
                "Сотрудник №{}: между сменами {} и {} должно быть не менее 12 часов отдыха",
                e1, d1, d2
            )));
        }
    }
    Ok(())
}

/// All business writes, history, notifications and proposal resolution share one transaction.
pub async fn execute(tx: &mut Tx<'_>, user: &Employee, a: &Action) -> ApiResult<Value> {
    validate(a)?;
    if a.manager_only() {
        api::require_manager(user)?;
    }
    use Action::*;
    match a {
        CreateTask {
            title,
            description,
            assignee_id,
            due: d,
            priority,
        } => {
            let id:i64=sqlx::query_scalar("INSERT INTO tasks(title,description,assignee_id,assignee,due_at,status,priority,source) VALUES($1,$2,$3,(SELECT name FROM employees WHERE id=$3),$4,'planned',$5,'AI-ассистент') RETURNING id").bind(title.trim()).bind(description.trim()).bind(assignee_id).bind(due(d)?).bind(priority).fetch_one(&mut **tx).await?;
            history(tx, id, user, "Задача создана после подтверждения").await?;
            if let Some(assignee) = assignee_id {
                if *assignee != user.id {
                    notice(
                        tx,
                        *assignee,
                        "Вам назначена задача",
                        title,
                        "Задачи",
                        &format!("task.html?id={id}"),
                    )
                    .await?;
                }
            }
            Ok(result(id, format!("task.html?id={id}"), "Задача создана"))
        }
        UpdateTask { id, changes: c } => {
            let old = record(tx, "tasks", *id).await?;
            let old_due = old["due_at"]
                .as_str()
                .ok_or_else(|| ApiError::internal("Не удалось прочитать срок"))?;
            let new_due = due(c.due.as_deref().unwrap_or(old_due))?;
            let new_assignee = if c.clear_assignee {
                None
            } else {
                c.assignee_id.or_else(|| old["assignee_id"].as_i64())
            };
            let status = c
                .status
                .as_deref()
                .unwrap_or(old["status"].as_str().unwrap_or("planned"));
            sqlx::query("UPDATE tasks SET title=$2,description=$3,assignee_id=$4,assignee=(SELECT name FROM employees WHERE id=$4),due_at=$5,status=$6,priority=$7,completed_at=CASE WHEN $6='done' THEN COALESCE(completed_at,NOW()) ELSE NULL END,updated_at=NOW() WHERE id=$1").bind(id).bind(c.title.as_deref().unwrap_or(old["title"].as_str().unwrap_or(""))).bind(c.description.as_deref().unwrap_or(old["description"].as_str().unwrap_or(""))).bind(new_assignee).bind(new_due).bind(status).bind(c.priority.as_deref().unwrap_or(old["priority"].as_str().unwrap_or("Обычный"))).execute(&mut **tx).await?;
            if c.due.is_some() {
                sqlx::query("UPDATE reminders SET remind_at=$2-(offset_minutes * INTERVAL '1 minute'), is_sent=FALSE,sent_at=NULL WHERE task_id=$1").bind(id).bind(new_due).execute(&mut **tx).await?;
            }
            if status == "done" {
                sqlx::query("UPDATE reminders SET is_sent=TRUE WHERE task_id=$1")
                    .bind(id)
                    .execute(&mut **tx)
                    .await?;
            }
            history(
                tx,
                *id,
                user,
                &format!(
                    "Изменения: {}",
                    details(a, &json!({"people":[],"resource":{"task":old}}))
                        .iter()
                        .map(|f| format!(
                            "{}: {}",
                            f["label"].as_str().unwrap(),
                            f["value"].as_str().unwrap()
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            )
            .await?;
            if new_assignee != old["assignee_id"].as_i64() {
                if let Some(assignee) = new_assignee {
                    if assignee != user.id {
                        notice(
                            tx,
                            assignee,
                            "Вам назначена задача",
                            c.title
                                .as_deref()
                                .unwrap_or(old["title"].as_str().unwrap_or("")),
                            "Задачи",
                            &format!("task.html?id={id}"),
                        )
                        .await?;
                    }
                }
            }
            if status == "done" && old["status"] != "done" {
                let managers: Vec<i64> = sqlx::query_scalar(
                    "SELECT id FROM employees WHERE role='Руководитель' AND id<>$1",
                )
                .bind(user.id)
                .fetch_all(&mut **tx)
                .await?;
                for manager in managers {
                    notice(
                        tx,
                        manager,
                        "Задача завершена",
                        c.title
                            .as_deref()
                            .unwrap_or(old["title"].as_str().unwrap_or("")),
                        "Задачи",
                        &format!("task.html?id={id}"),
                    )
                    .await?;
                }
            }
            Ok(result(*id, format!("task.html?id={id}"), "Задача изменена"))
        }
        DeleteTask { id } => {
            sqlx::query("DELETE FROM reminders WHERE task_id=$1")
                .bind(id)
                .execute(&mut **tx)
                .await?;
            sqlx::query("DELETE FROM tasks WHERE id=$1")
                .bind(id)
                .execute(&mut **tx)
                .await?;
            Ok(result(*id, "tasks.html".into(), "Задача удалена"))
        }
        AddComment { id, text } => {
            sqlx::query("INSERT INTO task_comments(task_id,author_id,author_name,is_ai,text) VALUES($1,$2,$3,TRUE,$4)").bind(id).bind(user.id).bind(&user.name).bind(text.trim()).execute(&mut **tx).await?;
            history(tx, *id, user, "Добавлен комментарий").await?;
            Ok(result(
                *id,
                format!("task.html?id={id}"),
                "Комментарий добавлен",
            ))
        }
        SetReminder {
            id,
            before,
            channel,
        } => {
            let task = record(tx, "tasks", *id).await?;
            if task["status"] == "done" {
                return Err(ApiError::bad_request(
                    "Задача завершена — напоминание не требуется",
                ));
            }
            let minutes: i32 = match before.as_str() {
                "15m" => 15,
                "1h" => 60,
                "1d" => 1440,
                _ => 0,
            };
            let when = due(task["due_at"].as_str().unwrap())? - Duration::minutes(minutes as i64);
            if when <= Utc::now() {
                return Err(ApiError::bad_request("Время напоминания уже прошло"));
            }
            sqlx::query("INSERT INTO reminders(task_id,text,remind_at,before_code,offset_minutes,channel,is_sent) VALUES($1,$2,$3,$4,$5,$6,FALSE) ON CONFLICT(task_id) WHERE task_id IS NOT NULL DO UPDATE SET text=EXCLUDED.text,remind_at=EXCLUDED.remind_at,before_code=EXCLUDED.before_code,offset_minutes=EXCLUDED.offset_minutes,channel=EXCLUDED.channel,is_sent=FALSE,sent_at=NULL").bind(id).bind(format!("Напоминание · {channel}")).bind(when).bind(before).bind(minutes).bind(channel).execute(&mut **tx).await?;
            history(tx, *id, user, "Напоминание настроено").await?;
            Ok(result(
                *id,
                format!("task.html?id={id}"),
                "Напоминание сохранено",
            ))
        }
        PlanShifts { shifts } => {
            for s in shifts {
                let status: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM shifts WHERE employee_id=$1 AND shift_date=$2",
                )
                .bind(s.employee_id)
                .bind(s.date)
                .fetch_optional(&mut **tx)
                .await?;
                if status.as_deref() == Some("ai") {
                    return Err(ApiError::conflict("Сначала подтвердите или отклоните существующее предложение в разделе графика"));
                }
                let (start, end) = util::parse_time_range(&s.time).unwrap();
                sqlx::query("INSERT INTO shifts(employee_id,shift_date,start_min,end_min,note,status) VALUES($1,$2,$3,$4,$5,'confirmed') ON CONFLICT(employee_id,shift_date) DO UPDATE SET start_min=EXCLUDED.start_min,end_min=EXCLUDED.end_min,note=EXCLUDED.note,status='confirmed',prev_start_min=NULL,prev_end_min=NULL,updated_at=NOW()").bind(s.employee_id).bind(s.date).bind(start).bind(end).bind(&s.note).execute(&mut **tx).await?;
            }
            let start = shifts.iter().map(|s| s.date).min().unwrap();
            let end = shifts.iter().map(|s| s.date).max().unwrap();
            check_rest(tx, &a.employees(), start, end).await?;
            let weeks: BTreeSet<NaiveDate> = shifts.iter().map(|s| monday(s.date)).collect();
            for week in weeks {
                sqlx::query("INSERT INTO schedule_weeks(week_start,published) VALUES($1,FALSE) ON CONFLICT(week_start) DO UPDATE SET published=FALSE,updated_at=NOW()").bind(week).execute(&mut **tx).await?;
            }
            Ok(
                json!({"text":format!("Сохранено смен: {}. График готов к публикации.",shifts.len()),"href":format!("schedule.html?weekStart={}",monday(start))}),
            )
        }
        DeleteShift { id } => {
            let s = record(tx, "shifts", *id).await?;
            let date: NaiveDate = serde_json::from_value(s["shift_date"].clone())
                .map_err(|_| ApiError::internal("Неверная дата смены"))?;
            sqlx::query("DELETE FROM shifts WHERE id=$1")
                .bind(id)
                .execute(&mut **tx)
                .await?;
            sqlx::query(
                "UPDATE schedule_weeks SET published=FALSE,updated_at=NOW() WHERE week_start=$1",
            )
            .bind(monday(date))
            .execute(&mut **tx)
            .await?;
            notice(
                tx,
                s["employee_id"].as_i64().unwrap(),
                "Смена отменена",
                &format!("Отменена смена {date}"),
                "График",
                "schedule.html",
            )
            .await?;
            Ok(result(*id, "schedule.html".into(), "Смена удалена"))
        }
        PublishSchedule { week_start } => {
            let end = *week_start + Duration::days(6);
            let employees: Vec<i64> = sqlx::query_scalar(
                "SELECT DISTINCT employee_id FROM shifts WHERE shift_date BETWEEN $1 AND $2",
            )
            .bind(week_start)
            .bind(end)
            .fetch_all(&mut **tx)
            .await?;
            if employees.is_empty() {
                return Err(ApiError::bad_request("Нельзя опубликовать пустой график"));
            }
            check_rest(tx, &employees, *week_start, end).await?;
            sqlx::query("UPDATE shifts SET status='confirmed',prev_start_min=NULL,prev_end_min=NULL,updated_at=NOW() WHERE shift_date BETWEEN $1 AND $2").bind(week_start).bind(end).execute(&mut **tx).await?;
            sqlx::query("INSERT INTO schedule_weeks(week_start,published,published_at) VALUES($1,TRUE,NOW()) ON CONFLICT(week_start) DO UPDATE SET published=TRUE,published_at=NOW(),proposal_text=NULL,updated_at=NOW()").bind(week_start).execute(&mut **tx).await?;
            for e in employees {
                if e != user.id {
                    notice(
                        tx,
                        e,
                        "Опубликован график",
                        &format!("График на неделю {week_start} — {end} утверждён"),
                        "График",
                        "schedule.html",
                    )
                    .await?;
                }
            }
            sqlx::query("UPDATE notifications SET is_read=TRUE WHERE kind='schedule_proposal' AND ref_key=$1").bind(week_start.to_string()).execute(&mut **tx).await?;
            Ok(
                json!({"text":"График опубликован, сотрудники уведомлены","href":format!("schedule.html?weekStart={week_start}")}),
            )
        }
        RejectSchedule { week_start } => {
            let end = *week_start + Duration::days(6);
            let employees:Vec<i64>=sqlx::query_scalar("SELECT DISTINCT employee_id FROM shifts WHERE status='ai' AND shift_date BETWEEN $1 AND $2").bind(week_start).bind(end).fetch_all(&mut **tx).await?;
            sqlx::query("UPDATE shifts SET start_min=prev_start_min,end_min=prev_end_min,status='confirmed',prev_start_min=NULL,prev_end_min=NULL,updated_at=NOW() WHERE status='ai' AND prev_start_min IS NOT NULL AND prev_end_min IS NOT NULL AND shift_date BETWEEN $1 AND $2").bind(week_start).bind(end).execute(&mut **tx).await?;
            sqlx::query("DELETE FROM shifts WHERE status='ai' AND shift_date BETWEEN $1 AND $2")
                .bind(week_start)
                .bind(end)
                .execute(&mut **tx)
                .await?;
            check_rest(tx, &employees, *week_start, end).await?;
            sqlx::query("UPDATE schedule_weeks SET proposal_text=NULL,why_text=NULL,why_source=NULL,why_meeting_id=NULL,updated_at=NOW() WHERE week_start=$1").bind(week_start).execute(&mut **tx).await?;
            sqlx::query("UPDATE notifications SET is_read=TRUE WHERE kind='schedule_proposal' AND ref_key=$1").bind(week_start.to_string()).execute(&mut **tx).await?;
            Ok(
                json!({"text":"Предложение графика отклонено","href":format!("schedule.html?weekStart={week_start}")}),
            )
        }
        ReadNotifications { ids } => {
            sqlx::query(
                "UPDATE notifications SET is_read=TRUE WHERE id=ANY($1) AND recipient_id=$2",
            )
            .bind(ids)
            .bind(user.id)
            .execute(&mut **tx)
            .await?;
            Ok(json!({"text":"Уведомления отмечены прочитанными","href":"notifications.html"}))
        }
        ReadAllNotifications {} => {
            sqlx::query(
                "UPDATE notifications SET is_read=TRUE WHERE recipient_id=$1 AND NOT is_read",
            )
            .bind(user.id)
            .execute(&mut **tx)
            .await?;
            Ok(json!({"text":"Уведомления отмечены прочитанными","href":"notifications.html"}))
        }
        UpdateSettings { changes } => {
            let mut settings = api::default_settings();
            if let Some(saved) =
                sqlx::query_scalar::<_, Value>("SELECT data FROM app_settings WHERE id=1")
                    .fetch_optional(&mut **tx)
                    .await?
            {
                api::merge_json(&mut settings, &saved);
            }
            api::merge_json(&mut settings, &serde_json::to_value(changes).unwrap());
            sqlx::query("INSERT INTO app_settings(id,data) VALUES(1,$1) ON CONFLICT(id) DO UPDATE SET data=EXCLUDED.data").bind(settings).execute(&mut **tx).await?;
            Ok(json!({"text":"Настройки сохранены","href":"settings.html"}))
        }
        CreateEmployee {
            name,
            position,
            department,
        } => {
            let id:i64=sqlx::query_scalar("INSERT INTO employees(name,position,department,role) VALUES($1,$2,$3,'Сотрудник') RETURNING id").bind(name.trim()).bind(position.trim()).bind(department.trim()).fetch_one(&mut **tx).await?;
            Ok(result(id, "settings.html".into(), "Сотрудник добавлен"))
        }
        CreateMeeting {
            title,
            notes,
            department,
            date,
        } => {
            let id:i64=sqlx::query_scalar("INSERT INTO meetings(title,raw_text,description,department,meeting_date,status,progress,actions) VALUES($1,$2,$2,$3,$4,'processing',5,'{}') RETURNING id").bind(title.trim()).bind(notes.trim()).bind(department.trim()).bind(due(date)?).fetch_one(&mut **tx).await?;
            Ok(result(
                id,
                format!("meeting.html?id={id}"),
                "Встреча создана, начата подготовка протокола",
            ))
        }
        DeleteMeeting { id } => {
            sqlx::query("DELETE FROM meetings WHERE id=$1")
                .bind(id)
                .execute(&mut **tx)
                .await?;
            Ok(result(*id, "meetings.html".into(), "Встреча удалена"))
        }
    }
}
