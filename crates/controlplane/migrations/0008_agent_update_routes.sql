-- Allow the new `update_routes` command kind so the scheduler can push Caddy
-- config refreshes to nodes when service_domains or placements change.
ALTER TABLE agent_commands DROP CONSTRAINT agent_commands_kind_check;
ALTER TABLE agent_commands ADD CONSTRAINT agent_commands_kind_check
    CHECK (kind IN (
        'pull_and_run', 'stop', 'restart', 'remove',
        'drain', 'prune', 'update_routes'
    ));
