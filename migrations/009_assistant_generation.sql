-- A bounded generation lease avoids holding a database connection while Ollama runs.
ALTER TABLE assistant_dialogs
    ADD COLUMN generation_token TEXT,
    ADD COLUMN generation_until TIMESTAMPTZ;
