-- Two more things a page may show its visitors: ways to subscribe to its incidents and
-- maintenance, and response times per probe location.
ALTER TABLE pages ADD COLUMN subscriptions INTEGER NOT NULL DEFAULT 0 CHECK (subscriptions IN (0, 1));
ALTER TABLE pages ADD COLUMN probe_locations INTEGER NOT NULL DEFAULT 0 CHECK (probe_locations IN (0, 1));
