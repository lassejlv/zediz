import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { AlertTriangle, Download, MoreHorizontal, RefreshCw, Server } from 'lucide-react';
import { useRef, useEffect, useState } from 'react';
import {
  nodesQuery,
  useCheckNodeAgentUpdate,
  useDeleteNode,
  useDrainNode,
  useUpdateNodeAgent,
} from '@/lib/nodes';
import { canAdmin, workspaceQuery } from '@/lib/workspaces';
import {
  Button,
  Card,
  EmptyState,
  PageHeader,
  Stack,
  StatusPill,
  RelativeTime,
  CopyableId,
  type SemanticStatus,
} from '@/components/ui';
import { ProvisionNodeSheet } from '@/components/provision-node-sheet';
import type { NodeSummary, NodeWorkloadSummary } from '@/lib/types';

export const Route = createFileRoute('/w/$workspaceSlug/nodes')({
  component: NodesPage,
});

function NodesPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const nodes = useQuery({ ...nodesQuery(workspaceSlug), refetchInterval: 5000 });
  const drain = useDrainNode(workspaceSlug);
  const del = useDeleteNode(workspaceSlug);
  const checkAgentUpdate = useCheckNodeAgentUpdate(workspaceSlug);
  const updateAgent = useUpdateNodeAgent(workspaceSlug);

  const canManage = canAdmin(workspace.data);

  return (
    <Stack gap={6}>
      <PageHeader
        title="Nodes"
        subtitle="Compute capacity where containers run. Hetzner nodes provision automatically when capacity runs out; idle nodes tear down after the autoscale TTL."
        actions={
          canManage ? (
            <ProvisionNodeSheet
              workspaceSlug={workspaceSlug}
              defaultLocation={workspace.data?.hetzner_location}
              defaultServerType={workspace.data?.default_server_type}
            >
              <Button>Provision node</Button>
            </ProvisionNodeSheet>
          ) : null
        }
      />

      {nodes.data?.length ? (
        <Stack gap={3}>
          {nodes.data.map((n) => (
            <NodeCard
              key={n.id}
              node={n}
              canManage={canManage && n.provider === 'hetzner'}
              onDrain={() => drain.mutate(n.id)}
              onDelete={() => del.mutate({ nodeId: n.id, force: true })}
              onCheckAgentUpdate={() => checkAgentUpdate.mutate(n.id)}
              onUpdateAgent={() => updateAgent.mutate(n.id)}
              drainPending={drain.isPending && drain.variables === n.id}
              deletePending={del.isPending && del.variables?.nodeId === n.id}
              checkAgentPending={checkAgentUpdate.isPending && checkAgentUpdate.variables === n.id}
              updateAgentPending={updateAgent.isPending && updateAgent.variables === n.id}
            />
          ))}
        </Stack>
      ) : (
        <EmptyState
          title="No nodes"
          body="Provision one to start deploying containers. Driftbase will autoscale from here when capacity fills up."
          cta={
            canManage ? (
              <ProvisionNodeSheet
                workspaceSlug={workspaceSlug}
                defaultLocation={workspace.data?.hetzner_location}
                defaultServerType={workspace.data?.default_server_type}
              >
                <Button>Provision node</Button>
              </ProvisionNodeSheet>
            ) : null
          }
        />
      )}
    </Stack>
  );
}

type ConfirmAction = 'drain' | 'delete' | null;

