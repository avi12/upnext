-- The first-week limit is gone (`docs/VERDICTS.md`, "Limits"), and the day
-- a key sent its first verdict was kept only for it, so it isn't kept at
-- all now.
ALTER TABLE keys DROP COLUMN first_day;
