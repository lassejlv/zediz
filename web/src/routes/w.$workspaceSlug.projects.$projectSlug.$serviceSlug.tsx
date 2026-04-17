import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { Trash2 } from 'lucide-react';
import {
  serviceQuery,
  serviceDeploymentsQuery,
  useDeleteService,
  useDeployService,
} from '@/lib/services';
import {
  deploymentTone,
  useRestartDeployment,
  useStopDeployment,
} from '@/lib/deployments';
import { canAdmin, canWrite, workspaceQuery } from '@/lib/workspaces';
import { domainsQuery } from '@/lib/domains';
import { ApiError } from '@/lib/api';
import {
  Button,
  Card,
  CopyableId,
  ErrorText,
  PageHeader,
  RelativeTime,
  Stack,
  StatCard,
  StatusPill,
  type SemanticStatus,
} from '@/components/ui';
import { DomainsSection } from '@/components/domains-section';
import type { DeploymentSummary, ServiceSummary } from '@/lib/types';

export const Route = createFileRoute(
  '/w/$workspaceSlug/projects/$projectSlug/$serviceSlug',
)({
  component: ServicePage,
});

type Tab = 'overview' | 'deployments' | 'domains' | 'logs' | 'settings';

function ServicePage() {
  const { workspaceSlug, projectSlug, serviceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const service = useQuery(serviceQuery(workspaceSlug, projectSlug, serviceSlug));
  const deployments = useQuery({
    ...serviceDeploymentsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 3000,
  });
  const domains = useQuery(domainsQuery(workspaceSlug, projectSlug, serviceSlug));
  const deploy = useDeployService(workspaceSlug, projectSlug, serviceSlug);
  const deleteService = useDeleteService(workspaceSlug, projectSlug);
  const stop = useStopDeployment();
  const restart = useRestartDeployment();

  const canDeploy = canWrite(workspace.data);
  const canDelete = canAdmin(workspace.data);

  const [error, setError] = useState<string | null>(null);
  const [activeDeploymentId, setActiveDeploymentId] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('overview');

  const latest = deployments.data?.[0];
  useEffect(() => {
    if (!activeDeploymentId && latest && latest.container_id) {
      setActiveDeploymentId(latest.id);
    }
  }, [activeDeploymentId, latest]);

  async function onDeploy() {
    setError(null);
    try {
      const d = await deploy.mutateAsync();
      setActiveDeploymentId(d.id);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Deploy failed');
    }
  }

  // Cmd/Ctrl+Enter triggers deploy anywhere on the page
  useEffect(() => {
    if (!canDeploy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        void onDeploy();
      }
    }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canDeploy]);

  const svc = service.data;
  const serviceStatus = computeServiceStatus(latest);
  const deployLabel = computeDeployLabel(latest);

  return (
    <Stack gap={6}>
      <PageHeader
        breadcrumbs={[
          { label: 'Projects', to: `/w/${workspaceSlug}/projects` },
          { label: projectSlug, to: `/w/${workspaceSlug}/projects/${projectSlug}` },
          { label: serviceSlug },
        ]}
        title={svc?.name ?? serviceSlug}
        subtitle={
          svc?.image_ref ? <span className="font-mono text-xs">{svc.image_ref}</span> : '—'
        }
        status={
          <StatusPill
            status={serviceStatus.tone}
            label={serviceStatus.label}
            pulse={serviceStatus.pulse}
          />
        }
        actions={
          <>
            {canDeploy ? (
              <Button onClick={onDeploy} disabled={deploy.isPending} title="⌘ ↵">
                {deploy.isPending ? 'Deploying…' : deployLabel}
              </Button>
            ) : null}
            {canDelete ? (
              <Button
                variant="danger"
                onClick={async () => {
                  if (!confirm(`Delete service ${serviceSlug}? This cannot be undone.`)) return;
                  try {
                    await deleteService.mutateAsync(serviceSlug);
                    window.location.href = `/w/${workspaceSlug}/projects/${projectSlug}`;
                  } catch (err) {
                    setError(err instanceof ApiError ? err.message : 'Delete failed');
                  }
                }}
                title="Delete service"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            ) : null}
          </>
        }
      />

      {error ? <ErrorText>{error}</ErrorText> : null}

      {svc ? (
        <SummaryStrip
          service={svc}
          latest={latest}
          primaryDomain={domains.data?.[0]?.hostname ?? null}
          serviceStatus={serviceStatus}
        />
      ) : null}

      <Tabs value={tab} onChange={setTab} />

      {tab === 'overview' ? (
        <OverviewTab
          service={svc}
          deployments={deployments.data ?? []}
          canManage={canDeploy}
          onSeeAll={() => setTab('deployments')}
          onRestart={(id) => restart.mutate(id)}
          onStop={(id) => stop.mutate(id)}
          activeId={activeDeploymentId}
          onSelect={(id) => setActiveDeploymentId(id)}
        />
      ) : null}

      {tab === 'deployments' ? (
        <DeploymentsTable
          deployments={deployments.data ?? []}
          canManage={canDeploy}
          activeId={activeDeploymentId}
          onSelect={(id) => {
            setActiveDeploymentId(id);
            setTab('logs');
          }}
          onStop={(id) => stop.mutate(id)}
          onRestart={(id) => restart.mutate(id)}
        />
      ) : null}

      {tab === 'domains' ? (
        <DomainsSection
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          serviceSlug={serviceSlug}
          canManage={canDeploy}
          defaultPort={svc?.ports?.[0]?.container_port ?? null}
        />
      ) : null}

      {tab === 'logs' ? (
        <LogsTab
          deployments={deployments.data ?? []}
          activeId={activeDeploymentId}
          onSelect={(id) => setActiveDeploymentId(id)}
        />
      ) : null}

      {tab === 'settings' ? <SettingsTab service={svc} /> : null}
    </Stack>
  );
}

