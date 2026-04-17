-- Phase 5 follow-up: generalise `build_context` to `root_dir` (the subdir
-- within the repo where the service lives, useful for monorepos) and let
-- users pick Railpack as an alternative to a hand-written Dockerfile.

ALTER TABLE services RENAME COLUMN build_context TO root_dir;

ALTER TABLE services ADD COLUMN builder TEXT NOT NULL DEFAULT 'dockerfile'
    CHECK (builder IN ('dockerfile', 'railpack'));
