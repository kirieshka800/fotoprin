//! Speech runtime integration; the application and orchestration remain in Rust.
use serde_json::Value;
use std::time::Duration;
#[cfg(test)]
mod tests;

pub struct Segment {
    pub start_seconds: f64,
    pub text: String,
}
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
    pub duration_seconds: i32,
}

pub fn endpoint(base: &str, provider: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    if base.ends_with("/v1/audio/transcriptions") || base.ends_with("/inference") {
        base.into()
    } else if provider == "whisper_cpp" {
        format!("{base}/inference")
    } else if base.ends_with("/v1") {
        format!("{base}/audio/transcriptions")
    } else {
        format!("{base}/v1/audio/transcriptions")
    }
}
fn timestamp(value: &Value) -> Option<f64> {
    let parts: Vec<f64> = value
        .as_str()?
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.len() == 3 {
        Some(parts[0] * 3600.0 + parts[1] * 60.0 + parts[2])
    } else {
        None
    }
}
pub fn parse_response(data: Value) -> Result<Transcript, String> {
    if let Some(error) = data.get("error") {
        return Err(format!("Сервис распознавания сообщил ошибку: {}", error));
    }
    let mut segments = Vec::new();
    let mut last_end = 0.0_f64;
    if let Some(list) = data["segments"]
        .as_array()
        .or_else(|| data["transcription"].as_array())
    {
        for item in list {
            let text = item["text"].as_str().unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }
            let start = item["start"]
                .as_f64()
                .or_else(|| item["offsets"]["from"].as_f64().map(|v| v / 1000.0))
                .or_else(|| timestamp(&item["timestamps"]["from"]))
                .unwrap_or(0.0);
            let end = item["end"]
                .as_f64()
                .or_else(|| item["offsets"]["to"].as_f64().map(|v| v / 1000.0))
                .or_else(|| timestamp(&item["timestamps"]["to"]))
                .unwrap_or(start);
            if !start.is_finite() || !end.is_finite() || start < 0.0 || end < start {
                return Err("Сервис распознавания вернул некорректные таймкоды".into());
            }
            last_end = last_end.max(end);
            segments.push(Segment {
                start_seconds: start,
                text: text.into(),
            });
        }
    }
    let full = data["text"].as_str().unwrap_or("").trim();
    if segments.is_empty() && !full.is_empty() {
        segments.push(Segment {
            start_seconds: 0.0,
            text: full.into(),
        });
    }
    if segments.is_empty() {
        return Err("В записи не удалось распознать речь".into());
    }
    let text = if full.is_empty() {
        segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        full.into()
    };
    let duration = data["duration"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(last_end);
    Ok(Transcript {
        text,
        segments,
        duration_seconds: duration.round().clamp(0.0, i32::MAX as f64) as i32,
    })
}
fn mime(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" => "audio/mp4",
        "webm" => "audio/webm",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}
pub async fn transcribe(file_name: &str, bytes: Vec<u8>) -> Result<Transcript, String> {
    let base = std::env::var("WHISPER_URL").unwrap_or_default();
    if base.trim().is_empty() {
        return Err("Распознавание аудио не настроено. Запустите Whisper и задайте WHISPER_URL в .env (см. AUDIO_SETUP.md), затем нажмите «Распознать аудио».".into());
    }
    let provider = std::env::var("WHISPER_PROVIDER").unwrap_or_else(|_| "openai".into());
    if !["openai", "whisper_cpp"].contains(&provider.as_str()) {
        return Err("WHISPER_PROVIDER должен быть openai или whisper_cpp".into());
    }
    let file = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name.to_owned())
        .mime_str(mime(file_name))
        .map_err(|e| e.to_string())?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", file)
        .text(
            "language",
            std::env::var("WHISPER_LANGUAGE").unwrap_or_else(|_| "ru".into()),
        )
        .text("response_format", "verbose_json");
    if provider == "openai" {
        form = form.text(
            "model",
            std::env::var("WHISPER_MODEL").unwrap_or_else(|_| "whisper-1".into()),
        );
    }
    let timeout = std::env::var("WHISPER_TIMEOUT_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600)
        .clamp(30, 3600);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client.post(endpoint(&base, &provider)).multipart(form);
    if let Ok(key) = std::env::var("WHISPER_API_KEY") {
        if !key.trim().is_empty() {
            request = request.bearer_auth(key.trim());
        }
    }
    let response=request.send().await.map_err(|_|"Нет связи с Whisper или истекло время ожидания. Проверьте работающий сервис, WHISPER_URL и WHISPER_PROVIDER; Ollama сама аудио не распознаёт.".to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Whisper вернул {status}: {}",
            body.chars().take(400).collect::<String>()
        ));
    }
    let data=response.json().await.map_err(|_|"Whisper вернул не JSON. Проверьте адрес: OpenAI-совместимый /v1/audio/transcriptions или whisper.cpp /inference.".to_string())?;
    parse_response(data)
}