/* ---------- status computation ---------- */

interface ServiceStatus {
  tone: SemanticStatus;
  label: string;
  pulse: boolean;
}

function computeServiceStatus(d: DeploymentSummary | undefined): ServiceStatus {
  const { tone, pulse } = deploymentTone(d?.status);
  const label = !d
    ? 'never deployed'
    : d.status === 'pulling'
      ? 'pulling image'
      : d.status === 'pending' || d.status === 'placing'
        ? 'pending'
        : d.status;
  return { tone, label, pulse };
}

function computeDeployLabel(latest: DeploymentSummary | undefined): string {
  if (!latest) return 'Deploy';
  if (['pending', 'placing', 'pulling', 'starting'].includes(latest.status)) return 'Deploying…';
  if (latest.status === 'running') return 'Redeploy';
  return 'Deploy';
}

/* ---------- summary strip ---------- */

function SummaryStrip({
  service,
  latest,
  primaryDomain,
  serviceStatus,
}: {
  service: ServiceSummary;
  latest: DeploymentSummary | undefined;
  primaryDomain: string | null;
  serviceStatus: ServiceStatus;
}) {
  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
      <StatCard label="Status" value={serviceStatus.label} />
      <StatCard label="Replicas" value={service.replicas} mono />
      <StatCard
        label="Primary domain"
        value={
          primaryDomain ? (
            <span className="font-mono text-xs">{primaryDomain}</span>
          ) : (
            <span className="text-[var(--color-muted)]">—</span>
          )
        }
      />
      <StatCard
        label="Last deploy"
        value={
          latest ? (
            <RelativeTime date={latest.created_at} className="text-[var(--color-fg)]" />
          ) : (
            <span className="text-[var(--color-muted)]">never</span>
          )
        }
      />
      <StatCard label="Memory" value={`${service.resources.memory_mb} MB`} mono />
      <StatCard label="CPU" value={`${service.resources.cpu_millis}m`} mono />
    </div>
  );
}

/* ---------- tabs ---------- */

