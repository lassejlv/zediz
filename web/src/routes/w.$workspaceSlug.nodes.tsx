import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { MoreHorizontal } from 'lucide-react';
import { useRef, useEffect, useState } from 'react';
import { nodesQuery, useDeleteNode, useDrainNode } from '@/lib/nodes';
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
import type { NodeSummary } from '@/lib/types';

export const Route = createFileRoute('/w/$workspaceSlug/nodes')({
  component: NodesPage,
});

function NodesPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const nodes = useQuery({ ...nodesQuery(workspaceSlug), refetchInterval: 5000 });
  const drain = useDrainNode(workspaceSlug);
  const del = useDeleteNode(workspaceSlug);

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
              onDrain={() => {
                if (confirm(`Drain ${n.name}?`)) drain.mutate(n.id);
              }}
              onDelete={() => {
                if (confirm(`Delete ${n.name}? This terminates the Hetzner VM.`)) {
                  del.mutate({ nodeId: n.id, force: true });
                }
              }}
            />
          ))}
        </Stack>
      ) : (
        <EmptyState
          title="No nodes"
          body="Provision one to start deploying containers. Zediz will autoscale from here when capacity fills up."
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

function NodeCard({
  node: n,
  canManage,
  onDrain,
  onDelete,
}: {
  node: NodeSummary;
  canManage: boolean;
  onDrain: () => void;
  onDelete: () => void;
}) {
  const cpuPct = n.total_cpu_millis > 0 ? (n.used_cpu_millis / n.total_cpu_millis) * 100 : 0;
  const memPct = n.total_memory_mb > 0 ? (n.used_memory_mb / n.total_memory_mb) * 100 : 0;
  const diskPct = n.total_disk_mb > 0 ? (n.used_disk_mb / n.total_disk_mb) * 100 : 0;

  return (
    <Card>
      <div className="flex items-start justify-between gap-4 border-b border-[var(--color-border)] px-5 py-4">
        <div className="min-w-0">
          <div className="flex items-center gap-3">
            <div className="font-medium">{n.name}</div>
            <StatusPill
              status={nodeStatusSemantic(n.status)}
              label={n.status}
              pulse={transientStatuses.includes(n.status)}
            />
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-[var(--color-muted)]">
            <span className="font-mono">{n.provider}</span>
            <span>·</span>
            <span>
              {(n.total_cpu_millis / 1000).toFixed(0)}× vCPU, {formatMb(n.total_memory_mb)} RAM,{' '}
              {formatMb(n.total_disk_mb)} disk
            </span>
            {n.public_ipv4 ? (
              <>
                <span>·</span>
                <CopyableId value={n.public_ipv4} />
              </>
            ) : null}
          </div>
        </div>
        {canManage ? (
          <NodeActionsMenu
            status={n.status}
            onDrain={onDrain}
            onDelete={onDelete}
          />
        ) : null}
      </div>

      <div className="grid grid-cols-1 gap-4 px-5 py-4 md:grid-cols-3">
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
    clamped >= 85
      ? 'bg-red-500'
      : clamped >= 70
        ? 'bg-amber-500'
        : 'bg-emerald-500';
  return (
    <div>
      <div className="flex items-center justify-between text-xs">
        <span className="text-[var(--color-muted)]">{label}</span>
        <span className="font-mono">
          {usedText} <span className="text-[var(--color-subtle)]">/ {totalText}</span>
        </span>
      </div>
      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-[color-mix(in_oklch,currentColor_8%,transparent)]">
        <div
          className={`h-full ${barColor} transition-[width] duration-300`}
          style={{ width: `${clamped}%` }}
        />
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
