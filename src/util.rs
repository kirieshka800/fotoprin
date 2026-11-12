//! Чистые функции без внешних зависимостей (только std): даты, время смен,
//! разбор сроков из русского текста, тихие часы. Покрыты тестами (`cargo test`).
#![allow(dead_code)]

pub const MONTHS_FULL: [&str; 12] = [
    "января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября",
    "октября", "ноября", "декабря",
];
pub const MONTHS_SHORT: [&str; 12] = [
    "янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек",
];
/// Индекс 0 = понедельник.
pub const WEEKDAYS_NOM: [&str; 7] = [
    "понедельник", "вторник", "среда", "четверг", "пятница", "суббота", "воскресенье",
];

pub fn plural<'a>(n: i64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let n100 = n.abs() % 100;
    let n10 = n100 % 10;
    if (11..20).contains(&n100) {
        many
    } else if (2..5).contains(&n10) {
        few
    } else if n10 == 1 {
        one
    } else {
        many
    }
}

// ---------------------------------------------------------------------------
// Гражданские даты (алгоритмы Говарда Хиннанта)
// ---------------------------------------------------------------------------
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = понедельник … 6 = воскресенье.
pub fn weekday_from_days(z: i64) -> u32 {
    (z + 3).rem_euclid(7) as u32
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

fn valid_date(y: i64, m: u32, d: u32) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)
}

// ---------------------------------------------------------------------------
// Время смен
// ---------------------------------------------------------------------------
fn parse_clock(part: &str) -> Option<(u32, u32)> {
    let mut it = part.split(':');
    let h = it.next()?;
    let m = it.next();
    if it.next().is_some() || h.is_empty() || h.len() > 2 || !h.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hh: u32 = h.parse().ok()?;
    let mm: u32 = match m {
        None => 0,
        Some(m) => {
            if m.len() != 2 || !m.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            m.parse().ok()?
        }
    };
    if mm > 59 {
        return None;
    }
    Some((hh, mm))
}

/// «14:00 — 22:00», «14:00-22:00», «14-22» → (начало, конец) в минутах.
/// Конец может быть 24:00 (=1440). Если конец <= начала, смена переходит на следующие сутки.
pub fn parse_time_range(text: &str) -> Option<(i32, i32)> {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let seps: Vec<(usize, char)> = clean
        .char_indices()
        .filter(|(_, c)| matches!(c, '-' | '–' | '—'))
        .collect();
    if seps.len() != 1 {
        return None;
    }
    let (idx, sep) = seps[0];
    let left = &clean[..idx];
    let right = &clean[idx + sep.len_utf8()..];
    let (sh, sm) = parse_clock(left)?;
    let (eh, em) = parse_clock(right)?;
    if sh > 23 || eh > 24 || (eh == 24 && em != 0) {
        return None;
    }
    let start = (sh * 60 + sm) as i32;
    let end = (eh * 60 + em) as i32;
    if start == end {
        return None;
    }
    Some((start, end))
}

pub fn fmt_minutes(m: i32) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn fmt_range(start: i32, end: i32) -> String {
    format!("{}–{}", fmt_minutes(start), fmt_minutes(end))
}

/// Конец смены в минутах от начала суток её начала (учитывает переход через полночь).
pub fn abs_end(start: i32, end: i32) -> i32 {
    if end <= start {
        end + 1440
    } else {
        end
    }
}

/// Отдых между сменой предыдущего дня и сменой следующего (в минутах).
pub fn rest_minutes(prev_start: i32, prev_end: i32, next_start: i32) -> i32 {
    1440 + next_start - abs_end(prev_start, prev_end)
}

