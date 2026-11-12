use super::*;
use serde_json::json;
#[test]
fn url_supports_base_v1_and_complete_endpoint() {
    assert_eq!(
        endpoint("http://localhost:8000", "openai"),
        "http://localhost:8000/v1/audio/transcriptions"
    );
    assert_eq!(
        endpoint("http://localhost:8000/v1/", "openai"),
        "http://localhost:8000/v1/audio/transcriptions"
    );
    assert_eq!(
        endpoint("http://localhost:8000/v1/audio/transcriptions", "openai"),
        "http://localhost:8000/v1/audio/transcriptions"
    );
    assert_eq!(
        endpoint("http://localhost:8080", "whisper_cpp"),
        "http://localhost:8080/inference"
    );
}
#[test]
fn parses_segments_text_and_real_duration() {
    let t=parse_response(json!({"text":"Проверить заказ.","duration":3.12,"segments":[{"start":0,"end":2.8,"text":"Проверить заказ."}]})).unwrap();
    assert!(t.text.contains("Проверить заказ"));
    assert_eq!(t.duration_seconds, 3);
    assert_eq!(t.segments.len(), 1);
    let t = parse_response(json!({"segments":[{"start":1,"end":3,"text":"Печать"}]})).unwrap();
    assert_eq!(t.duration_seconds, 3);
    assert_eq!(
        parse_response(json!({"text":"Только текст"}))
            .unwrap()
            .duration_seconds,
        0
    );
    assert!(parse_response(json!({"text":"  ","segments":[]})).is_err());
}
#[test]
fn parses_whisper_cpp_and_rejects_invalid_timestamps() {
    let t = parse_response(
        json!({"transcription":[{"offsets":{"from":0,"to":3000},"text":"Проверить печать"}]}),
    )
    .unwrap();
    assert_eq!(t.duration_seconds, 3);
    assert_eq!(t.segments[0].start_seconds, 0.0);
    assert!(parse_response(json!({"segments":[{"start":-10,"end":3,"text":"bad"}]})).is_err());
}
