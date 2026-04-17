-- Per-sample history of container stats the agent reports on each
-- heartbeat. Complements deployments.runtime_metrics (which stays as the
-- quick "latest" JSONB for list views) by giving the Metrics tab a real
-- time series it can load on mount and slice by time range.
--
-- Retained for one hour by a scheduler task; CASCADE on deployment delete
-- keeps cleanup automatic when a deployment row goes away.
CREATE TABLE IF NOT EXISTS deployment_metrics (
    deployment_id TEXT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    ts TIMESTAMPTZ NOT NULL,
    cpu_percent REAL NOT NULL,
    memory_bytes BIGINT NOT NULL,
    memory_limit_bytes BIGINT,
    rx_bytes BIGINT NOT NULL,
    tx_bytes BIGINT NOT NULL,
    PRIMARY KEY (deployment_id, ts)
);

CREATE INDEX IF NOT EXISTS idx_deployment_metrics_ts
    ON deployment_metrics (deployment_id, ts DESC);
