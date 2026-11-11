-- A postmortem the team writes once an incident is resolved: plain text and when it was last
-- set. Both are null without one.
ALTER TABLE incidents ADD COLUMN postmortem TEXT;
ALTER TABLE incidents ADD COLUMN postmortem_at INTEGER;