function Tabs({ value, onChange }: { value: Tab; onChange: (t: Tab) => void }) {
  const tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'deployments', label: 'Deployments' },
    { id: 'domains', label: 'Domains' },
    { id: 'logs', label: 'Logs' },
    { id: 'settings', label: 'Settings' },
  ];
  return (
    <div className="flex gap-1 border-b border-[var(--color-border)]">
      {tabs.map((t) => {
        const active = t.id === value;
        return (
          <button
            key={t.id}
            type="button"
            onClick={() => onChange(t.id)}
            className={[
              '-mb-px border-b-2 px-3 py-2 text-sm transition-colors',
              active
                ? 'border-[var(--color-accent)] text-[var(--color-fg)]'
                : 'border-transparent text-[var(--color-muted)] hover:text-[var(--color-fg)]',
            ].join(' ')}
          >
            {t.label}
          </button>
        );
      })}
    </div>
  );
}

/* ---------- overview tab ---------- */

function OverviewTab({
  service,
  deployments,
  canManage,
  onSeeAll,
  onRestart,
  onStop,
  activeId,
  onSelect,
}: {
  service: ServiceSummary | undefined;
  deployments: DeploymentSummary[];
  canManage: boolean;
  onSeeAll: () => void;
  onRestart: (id: string) => void;
  onStop: (id: string) => void;
  activeId: string | null;
  onSelect: (id: string) => void;
}) {
  if (!service) return null;
  const recent = deployments.slice(0, 5);
  return (
    <Stack gap={4}>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="p-5">
          <div className="mb-3 text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            Resources
          </div>
          <dl className="space-y-2 text-sm">
            <Row label="CPU" value={`${service.resources.cpu_millis}m`} />
            <Row label="Memory" value={`${service.resources.memory_mb} MB`} />
            <Row label="Disk" value={`${service.resources.disk_mb} MB`} />
            <Row label="Replicas" value={String(service.replicas)} />
            <Row label="Restart policy" value={service.restart_policy} />
          </dl>
        </Card>
        <Card className="p-5">
          <div className="mb-3 text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            General
          </div>
          <dl className="space-y-2 text-sm">
            <Row
              label="Image"
              value={
                service.image_ref ? (
                  <CopyableId value={service.image_ref} />
                ) : (
                  <span className="text-[var(--color-muted)]">—</span>
                )
              }
            />
            <Row
              label="Source"
              value={<span className="font-mono text-xs">{service.source}</span>}
            />
            <Row label="Ports" value={service.ports.map((p) => p.container_port).join(', ') || '—'} />
            <Row
              label="Created"
              value={<RelativeTime date={service.created_at} className="text-[var(--color-fg)]" />}
            />
          </dl>
        </Card>
      </div>

      <Card className="overflow-hidden">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2.5">
          <div className="text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            Recent deployments
          </div>
          <button
            type="button"
            onClick={onSeeAll}
            className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            See all →
          </button>
        </div>
        {recent.length === 0 ? (
          <div className="px-4 py-6 text-center text-sm text-[var(--color-muted)]">
            No deployments yet. Hit Deploy to start one.
          </div>
        ) : (
          <table className="w-full text-sm">
            <tbody>
              {recent.map((d) => (
                <DeploymentRow
                  key={d.id}
                  d={d}
                  active={activeId === d.id}
                  canManage={canManage}
                  onSelect={() => onSelect(d.id)}
                  onRestart={() => onRestart(d.id)}
                  onStop={() => onStop(d.id)}
                />
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </Stack>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <dt className="text-xs text-[var(--color-muted)]">{label}</dt>
      <dd className="font-mono text-xs">{value}</dd>
    </div>
  );
}

/* ---------- deployments tab ---------- */

function DeploymentsTable({
  deployments,
  canManage,
  activeId,
  onSelect,
  onStop,
  onRestart,
}: {
  deployments: DeploymentSummary[];
  canManage: boolean;
  activeId: string | null;
  onSelect: (id: string) => void;
  onStop: (id: string) => void;
  onRestart: (id: string) => void;
}) {
  return (
    <Card className="overflow-hidden">
      <table className="w-full text-sm">
        <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          <tr>
            <th className="px-4 py-2.5 font-medium">Status</th>
            <th className="px-4 py-2.5 font-medium">Image</th>
            <th className="px-4 py-2.5 font-medium">Started</th>
            <th className="px-4 py-2.5 font-medium">Duration</th>
            <th className="px-4 py-2.5 font-medium">Reason</th>
            <th className="px-4 py-2.5" />
          </tr>
        </thead>
        <tbody>
          {deployments.length ? (
            deployments.map((d) => (
              <DeploymentRow
                key={d.id}
                d={d}
                active={activeId === d.id}
                canManage={canManage}
                onSelect={() => onSelect(d.id)}
                onStop={() => onStop(d.id)}
                onRestart={() => onRestart(d.id)}
                showDuration
                showReason
                showImage
              />
            ))
          ) : (
            <tr>
              <td colSpan={6} className="px-4 py-6 text-center text-sm text-[var(--color-muted)]">
                No deployments yet.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </Card>
  );
}

function DeploymentRow({
  d,
  active,
  canManage,
  onSelect,
  onStop,
  onRestart,
  showDuration = false,
  showReason = false,
  showImage = true,
}: {
  d: DeploymentSummary;
  active: boolean;
  canManage: boolean;
  onSelect: () => void;
  onStop: () => void;
  onRestart: () => void;
  showDuration?: boolean;
  showReason?: boolean;
  showImage?: boolean;
}) {
  const stoppable =
    d.status === 'running' || d.status === 'starting' || d.status === 'pulling';
  const status = deploymentTone(d.status);
  return (
    <tr
      className={[
        'cursor-pointer border-t border-[var(--color-border)]',
        active ? 'bg-black/5 dark:bg-white/5' : 'hover:bg-black/3 dark:hover:bg-white/2',
      ].join(' ')}
      onClick={onSelect}
    >
      <td className="px-4 py-2">
        <StatusPill status={status.tone} label={d.status} pulse={status.pulse} />
      </td>
      {showImage ? (
        <td className="px-4 py-2">
          <CopyableId value={d.image_ref} display={truncateImage(d.image_ref)} />
        </td>
      ) : null}
      <td className="px-4 py-2 text-xs text-[var(--color-muted)]">
        <RelativeTime date={d.created_at} />
      </td>
      {showDuration ? (
        <td className="px-4 py-2 font-mono text-xs text-[var(--color-muted)]">
          {formatDuration(d)}
        </td>
      ) : null}
      {showReason ? (
        <td className="px-4 py-2 text-xs text-[var(--color-muted)]">
          {d.reason ? <span title={d.reason}>{truncate(d.reason, 40)}</span> : '—'}
        </td>
      ) : null}
      <td className="px-4 py-2 text-right">
        {canManage ? (
          <div className="flex justify-end gap-2">
            {stoppable ? (
              <Button
                variant="secondary"
                onClick={(e) => {
                  e.stopPropagation();
                  onStop();
                }}
              >
                Stop
              </Button>
            ) : null}
            <Button
              variant="secondary"
              onClick={(e) => {
                e.stopPropagation();
                onRestart();
              }}
            >
              Restart
            </Button>
          </div>
        ) : null}
      </td>
    </tr>
  );
}

function truncateImage(ref: string): string {
  if (ref.length <= 40) return ref;
  const at = ref.indexOf('@');
  if (at === -1) return ref.slice(0, 37) + '…';
  const prefix = ref.slice(0, at);
  const digest = ref.slice(at + 1);
  return `${prefix}@${digest.slice(0, 12)}…`;
}

function truncate(s: string, max: number): string {
  return s.length <= max ? s : s.slice(0, max - 1) + '…';
}

function formatDuration(d: DeploymentSummary): string {
  const start = d.started_at ? new Date(d.started_at).getTime() : null;
  if (!start) return '—';
  const end = d.stopped_at ? new Date(d.stopped_at).getTime() : Date.now();
  const secs = Math.round((end - start) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.round(mins / 60);
  return `${hours}h`;
}

/* ---------- logs tab ---------- */

function LogsTab({
  deployments,
  activeId,
  onSelect,
}: {
  deployments: DeploymentSummary[];
  activeId: string | null;
  onSelect: (id: string) => void;
}) {
  if (deployments.length === 0) {
    return (
      <Card className="px-6 py-10 text-center text-sm text-[var(--color-muted)]">
        No deployments yet. Deploy to stream logs.
      </Card>
    );
  }
  return (
    <Stack gap={3}>
      <Card className="p-3">
        <label className="flex items-center gap-3 text-xs">
          <span className="text-[var(--color-muted)]">Deployment</span>
          <select
            value={activeId ?? ''}
            onChange={(e) => onSelect(e.target.value)}
            className="flex-1 rounded-md border border-[var(--color-border)] bg-transparent px-2 py-1.5 text-xs focus:border-[var(--color-accent)] focus:outline-none"
          >
            {deployments.map((d) => (
              <option key={d.id} value={d.id}>
                {d.status} · {truncateImage(d.image_ref)} ·{' '}
                {new Date(d.created_at).toLocaleString()}
              </option>
            ))}
          </select>
        </label>
      </Card>
      {activeId ? <LogViewer deploymentId={activeId} /> : null}
    </Stack>
  );
}

interface LogEntry {
  stream: 'stdout' | 'stderr';
  ts: string;
  text: string;
}

function LogViewer({ deploymentId }: { deploymentId: string }) {
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [connState, setConnState] = useState<'connecting' | 'open' | 'closed' | 'error'>(
    'connecting',
  );
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setLines([]);
    setConnState('connecting');
    const url = `/api/v1/deployments/${encodeURIComponent(deploymentId)}/logs`;
    const es = new EventSource(url, { withCredentials: true });

    es.onopen = () => setConnState('open');
    es.addEventListener('log', (e) => {
      try {
        const data = JSON.parse((e as MessageEvent).data) as LogEntry;
        setLines((prev) => {
          const next = prev.concat(data);
          return next.length > 500 ? next.slice(next.length - 500) : next;
        });
      } catch {
        // ignore malformed
      }
    });
    es.addEventListener('error', () => setConnState('error'));
    es.onerror = () => setConnState('error');

    return () => {
      es.close();
      setConnState('closed');
    };
  }, [deploymentId]);

  useEffect(() => {
    const el = containerRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const stateTone: SemanticStatus =
    connState === 'open' ? 'ok' : connState === 'error' ? 'error' : 'muted';

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-xs">
        <CopyableId value={deploymentId} display={deploymentId.slice(0, 8)} />
        <StatusPill
          status={stateTone}
          label={connState}
          pulse={connState === 'connecting'}
        />
      </div>
      <div
        ref={containerRef}
        className="max-h-[28rem] overflow-auto bg-black/30 p-3 font-mono text-xs"
      >
        {lines.length === 0 ? (
          <div className="text-[var(--color-muted)]">Waiting for logs…</div>
        ) : (
          lines.map((l, i) => (
            <div
              key={i}
              className={l.stream === 'stderr' ? 'text-red-300' : 'text-[var(--color-fg)]'}
            >
              <span className="mr-2 text-[var(--color-muted)]">
                {new Date(l.ts).toLocaleTimeString()}
              </span>
              {l.text}
            </div>
          ))
        )}
      </div>
    </Card>
  );
}

/* ---------- settings tab ---------- */

function SettingsTab({ service }: { service: ServiceSummary | undefined }) {
  if (!service) return null;
  return (
    <Stack gap={4}>
      <Card className="p-5">
        <div className="mb-3 text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
          Environment variables
        </div>
        {Object.keys(service.env_vars ?? {}).length === 0 ? (
          <p className="text-sm text-[var(--color-muted)]">No environment variables set.</p>
        ) : (
          <dl className="space-y-1.5 text-xs">
            {Object.entries(service.env_vars).map(([k, v]) => (
              <div key={k} className="flex items-center justify-between gap-4 font-mono">
                <dt>{k}</dt>
                <dd className="truncate text-[var(--color-muted)]">{v}</dd>
              </div>
            ))}
          </dl>
        )}
      </Card>
      <Card className="p-5">
        <div className="mb-2 text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
          Advanced
        </div>
        <p className="text-xs text-[var(--color-muted)]">
          Delete the service from the header (trash icon). Env var editing and volume mounts
          are coming soon.
        </p>
      </Card>
    </Stack>
  );
}

