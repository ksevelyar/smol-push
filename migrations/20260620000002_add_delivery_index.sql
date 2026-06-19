CREATE INDEX IF NOT EXISTS idx_pushes_delivery
ON pushes(platform, status, inserted_at);
