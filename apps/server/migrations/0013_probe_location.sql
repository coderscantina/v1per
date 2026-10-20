-- Where a probe runs, as its admins describe it: an ISO 3166-1 alpha-2 country, a city and the
-- hosting provider. `platform` is the operating system and architecture the agent reports.
ALTER TABLE probes ADD COLUMN country TEXT;
ALTER TABLE probes ADD COLUMN city TEXT;
ALTER TABLE probes ADD COLUMN provider TEXT;
ALTER TABLE probes ADD COLUMN platform TEXT;
