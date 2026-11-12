-- 004: аудиозаписи встреч, голосовые сообщения и расшифровка (текст по таймкодам)
ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS audio_file VARCHAR(255),
    ADD COLUMN IF NOT EXISTS audio_duration INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS transcript JSONB NOT NULL DEFAULT '[]'::jsonb;
