-- The longest time frame visitors may pick on a page; grant holders always get 90 days.
ALTER TABLE pages ADD COLUMN public_range TEXT NOT NULL DEFAULT '90d'
    CHECK (public_range IN ('24h', '7d', '30d', '90d'));
