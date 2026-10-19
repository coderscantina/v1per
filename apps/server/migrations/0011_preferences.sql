-- Display settings a person keeps across browsers, as the JSON of `account.preferences`.
ALTER TABLE users ADD COLUMN preferences TEXT NOT NULL DEFAULT '{}';