function NodeCard({
  node: n,
  canManage,
  onDrain,
  onDelete,
  onCheckAgentUpdate,
  onUpdateAgent,
  drainPending,
  deletePending,
  checkAgentPending,
  updateAgentPending,
}: {
  node: NodeSummary;
  canManage: boolean;
  onDrain: () => void;
  onDelete: () => void;
  onCheckAgentUpdate: () => void;
  onUpdateAgent: () => void;
  drainPending: boolean;
  deletePending: boolean;
  checkAgentPending: boolean;
  updateAgentPending: boolean;
}) {
  const [confirming, setConfirming] = useState<ConfirmAction>(null);

  const cpuPct = n.total_cpu_millis > 0 ? (n.used_cpu_millis / n.total_cpu_millis) * 100 : 0;
  const memPct = n.total_memory_mb > 0 ? (n.used_memory_mb / n.total_memory_mb) * 100 : 0;
  const diskPct = n.total_disk_mb > 0 ? (n.used_disk_mb / n.total_disk_mb) * 100 : 0;

  return (
    <Card className="overflow-hidden">
      {confirming ? (
        <ConfirmBanner
          action={confirming}
          nodeName={n.name}
          onCancel={() => setConfirming(null)}
          onConfirm={() => {
            if (confirming === 'drain') onDrain();
            if (confirming === 'delete') onDelete();
            setConfirming(null);
          }}
          pending={confirming === 'drain' ? drainPending : deletePending}
        />
      ) : null}

      <div className="flex items-start justify-between gap-4 px-5 py-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2.5">
            <Server className="h-3.5 w-3.5 text-[var(--color-muted)]" />
            <span className="font-medium">{n.name}</span>
            <StatusPill
              status={nodeStatusSemantic(n.status)}
              label={n.status}
              pulse={transientStatuses.includes(n.status)}
            />
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--color-muted)]">
            <Tag>{n.provider}</Tag>
            <span>
              {(n.total_cpu_millis / 1000).toFixed(0)}× vCPU ·{' '}
              {formatMb(n.total_memory_mb)} RAM · {formatMb(n.total_disk_mb)} disk
            </span>
            {n.public_ipv4 ? <CopyableId value={n.public_ipv4} /> : null}
          </div>
        </div>
        {canManage ? (
          <NodeActionsMenu
            status={n.status}
            onDrain={() => setConfirming('drain')}
            onDelete={() => setConfirming('delete')}
          />
        ) : null}
      </div>

      <div className="grid grid-cols-1 gap-4 border-t border-[var(--color-border)] px-5 py-4 md:grid-cols-3">
        <CapacityBar
          label="CPU"
          usedText={`${n.used_cpu_millis}m`}
          totalText={`${n.total_cpu_millis}m`}
          percent={cpuPct}
        />
        <CapacityBar
          label="Memory"
          usedText={formatMb(n.used_memory_mb)}
          totalText={formatMb(n.total_memory_mb)}
          percent={memPct}
        />
        <CapacityBar
          label="Disk"
          usedText={formatMb(n.used_disk_mb)}
          totalText={formatMb(n.total_disk_mb)}
          percent={diskPct}
        />
      </div>

      <AgentUpdatePanel
        node={n}
        canManage={canManage}
        onCheck={onCheckAgentUpdate}
        onUpdate={onUpdateAgent}
        checkPending={checkAgentPending}
        updatePending={updateAgentPending}
      />

      {n.workloads.length > 0 ? (
        <div className="border-t border-[var(--color-border)] px-5 py-4">
          <div className="mb-3 flex items-baseline justify-between">
            <span className="text-[11px] uppercase tracking-[0.18em] text-[var(--color-muted)]">
              Active workloads
            </span>
            <span className="text-[11px] text-[var(--color-subtle)]">
              {n.workloads.length}
            </span>
          </div>
          <div className="space-y-1.5">
            {n.workloads.map((workload) => (
              <WorkloadRow
                key={`${workload.kind}-${workload.deployment_id}-${workload.build_id ?? 'runtime'}`}
                workload={workload}
              />
            ))}
          </div>
        </div>
      ) : null}

      <div className="flex items-center justify-between border-t border-[var(--color-border)] px-5 py-2.5 text-xs text-[var(--color-muted)]">
        <span>
          Added <RelativeTime date={n.created_at} />
        </span>
        <span>
          {n.last_seen_at ? (
            <>
              seen <RelativeTime date={n.last_seen_at} />
            </>
          ) : (
            'never seen'
          )}
        </span>
      </div>
    </Card>
  );
}

