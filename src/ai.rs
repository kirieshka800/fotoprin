//! Вызов локальной модели (Ollama) для новых разделов. Настройки: OLLAMA_URL, OLLAMA_MODEL.
//! Существующая логика протоколов и чата в main.rs не затронута.

use serde_json::{json, Value};
use std::time::Duration;

/// Shared by assistant, schedule and meeting calls. Both URL forms are accepted.
pub(crate) fn generate_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    if base.ends_with("/api/generate") {
        base.to_owned()
    } else if base.ends_with("/api") {
        format!("{base}/generate")
    } else {
        format!("{base}/api/generate")
    }
}

pub async fn generate_json(prompt: String, schema: Value) -> Result<Value, String> {
    let url = generate_url(
        &std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string()),
    );
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_string());

    let context_size = std::env::var("OLLAMA_NUM_CTX")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(32768)
        .clamp(2048, 131072);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| format!("не удалось создать HTTP-клиент: {}", e))?;

    let response = client
        .post(&url)
        .json(&json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "format": schema,
            "options": { "temperature": 0.2, "num_ctx": context_size }
        }))
        .send()
        .await
        .map_err(|e| format!("нет связи с Ollama: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ollama вернула статус {}: {}", status, body));
    }

    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("ответ Ollama не разобран: {}", e))?;
    let text = body["response"]
        .as_str()
        .ok_or_else(|| "Ollama вернула пустой ответ".to_string())?;
    serde_json::from_str(text).map_err(|e| format!("модель вернула не JSON: {}", e))
}
