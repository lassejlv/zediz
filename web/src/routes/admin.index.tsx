import { createFileRoute, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  HardDrive,
  RefreshCw,
  RotateCcw,
  Trash2,
} from 'lucide-react';
import {
  adminNodesQuery,
  adminOverviewQuery,
  useAdminCheckAgentUpdate,
  useAdminDeleteNode,
  useAdminDrainNode,
  useAdminUpdateAgent,
  type AdminDeployment,
  type AdminNode,
} from '@/lib/admin';
import {
  Card,
  CopyableId,
  EmptyState,
  PageHeader,
  RelativeTime,
  Stack,
  StatCard,
  StatusPill,
  type SemanticStatus,
} from '@/components/ui';

export const Route = createFileRoute('/admin/')({
  loader: async ({ context }) => {
    await Promise.all([
      context.queryClient.ensureQueryData(adminOverviewQuery),
      context.queryClient.ensureQueryData(adminNodesQuery),
    ]);
  },
  component: AdminDashboard,
});

function AdminDashboard() {
  const overview = useQuery({ ...adminOverviewQuery, refetchInterval: 15_000 });
  const nodes = useQuery({ ...adminNodesQuery, refetchInterval: 15_000 });

  const counts = overview.data?.counts;
  const list = nodes.data ?? [];

  return (
    <Stack gap={6}>
      <PageHeader
        title="Admin"
        subtitle="Platform operations across workspaces, nodes, deployments, and users."
      />

      <div className="grid gap-3 md:grid-cols-4">
        <StatCard
          label="Nodes"
          value={`${counts?.ready_nodes ?? 0}/${counts?.nodes ?? 0}`}
          hint="ready / total"
          mono
        />
        <StatCard
          label="Deployments"
          value={counts?.deployments ?? 0}
          hint={`${counts?.running_deployments ?? 0} running`}
          mono
        />
        <StatCard
          label="Projects"
          value={counts?.projects ?? 0}
          hint={`${counts?.services ?? 0} services`}
          mono
        />
        <StatCard
          label="Pending users"
          value={overview.data?.pending_users ?? 0}
          hint={
            <Link to="/admin/users" className="hover:text-[var(--color-fg)]">
              Review accounts
            </Link>
          }
          mono
        />
      </div>

      <NodeManagement nodes={list} loading={nodes.isLoading} />

      <UnhealthyDeployments rows={overview.data?.unhealthy_deployments ?? []} />
    </Stack>
  );
}