function AgentUpdatePanel({
  node,
  canManage,
  onCheck,
  onUpdate,
  checkPending,
  updatePending,
}: {
  node: NodeSummary;
  canManage: boolean;
  onCheck: () => void;
  onUpdate: () => void;
  checkPending: boolean;
  updatePending: boolean;
}) {
  const updateAvailable =
    node.agent_self_update_capable && node.agent_update_status === 'available';
  const updateBusy = ['updating', 'restarting'].includes(node.agent_update_status);
  const checkDisabled = checkPending || updatePending || updateBusy;
  const updateDisabled = updatePending || checkPending || updateBusy || !updateAvailable;

  return (
    <div className="border-t border-[var(--color-border)] px-5 py-4">
      <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
        <div className="min-w-0 space-y-2">
          <div className="flex flex-wrap items-center gap-2.5">
            <span className="text-[11px] uppercase tracking-[0.18em] text-[var(--color-muted)]">
              Agent
            </span>
            <StatusPill
              status={agentUpdateSemantic(node)}
              label={agentUpdateLabel(node)}
              pulse={updateBusy}
            />
            {node.agent_version ? <Tag>v{node.agent_version}</Tag> : null}
          </div>
          <div className="grid grid-cols-1 gap-x-5 gap-y-1 text-xs text-[var(--color-muted)] lg:grid-cols-2">
            <AgentMeta label="Image" value={node.agent_image_ref ?? 'unknown'} />
            <AgentMeta label="Digest" value={formatDigest(node.agent_image_digest)} />
            <AgentMeta
              label="Target"
              value={formatDigest(node.agent_update_target_digest)}
            />
            <div>
              <span className="text-[var(--color-subtle)]">Checked </span>
              {node.agent_update_checked_at ? (
                <RelativeTime date={node.agent_update_checked_at} />
              ) : (
                <span>never</span>
              )}
            </div>
          </div>
          {node.agent_update_error ? (
            <div className="text-xs text-red-400">{node.agent_update_error}</div>
          ) : null}
        </div>

        {canManage ? (
          <div className="flex shrink-0 flex-wrap gap-2 md:justify-end">
            <Button
              variant="secondary"
              onClick={onCheck}
              disabled={checkDisabled}
              className="gap-1.5"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${checkPending ? 'animate-spin' : ''}`} />
              <span>{checkPending ? 'Checking' : 'Check update'}</span>
            </Button>
            {updateAvailable ? (
              <Button
                variant="primary"
                onClick={onUpdate}
                disabled={updateDisabled}
                className="gap-1.5"
              >
                <Download className="h-3.5 w-3.5" />
                <span>{updatePending ? 'Updating' : 'Update agent'}</span>
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function AgentMeta({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <span className="text-[var(--color-subtle)]">{label} </span>
      <span className="break-all font-mono text-[11px] text-[var(--color-fg)]">{value}</span>
    </div>
  );
}

function agentUpdateSemantic(node: NodeSummary): SemanticStatus {
  if (!node.agent_self_update_capable) return 'muted';
  switch (node.agent_update_status) {
    case 'current':
      return 'ok';
    case 'available':
    case 'updating':
    case 'restarting':
      return 'warn';
    case 'failed':
    case 'check_failed':
      return 'error';
    default:
      return 'muted';
  }
}

function agentUpdateLabel(node: NodeSummary): string {
  if (!node.agent_self_update_capable) return 'unsupported';
  switch (node.agent_update_status) {
    case 'current':
      return 'current';
    case 'available':
      return 'update available';
    case 'updating':
      return 'updating';
    case 'restarting':
      return 'restarting';
    case 'failed':
      return 'failed';
    case 'check_failed':
      return 'check failed';
    default:
      return 'not checked';
  }
}

function formatDigest(digest: string | null): string {
  if (!digest) return 'unknown';
  if (digest.startsWith('sha256:') && digest.length > 19) {
    return `${digest.slice(0, 19)}...`;
  }
  return digest;
}

function ConfirmBanner({
  action,
  nodeName,
  onCancel,
  onConfirm,
  pending,
}: {
  action: Exclude<ConfirmAction, null>;
  nodeName: string;
  onCancel: () => void;
  onConfirm: () => void;
  pending: boolean;
}) {
  const copy =
    action === 'drain'
      ? {
          title: `Drain ${nodeName}?`,
          body: 'Existing workloads keep running but no new work is scheduled here.',
          cta: pending ? 'Draining…' : 'Drain',
          variant: 'primary' as const,
          accent: 'border-amber-500/30 bg-amber-500/[0.06]',
          icon: 'text-amber-500',
        }
      : {
          title: `Delete ${nodeName}?`,
          body: 'The Hetzner VM is terminated. This cannot be undone.',
          cta: pending ? 'Deleting…' : 'Delete',
          variant: 'danger' as const,
          accent: 'border-red-500/30 bg-red-500/[0.06]',
          icon: 'text-red-500',
        };
  return (
    <div
      className={`flex flex-col gap-3 border-b ${copy.accent} px-5 py-3.5 md:flex-row md:items-center md:justify-between`}
    >
      <div className="flex items-start gap-2.5">
        <AlertTriangle className={`mt-0.5 h-4 w-4 shrink-0 ${copy.icon}`} />
        <div>
          <div className="text-sm font-medium">{copy.title}</div>
          <div className="text-xs text-[var(--color-muted)]">{copy.body}</div>
        </div>
      </div>
      <div className="flex shrink-0 gap-2 md:self-start">
        <Button variant="ghost" onClick={onCancel} disabled={pending}>
          Cancel
        </Button>
        <Button variant={copy.variant} onClick={onConfirm} disabled={pending}>
          {copy.cta}
        </Button>
      </div>
    </div>
  );
}

function Tag({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-[var(--color-fg)]">
      {children}
    </span>
  );
}

const transientStatuses = ['provisioning', 'draining', 'pulling', 'starting'];

function nodeStatusSemantic(s: string): SemanticStatus {
  switch (s) {
    case 'ready':
      return 'ok';
    case 'provisioning':
    case 'draining':
      return 'warn';
    case 'terminated':
    case 'errored':
      return 'error';
    default:
      return 'muted';
  }
}

function WorkloadRow({ workload }: { workload: NodeWorkloadSummary }) {
  const kindColor =
    workload.kind === 'build' ? 'text-indigo-400' : 'text-emerald-500';
  return (
    <div className="group flex flex-col gap-2 rounded-md border border-[var(--color-border)] bg-black/[0.02] px-3 py-2.5 transition-colors hover:border-[var(--color-border-strong)] dark:bg-white/[0.02] md:flex-row md:items-center md:justify-between">
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <span className={`font-mono text-[10px] uppercase tracking-wider ${kindColor}`}>
            {workload.kind}
          </span>
          <span className="text-[var(--color-subtle)]">·</span>
          <span className="font-medium">
            {workload.project_slug} / {workload.service_slug}
          </span>
          <StatusPill
            status={workloadStatusSemantic(workload.status)}
            label={workload.status}
            pulse={[
              'queued',
              'cloning',
              'building',
              'pushing',
              'pending',
              'pulling',
              'starting',
            ].includes(workload.status)}
          />
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--color-muted)]">
          {workload.build_id ? (
            <CopyableId
              value={workload.build_id}
              display={`build ${workload.build_id.slice(0, 8)}`}
            />
          ) : null}
          <CopyableId
            value={workload.deployment_id}
            display={`deploy ${workload.deployment_id.slice(0, 8)}`}
          />
        </div>
      </div>
      <div className="text-xs font-mono text-[var(--color-muted)]">
        {workload.cpu_millis}m · {formatMb(workload.memory_mb)} · {formatMb(workload.disk_mb)}
      </div>
    </div>
  );
}

function workloadStatusSemantic(status: string): SemanticStatus {
  switch (status) {
    case 'running':
    case 'succeeded':
      return 'ok';
    case 'queued':
    case 'cloning':
    case 'building':
    case 'pushing':
    case 'pending':
    case 'placing':
    case 'pulling':
    case 'starting':
      return 'warn';
    case 'failed':
    case 'errored':
    case 'failing':
      return 'error';
    default:
      return 'muted';
  }
}

function CapacityBar({
  label,
  usedText,
  totalText,
  percent,
}: {
  label: string;
  usedText: string;
  totalText: string;
  percent: number;
}) {
  const clamped = Math.max(0, Math.min(100, percent));
  const barColor =
    clamped >= 85 ? 'bg-red-500' : clamped >= 70 ? 'bg-amber-500' : 'bg-emerald-500';
  const pctColor =
    clamped >= 85
      ? 'text-red-400'
      : clamped >= 70
        ? 'text-amber-400'
        : 'text-[var(--color-muted)]';

  return (
    <div>
      <div className="flex items-baseline justify-between text-xs">
        <span className="text-[11px] uppercase tracking-wider text-[var(--color-muted)]">
          {label}
        </span>
        <span className={`font-mono text-[11px] ${pctColor}`}>
          {clamped.toFixed(clamped >= 10 ? 0 : 1)}%
        </span>
      </div>
      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-black/[0.06] dark:bg-white/[0.06]">
        <div
          className={`h-full ${barColor} transition-[width] duration-500 ease-out`}
          style={{ width: `${clamped}%` }}
        />
      </div>
      <div className="mt-1.5 flex items-center justify-between text-[11px] font-mono text-[var(--color-muted)]">
        <span className="text-[var(--color-fg)]">{usedText}</span>
        <span className="text-[var(--color-subtle)]">/ {totalText}</span>
      </div>
    </div>
  );
}

function formatMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${mb} MB`;
}

function NodeActionsMenu({
  status,
  onDrain,
  onDelete,
}: {
  status: string;
  onDrain: () => void;
  onDelete: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') setOpen(false);
    }
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="inline-flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-black/5 hover:text-[var(--color-fg)] dark:hover:bg-white/5"
        aria-label="Node actions"
      >
        <MoreHorizontal className="h-4 w-4" />
      </button>
      {open ? (
        <div className="absolute right-0 top-full z-20 mt-1 w-40 overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface-elevated)] py-1 text-sm shadow-lg">
          {status === 'ready' ? (
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                onDrain();
              }}
              className="block w-full px-3 py-1.5 text-left hover:bg-black/5 dark:hover:bg-white/5"
            >
              Drain
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => {
              setOpen(false);
              onDelete();
            }}
            className="block w-full px-3 py-1.5 text-left text-red-400 hover:bg-red-500/10"
          >
            Delete
          </button>
        </div>
      ) : null}
    </div>
  );
}
