use super::actions::{schema, validate, Action};
use serde_json::json;

fn action(value: serde_json::Value) -> Action {
    serde_json::from_value(value).unwrap()
}

#[test]
fn model_cannot_select_arbitrary_actions_or_grant_roles() {
    assert!(
        serde_json::from_value::<Action>(json!({"type":"run_sql","sql":"DELETE FROM tasks"}))
            .is_err()
    );
    assert!(serde_json::from_value::<Action>(json!({"type":"create_employee","name":"Иван","position":"","department":"","role":"Руководитель"})).is_err());
}
#[test]
fn task_requires_real_due_and_nonempty_title() {
    for (title, due) in [("", "2026-12-01T10:00:00+03:00"), ("Заказ", "завтра")] {
        assert!(validate(&action(json!({"type":"create_task","title":title,"description":"","assigneeId":null,"due":due,"priority":"Обычный"}))).is_err());
    }
    assert!(validate(&action(json!({"type":"create_task","title":"Заказ","description":"","assigneeId":null,"due":"2026-12-01T10:00:00+03:00","priority":"Обычный"}))).is_ok());
}
#[test]
fn schedule_rejects_duplicate_employee_day_and_bad_time() {
    let shift = json!({"employeeId":1,"date":"2026-12-01","time":"09:00-18:00","note":""});
    assert!(validate(&action(
        json!({"type":"plan_shifts","shifts":[shift.clone(),shift]})
    ))
    .is_err());
    assert!(validate(&action(json!({"type":"plan_shifts","shifts":[{"employeeId":1,"date":"2026-12-01","time":"25:00-18:00","note":""}]}))).is_err());
}
#[test]
fn schedule_batch_is_bounded_and_settings_are_whitelisted() {
    assert!(validate(&action(json!({"type":"plan_shifts","shifts":[]}))).is_err());
    assert!(serde_json::from_value::<Action>(
        json!({"type":"update_settings","changes":{"telegram":{"connected":true}}})
    )
    .is_err());
    assert!(validate(&action(
        json!({"type":"update_settings","changes":{"company":"Новая компания"}})
    ))
    .is_ok());
    assert!(schema()["oneOf"].as_array().unwrap().len() >= 12);
}

#[test]
fn extreme_calendar_dates_are_rejected_before_neighbor_arithmetic() {
    let action = action(
        json!({"type":"plan_shifts","shifts":[{"employeeId":1,"date":"+262142-12-31","time":"09:00-18:00","note":""}]}),
    );
    assert!(validate(&action).is_err());
}

#[test]
fn rejection_previews_restoration_and_removal_exactly() {
    use super::actions::details;
    let a = action(json!({"type":"reject_schedule","weekStart":"2027-04-05"}));
    let s = json!({"people":[],"resource":{"shifts":[
        {"employee_id":1,"shift_date":"2027-04-05","start_min":480,"end_min":960,"prev_start_min":1200,"prev_end_min":480,"status":"ai"},
        {"employee_id":2,"shift_date":"2027-04-06","start_min":540,"end_min":1080,"prev_start_min":null,"prev_end_min":null,"status":"ai"}
    ]}});
    let shown = json!(details(&a, &s)).to_string();
    assert!(shown.contains("08:00–16:00 → 20:00–08:00"), "{shown}");
    assert!(shown.contains("09:00–18:00 → удалить смену"), "{shown}");
}