function NodeManagement({
  nodes,
  loading,
}: {
  nodes: AdminNode[];
  loading: boolean;
}) {
  const drain = useAdminDrainNode();
  const check = useAdminCheckAgentUpdate();
  const update = useAdminUpdateAgent();
  const del = useAdminDeleteNode();

  return (
    <section>
      <div className="mb-2 flex items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium">Nodes</h2>
          <p className="mt-0.5 text-xs text-[var(--color-muted)]">
            Capacity, agent status, private networking, and workloads.
          </p>
        </div>
        <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
          <HardDrive className="h-3.5 w-3.5" />
          {nodes.length} active
        </div>
      </div>

      <Card className="overflow-hidden">
        {nodes.length === 0 ? (
          <EmptyState
            title={loading ? 'Loading nodes' : 'No active nodes'}
            body="Autoscaled nodes will show here after provisioning starts."
            className="border-0"
          />
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[1180px] text-sm">
              <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
                <tr>
                  <th className="px-4 py-2 font-medium">Node</th>
                  <th className="px-4 py-2 font-medium">Workspace</th>
                  <th className="px-4 py-2 font-medium">Capacity</th>
                  <th className="px-4 py-2 font-medium">Agent</th>
                  <th className="px-4 py-2 font-medium">Private network</th>
                  <th className="px-4 py-2 font-medium">Workloads</th>
                  <th className="px-4 py-2" />
                </tr>
              </thead>
              <tbody>
                {nodes.map((node) => (
                  <tr key={node.id} className="border-t border-[var(--color-border)] align-top">
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-2">
                        <StatusPill
                          status={statusTone(node.status)}
                          label={node.status}
                          pulse={node.status === 'provisioning'}
                        />
                      </div>
                      <div className="mt-1 font-mono text-xs">{node.name}</div>
                      <div className="mt-1 flex items-center gap-1.5 text-xs text-[var(--color-muted)]">
                        <CopyableId
                          value={node.id}
                          display={node.id.slice(0, 8)}
                          className="!text-[var(--color-muted)]"
                        />
                        <span>·</span>
                        <span>{node.public_ipv4 ?? 'no public ip'}</span>
                      </div>
                      <div className="mt-1 text-xs text-[var(--color-muted)]">
                        {node.hetzner_location ?? 'unknown'} ·{' '}
                        {node.hetzner_server_type ?? node.provider}
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <div className="font-medium">{node.workspace_name}</div>
                      <div className="font-mono text-xs text-[var(--color-muted)]">
                        {node.workspace_slug}
                      </div>
                      <div className="mt-1 text-xs text-[var(--color-muted)]">
                        created <RelativeTime date={node.created_at} />
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <ResourceLine
                        label="cpu"
                        used={node.used_cpu_millis}
                        total={node.total_cpu_millis}
                        suffix="m"
                      />
                      <ResourceLine
                        label="mem"
                        used={node.used_memory_mb}
                        total={node.total_memory_mb}
                        suffix="MB"
                      />
                      <ResourceLine
                        label="disk"
                        used={node.used_disk_mb}
                        total={node.total_disk_mb}
                        suffix="MB"
                      />
                    </td>
                    <td className="px-4 py-3">
                      <StatusPill
                        status={agentTone(node.agent_update_status)}
                        label={node.agent_update_status}
                        pulse={node.agent_update_status === 'updating'}
                      />
                      <div className="mt-1 font-mono text-xs text-[var(--color-muted)]">
                        {node.agent_version ?? 'unknown version'}
                      </div>
                      {node.agent_update_error ? (
                        <div className="mt-1 max-w-[220px] truncate text-xs text-red-400">
                          {node.agent_update_error}
                        </div>
                      ) : null}
                      <div className="mt-2 flex flex-wrap gap-1.5">
                        <IconButton
                          title="Check update"
                          pending={check.isPending && check.variables === node.id}
                          onClick={() => check.mutate(node.id)}
                        >
                          <RefreshCw className="h-3.5 w-3.5" />
                        </IconButton>
                        <IconButton
                          title="Update agent"
                          disabled={!node.agent_self_update_capable}
                          pending={update.isPending && update.variables === node.id}
                          onClick={() => update.mutate(node.id)}
                        >
                          <RotateCcw className="h-3.5 w-3.5" />
                        </IconButton>
                      </div>
                    </td>
                    <td className="px-4 py-3">
                      <StatusPill
                        status={node.private_network_capable ? 'ok' : 'warn'}
                        label={node.private_network_capable ? 'capable' : 'disabled'}
                      />
                      <div className="mt-1 font-mono text-xs text-[var(--color-muted)]">
                        {node.wireguard_mesh_ip ?? 'no mesh ip'}
                      </div>
                      {node.private_network_sync_error ? (
                        <div className="mt-1 max-w-[220px] truncate text-xs text-red-400">
                          {node.private_network_sync_error}
                        </div>
                      ) : (
                        <div className="mt-1 text-xs text-[var(--color-muted)]">
                          synced{' '}
                          {node.private_network_synced_at ? (
                            <RelativeTime date={node.private_network_synced_at} />
                          ) : (
                            'never'
                          )}
                        </div>
                      )}
                    </td>
                    <td className="px-4 py-3">
                      {node.workloads.length === 0 ? (
                        <span className="text-xs text-[var(--color-muted)]">idle</span>
                      ) : (
                        <div className="space-y-1">
                          {node.workloads.slice(0, 3).map((workload) => (
                            <div
                              key={`${workload.kind}-${workload.deployment_id}-${workload.build_id ?? ''}`}
                              className="text-xs"
                            >
                              <span className="font-mono">{workload.project_slug}</span>
                              <span className="text-[var(--color-muted)]"> / </span>
                              <span className="font-mono">{workload.service_slug}</span>
                              <span className="ml-2 text-[var(--color-muted)]">
                                {workload.kind}:{workload.status}
                              </span>
                            </div>
                          ))}
                          {node.workloads.length > 3 ? (
                            <div className="text-xs text-[var(--color-muted)]">
                              +{node.workloads.length - 3} more
                            </div>
                          ) : null}
                        </div>
                      )}
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1.5">
                        <IconButton
                          title="Drain node"
                          pending={drain.isPending && drain.variables === node.id}
                          onClick={() => drain.mutate(node.id)}
                        >
                          <AlertTriangle className="h-3.5 w-3.5" />
                        </IconButton>
                        <IconButton
                          title="Delete node"
                          danger
                          pending={del.isPending && del.variables?.id === node.id}
                          onClick={() => {
                            const force = node.workloads.length > 0;
                            const message = force
                              ? `Delete ${node.name} with ${node.workloads.length} active workloads? This will force provider cleanup.`
                              : `Delete ${node.name}? This will remove the provider server when one exists.`;
                            if (!window.confirm(message)) return;
                            del.mutate({
                              id: node.id,
                              force,
                            });
                          }}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </IconButton>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </section>
  );
}

function UnhealthyDeployments({ rows }: { rows: AdminDeployment[] }) {
  return (
    <section>
      <div className="mb-2 flex items-center justify-between gap-3">
        <h2 className="text-sm font-medium">Unhealthy deployments</h2>
        <span className="text-xs text-[var(--color-muted)]">{rows.length}</span>
      </div>
      <Card className="overflow-hidden">
        {rows.length === 0 ? (
          <EmptyState
            title="No unhealthy deployments"
            body="Errored, failing, and long-running transitional deployments will appear here."
            className="border-0"
          />
        ) : (
          <table className="w-full text-sm">
            <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
              <tr>
                <th className="px-4 py-2 font-medium">Service</th>
                <th className="px-4 py-2 font-medium">Status</th>
                <th className="px-4 py-2 font-medium">Reason</th>
                <th className="px-4 py-2 font-medium">Updated</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.id} className="border-t border-[var(--color-border)]">
                  <td className="px-4 py-3">
                    <div className="font-mono text-xs">
                      {row.workspace_slug}/{row.project_slug}/{row.service_slug}
                    </div>
                    <div className="mt-1 max-w-[340px] truncate font-mono text-xs text-[var(--color-muted)]">
                      {row.image_ref}
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <StatusPill status={statusTone(row.status)} label={row.status} />
                  </td>
                  <td className="max-w-[460px] truncate px-4 py-3 text-xs text-[var(--color-muted)]">
                    {row.reason ?? 'No reason recorded'}
                  </td>
                  <td className="px-4 py-3 text-xs">
                    <RelativeTime date={row.updated_at} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </section>
  );
}

function ResourceLine({
  label,
  used,
  total,
  suffix,
}: {
  label: string;
  used: number;
  total: number;
  suffix: string;
}) {
  const pct = total > 0 ? Math.min(100, Math.round((used / total) * 100)) : 0;
  return (
    <div className="mb-1.5 last:mb-0">
      <div className="mb-1 flex items-center justify-between gap-2 text-[11px]">
        <span className="text-[var(--color-muted)]">{label}</span>
        <span className="font-mono">
          {used}/{total}
          {suffix}
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-black/10 dark:bg-white/10">
        <div
          className="h-1.5 rounded-full bg-[var(--color-accent)]"
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

function IconButton({
  children,
  title,
  onClick,
  pending,
  disabled,
  danger,
}: {
  children: React.ReactNode;
  title: string;
  onClick: () => void;
  pending?: boolean;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      disabled={pending || disabled}
      className={[
        'inline-flex h-7 w-7 items-center justify-center rounded-md border transition-colors disabled:opacity-50',
        danger
          ? 'border-red-500/40 text-red-400 hover:bg-red-500/10'
          : 'border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]',
      ].join(' ')}
    >
      {pending ? <RefreshCw className="h-3.5 w-3.5 animate-spin" /> : children}
    </button>
  );
}

function statusTone(status: string): SemanticStatus {
  switch (status) {
    case 'ready':
    case 'running':
    case 'active':
      return 'ok';
    case 'provisioning':
    case 'pulling':
    case 'starting':
    case 'placing':
    case 'draining':
      return 'warn';
    case 'errored':
    case 'failing':
    case 'terminated':
      return 'error';
    default:
      return 'muted';
  }
}

function agentTone(status: string): SemanticStatus {
  switch (status) {
    case 'current':
      return 'ok';
    case 'available':
    case 'unknown':
      return 'warn';
    case 'updating':
    case 'restarting':
      return 'info';
    case 'failed':
    case 'check_failed':
      return 'error';
    default:
      return 'muted';
  }
}
