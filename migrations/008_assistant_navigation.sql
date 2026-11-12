ALTER TABLE assistant_messages ADD COLUMN navigation JSONB NOT NULL DEFAULT '[]'::jsonb;
