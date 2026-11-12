-- A page may show its monitors' response times to every visitor, not only to grant holders.
ALTER TABLE pages ADD COLUMN response_times INTEGER NOT NULL DEFAULT 0 CHECK (response_times IN (0, 1));