// ---------------------------------------------------------------------------
// Часовой пояс и тихие часы
// ---------------------------------------------------------------------------
/// Достаёт смещение из строки вида «Москва · UTC+3» или «UTC-5:30» (в секундах).
pub fn parse_utc_offset(text: &str) -> Option<i32> {
    let pos = text.find("UTC")?;
    let rest = &text[pos + 3..];
    let mut chars = rest.chars();
    let sign = match chars.next()? {
        '+' => 1,
        '-' | '−' => -1,
        _ => return None,
    };
    let tail: String = chars.collect();
    let mut parts = tail.splitn(2, ':');
    let hours_str: String = parts
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let hours: i32 = hours_str.parse().ok()?;
    let minutes: i32 = parts
        .next()
        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if hours > 14 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

pub fn parse_hhmm(text: &str) -> Option<u32> {
    let (h, m) = parse_clock(text.trim())?;
    if h > 23 || !text.contains(':') {
        return None;
    }
    Some(h * 60 + m)
}

/// Попадает ли момент (минуты от начала суток) в тихие часы [from, to). Поддерживает переход через полночь.
pub fn in_quiet_hours(now: u32, from: u32, to: u32) -> bool {
    if from == to {
        false
    } else if from < to {
        now >= from && now < to
    } else {
        now >= from || now < to
    }
}

// ---------------------------------------------------------------------------
// Срок задачи из русского текста («до пятницы», «13 ноября 16:00», «завтра»)
// ---------------------------------------------------------------------------
fn month_from_word(word: &str) -> Option<u32> {
    const STEMS: [&str; 12] = [
        "январ", "феврал", "март", "апрел", "ма", "июн", "июл", "август", "сентябр", "октябр",
        "ноябр", "декабр",
    ];
    for (i, stem) in STEMS.iter().enumerate() {
        if i == 4 {
            if word == "мая" || word == "май" {
                return Some(5);
            }
        } else if word.starts_with(stem) {
            return Some(i as u32 + 1);
        }
    }
    None
}

fn weekday_from_word(word: &str) -> Option<u32> {
    const STEMS: [&str; 7] = [
        "понедельник", "вторник", "сред", "четверг", "пятниц", "суббот", "воскресень",
    ];
    for (i, stem) in STEMS.iter().enumerate() {
        if word.starts_with(stem) {
            return Some(i as u32);
        }
    }
    None
}

fn trim_token(t: &str) -> &str {
    t.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Возвращает (год, месяц, день, часы, минуты). Время по умолчанию — 18:00.
/// `today` — локальная дата, `today_wd` — её день недели (0 = понедельник).
pub fn parse_ru_deadline(
    text: &str,
    today: (i64, u32, u32),
    today_wd: u32,
) -> Option<(i64, u32, u32, u32, u32)> {
    let lower = text.to_lowercase();
    if lower.trim().is_empty() || lower.contains("уточн") {
        return None;
    }
    let today_days = days_from_civil(today.0, today.1, today.2);
    let tokens: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '«' | '»' | '"'))
        .filter(|t| !t.is_empty())
        .collect();

    let mut time: Option<(u32, u32)> = None;
    let mut date_days: Option<i64> = None;

    for (i, raw) in tokens.iter().enumerate() {
        let tok = raw.trim_matches(|c: char| !c.is_alphanumeric());
        if tok.is_empty() {
            continue;
        }
        // время 16:30
        if tok.contains(':') && time.is_none() {
            if let Some((h, m)) = parse_clock(tok) {
                if h <= 23 {
                    time = Some((h, m));
                }
            }
            continue;
        }
        if date_days.is_some() {
            continue;
        }
        // 2026-11-13
        if tok.len() == 10 && tok.as_bytes()[4] == b'-' && tok.as_bytes()[7] == b'-' {
            let p: Vec<&str> = tok.split('-').collect();
            if p.len() == 3 {
                if let (Ok(y), Ok(m), Ok(d)) = (p[0].parse::<i64>(), p[1].parse::<u32>(), p[2].parse::<u32>()) {
                    if valid_date(y, m, d) {
                        date_days = Some(days_from_civil(y, m, d));
                        continue;
                    }
                }
            }
        }
        // 13.11 или 13.11.2026
        if tok.contains('.') {
            let p: Vec<&str> = tok.split('.').filter(|s| !s.is_empty()).collect();
            if (p.len() == 2 || p.len() == 3) && p.iter().all(|s| s.chars().all(|c| c.is_ascii_digit())) {
                if let (Ok(d), Ok(m)) = (p[0].parse::<u32>(), p[1].parse::<u32>()) {
                    let explicit_year = if p.len() == 3 {
                        p[2].parse::<i64>().ok().map(|y| if y < 100 { 2000 + y } else { y })
                    } else {
                        None
                    };
                    let y = explicit_year.unwrap_or(today.0);
                    if valid_date(y, m, d) {
                        let mut days = days_from_civil(y, m, d);
                        if explicit_year.is_none() && days < today_days {
                            if valid_date(y + 1, m, d) {
                                days = days_from_civil(y + 1, m, d);
                            }
                        }
                        date_days = Some(days);
                        continue;
                    }
                }
            }
        }
        // «13 ноября»
        if tok.len() <= 2 && !tok.is_empty() && tok.chars().all(|c| c.is_ascii_digit()) {
            if let Some(next) = tokens.get(i + 1) {
                if let Some(m) = month_from_word(trim_token(next)) {
                    if let Ok(d) = tok.parse::<u32>() {
                        let mut y = today.0;
                        if valid_date(y, m, d) {
                            let mut days = days_from_civil(y, m, d);
                            if days < today_days {
                                y += 1;
                                if valid_date(y, m, d) {
                                    days = days_from_civil(y, m, d);
                                }
                            }
                            date_days = Some(days);
                            continue;
                        }
                    }
                }
            }
        }
        match tok {
            "сегодня" => {
                date_days = Some(today_days);
                continue;
            }
            "послезавтра" => {
                date_days = Some(today_days + 2);
                continue;
            }
            "завтра" => {
                date_days = Some(today_days + 1);
                continue;
            }
            "через" => {
                if let (Some(n), Some(unit)) = (tokens.get(i + 1), tokens.get(i + 2)) {
                    if let Ok(n) = trim_token(n).parse::<i64>() {
                        let unit = trim_token(unit);
                        if unit.starts_with("дн") || unit.starts_with("день") {
                            date_days = Some(today_days + n);
                            continue;
                        }
                        if unit.starts_with("недел") {
                            date_days = Some(today_days + 7 * n);
                            continue;
                        }
                    }
                }
            }
            "конца" | "конце" | "конец" => {
                if tokens.get(i + 1).map(|t| trim_token(t).starts_with("недел")).unwrap_or(false) {
                    let delta = (4 + 7 - today_wd as i64) % 7; // ближайшая пятница, сегодня — если пятница
                    date_days = Some(today_days + delta);
                    continue;
                }
            }
            _ => {}
        }
        if let Some(wd) = weekday_from_word(tok) {
            let mut delta = (wd as i64 - today_wd as i64 + 7) % 7;
            if delta == 0 {
                delta = 7;
            }
            date_days = Some(today_days + delta);
        }
    }

    let days = date_days?;
    let (y, m, d) = civil_from_days(days);
    let (h, mi) = time.unwrap_or((18, 0));
    Some((y, m, d, h, mi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip_and_weekday() {
        let z = days_from_civil(2026, 11, 12);
        assert_eq!(civil_from_days(z), (2026, 11, 12));
        assert_eq!(weekday_from_days(z), 3); // четверг
        assert_eq!(weekday_from_days(days_from_civil(1970, 1, 1)), 3);
        assert_eq!(weekday_from_days(days_from_civil(2026, 11, 9)), 0); // понедельник
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(days_in_month(2100, 2), 28);
    }

    #[test]
    fn time_ranges() {
        assert_eq!(parse_time_range("14:00 — 22:00"), Some((840, 1320)));
        assert_eq!(parse_time_range("14:00-22:00"), Some((840, 1320)));
        assert_eq!(parse_time_range("8-16"), Some((480, 960)));
        assert_eq!(parse_time_range("09:30–18:00"), Some((570, 1080)));
        assert_eq!(parse_time_range("22:00–06:00"), Some((1320, 360)));
        assert_eq!(parse_time_range("08:00–24:00"), Some((480, 1440)));
        assert_eq!(parse_time_range("08:00–24:30"), None);
        assert_eq!(parse_time_range("25:00–26:00"), None);
        assert_eq!(parse_time_range("10:00–10:00"), None);
        assert_eq!(parse_time_range("10:60–12:00"), None);
        assert_eq!(parse_time_range("abc"), None);
        assert_eq!(parse_time_range("10:00"), None);
        assert_eq!(parse_time_range("10-12-14"), None);
        assert_eq!(fmt_range(480, 960), "08:00–16:00");
        assert_eq!(fmt_range(480, 1440), "08:00–24:00");
    }

    #[test]
    fn rest() {
        // 14–22, затем 08:00 → 10 часов отдыха
        assert_eq!(rest_minutes(840, 1320, 480), 600);
        // 08–16, затем 08 → 16 часов
        assert_eq!(rest_minutes(480, 960, 480), 960);
        // ночная 22–06 (до 06 следующего дня), затем 08:00 → 2 часа
        assert_eq!(rest_minutes(1320, 360, 480), 120);
        // пересечение
        assert_eq!(rest_minutes(1320, 480, 480), 0);
        assert!(rest_minutes(1320, 540, 480) < 0);
    }

    #[test]
    fn offsets_and_quiet() {
        assert_eq!(parse_utc_offset("Москва · UTC+3"), Some(10800));
        assert_eq!(parse_utc_offset("UTC-5:30"), Some(-19800));
        assert_eq!(parse_utc_offset("Лондон"), None);
        assert_eq!(parse_utc_offset("UTC+99"), None);
        assert_eq!(parse_hhmm("20:00"), Some(1200));
        assert_eq!(parse_hhmm("9:05"), Some(545));
        assert_eq!(parse_hhmm("2000"), None);
        assert_eq!(parse_hhmm("24:00"), None);
        // 20:00–09:00
        assert!(in_quiet_hours(1300, 1200, 540));
        assert!(in_quiet_hours(100, 1200, 540));
        assert!(!in_quiet_hours(600, 1200, 540));
        assert!(!in_quiet_hours(1200 - 1, 1200, 540));
        assert!(in_quiet_hours(1200, 1200, 540));
        assert!(!in_quiet_hours(540, 1200, 540));
        assert!(!in_quiet_hours(500, 600, 600));
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "задача", "задачи", "задач"), "задача");
        assert_eq!(plural(3, "задача", "задачи", "задач"), "задачи");
        assert_eq!(plural(5, "задача", "задачи", "задач"), "задач");
        assert_eq!(plural(11, "задача", "задачи", "задач"), "задач");
        assert_eq!(plural(21, "задача", "задачи", "задач"), "задача");
    }

    // 12 ноября 2026 — четверг
    const TODAY: (i64, u32, u32) = (2026, 11, 12);
    const WD: u32 = 3;

    #[test]
    fn deadlines() {
        assert_eq!(parse_ru_deadline("требует уточнения", TODAY, WD), None);
        assert_eq!(parse_ru_deadline("", TODAY, WD), None);
        assert_eq!(parse_ru_deadline("сегодня 16:00", TODAY, WD), Some((2026, 11, 12, 16, 0)));
        assert_eq!(parse_ru_deadline("завтра", TODAY, WD), Some((2026, 11, 13, 18, 0)));
        assert_eq!(parse_ru_deadline("послезавтра до 10:30", TODAY, WD), Some((2026, 11, 14, 10, 30)));
        assert_eq!(parse_ru_deadline("до пятницы", TODAY, WD), Some((2026, 11, 13, 18, 0)));
        assert_eq!(parse_ru_deadline("в четверг", TODAY, WD), Some((2026, 11, 19, 18, 0)));
        assert_eq!(parse_ru_deadline("до понедельника", TODAY, WD), Some((2026, 11, 16, 18, 0)));
        assert_eq!(parse_ru_deadline("13 ноября", TODAY, WD), Some((2026, 11, 13, 18, 0)));
        assert_eq!(parse_ru_deadline("до 20 ноября 12:00", TODAY, WD), Some((2026, 11, 20, 12, 0)));
        assert_eq!(parse_ru_deadline("1 марта", TODAY, WD), Some((2027, 3, 1, 18, 0)));
        assert_eq!(parse_ru_deadline("5 мая", TODAY, WD), Some((2027, 5, 5, 18, 0)));
        assert_eq!(parse_ru_deadline("15.11", TODAY, WD), Some((2026, 11, 15, 18, 0)));
        assert_eq!(parse_ru_deadline("15.11.2026", TODAY, WD), Some((2026, 11, 15, 18, 0)));
        assert_eq!(parse_ru_deadline("2026-12-01", TODAY, WD), Some((2026, 12, 1, 18, 0)));
        assert_eq!(parse_ru_deadline("через 3 дня", TODAY, WD), Some((2026, 11, 15, 18, 0)));
        assert_eq!(parse_ru_deadline("через 2 недели", TODAY, WD), Some((2026, 11, 26, 18, 0)));
        assert_eq!(parse_ru_deadline("до конца недели", TODAY, WD), Some((2026, 11, 13, 18, 0)));
        assert_eq!(parse_ru_deadline("31.02", TODAY, WD), None);
        assert_eq!(parse_ru_deadline("как получится", TODAY, WD), None);
        assert_eq!(parse_ru_deadline("к среде, 09:00", TODAY, WD), Some((2026, 11, 18, 9, 0)));
    }
}
