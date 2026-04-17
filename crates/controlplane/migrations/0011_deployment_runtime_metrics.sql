-- Latest container metrics sample the agent reported for a deployment.
-- Overwritten on every heartbeat while the container is running. We don't
-- keep history server-side; the UI maintains its own rolling buffer over
-- the tab's lifetime.
ALTER TABLE deployments
    ADD COLUMN IF NOT EXISTS runtime_metrics JSONB;
