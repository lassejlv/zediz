import { createFileRoute, useSearch } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Loader2, Trash2 } from 'lucide-react';
import { useRegisterActivity } from '@/components/app-shell';
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
import { serviceVolumeQuery } from '@/lib/volumes';
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
import { ServiceConsole } from '@/components/service-console';
import { ServiceMetricsTab } from '@/components/service-metrics';
import { ServiceSettingsTab } from '@/components/service-settings';
import { ServiceVolumeTab } from '@/components/service-volume';
import {
  buildsQuery,
  buildTone,
  isBuildCancellable,
  useCancelBuild,
} from '@/lib/builds';
import type { BuildSummary, DeploymentSummary, ServiceSummary } from '@/lib/types';

interface ServiceSearch {
  tab?: TopTab;
}

export const Route = createFileRoute(
  '/w/$workspaceSlug/projects/$projectSlug/$serviceSlug',
)({
  component: ServicePage,
  validateSearch: (raw: Record<string, unknown>): ServiceSearch => {
    const tab = raw.tab;
    const validTabs: TopTab[] = [
      'overview',
      'metrics',
      'deployments',
      'builds',
      'logs',
      'console',
      'networking',
      'storage',
      'settings',
    ];
    return {
      tab:
        typeof tab === 'string' && validTabs.includes(tab as TopTab)
          ? (tab as TopTab)
          : undefined,
    };
  },
});

type TopTab =
  | 'overview'
  | 'metrics'
  | 'deployments'
  | 'builds'
  | 'logs'
  | 'console'
  | 'networking'
  | 'storage'
  | 'settings';

function ServicePage() {
  const { workspaceSlug, projectSlug, serviceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const service = useQuery(serviceQuery(workspaceSlug, projectSlug, serviceSlug));
  const deployments = useQuery({
    ...serviceDeploymentsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 3000,
  });
  const domains = useQuery(domainsQuery(workspaceSlug, projectSlug, serviceSlug));
  const canDeploy = canWrite(workspace.data);
  const canDelete = canAdmin(workspace.data);
  const attachedVolume = useQuery({
    ...serviceVolumeQuery(workspaceSlug, projectSlug, serviceSlug),
    enabled: canDelete,
  });
  const deploy = useDeployService(workspaceSlug, projectSlug, serviceSlug);
  const deleteService = useDeleteService(workspaceSlug, projectSlug);
  const stop = useStopDeployment();
  const restart = useRestartDeployment();

  const search = useSearch({
    from: '/w/$workspaceSlug/projects/$projectSlug/$serviceSlug',
  });

  const [error, setError] = useState<string | null>(null);
  const [activeDeploymentId, setActiveDeploymentId] = useState<string | null>(null);
  const [tab, setTab] = useState<TopTab>(search.tab ?? 'overview');

  const latest = deployments.data?.[0];
  useEffect(() => {
    if (!activeDeploymentId && latest) {
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

  async function onDeleteService() {
    const volume = attachedVolume.data;
    const message = volume
      ? `Delete service ${serviceSlug} and attached volume ${volume.name}? The volume data will be permanently deleted.`
      : `Delete service ${serviceSlug}? This cannot be undone.`;

    if (!confirm(message)) return;

    setError(null);
    try {
      await deleteService.mutateAsync(serviceSlug);
      window.location.href = `/w/${workspaceSlug}/projects/${projectSlug}`;
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Delete failed');
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

  // Push in-flight deploys to the global activity strip.
  const inFlightStatus = useMemo(() => activityStatus(latest), [latest]);
  useRegisterActivity(
    svc && latest && inFlightStatus
      ? {
          key: `service:${svc.id}`,
          status: inFlightStatus,
          title: (
            <>
              <span className="font-mono">{svc.name}</span> · {latest.status}
            </>
          ),
          detail: truncateImage(latest.image_ref),
          to: `/w/${workspaceSlug}/projects/${projectSlug}/${serviceSlug}?tab=deployments`,
        }
      : null,
  );

  return (
    <Stack gap={6}>
      <PageHeader
        breadcrumbs={[
          { label: 'Projects', to: `/w/${workspaceSlug}/projects` },
          { label: projectSlug, to: `/w/${workspaceSlug}/projects/${projectSlug}` },
          { label: serviceSlug },
        ]}
        title={svc?.name ?? serviceSlug}
        subtitle={<ServiceSubtitle service={svc} />}
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
              <Button
                onClick={onDeploy}
                disabled={deploy.isPending || deleteService.isPending}
                title="⌘ ↵"
              >
                {deploy.isPending ? 'Deploying…' : deployLabel}
              </Button>
            ) : null}
            {canDelete ? (
              <Button
                variant="danger"
                onClick={onDeleteService}
                disabled={deleteService.isPending}
                title="Delete service"
                aria-label="Delete service"
              >
                {deleteService.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Trash2 className="h-3.5 w-3.5" />
                )}
              </Button>
            ) : null}
          </>
        }
      />

      {error ? <ErrorText>{error}</ErrorText> : null}

      {deleteService.isPending ? (
        <DeleteProgress volumeName={attachedVolume.data?.name ?? null} />
      ) : null}

      {svc ? (
        <SummaryStrip
          service={svc}
          latest={latest}
          primaryDomain={domains.data?.[0]?.hostname ?? null}
          serviceStatus={serviceStatus}
        />
      ) : null}

      <Tabs value={tab} onChange={setTab} showConsole={canDelete} />

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

      {tab === 'metrics' && svc ? (
        <ServiceMetricsTab
          service={svc}
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          serviceSlug={serviceSlug}
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

      {tab === 'builds' ? (
        <BuildsTab
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          serviceSlug={serviceSlug}
          canManage={canDeploy}
          onViewLogs={(deploymentId) => {
            setActiveDeploymentId(deploymentId);
            setTab('logs');
          }}
        />
      ) : null}

      {tab === 'logs' ? (
        <LogsTab
          deployments={deployments.data ?? []}
          activeId={activeDeploymentId}
          onSelect={(id) => setActiveDeploymentId(id)}
        />
      ) : null}

      {tab === 'console' && canDelete ? (
        <ConsoleTab
          deployments={deployments.data ?? []}
          activeId={activeDeploymentId}
          onSelect={(id) => setActiveDeploymentId(id)}
        />
      ) : null}

      {tab === 'networking' ? (
        <DomainsSection
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          serviceSlug={serviceSlug}
          canManage={canDeploy}
          defaultPort={svc?.ports?.[0]?.container_port ?? null}
        />
      ) : null}

      {tab === 'storage' && svc ? (
        <ServiceVolumeTab
          service={svc}
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          canManage={canDeploy}
        />
      ) : null}

      {tab === 'settings' && svc ? (
        <ServiceSettingsTab
          service={svc}
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          canManage={canDeploy}
        />
      ) : null}
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
    : d.status === 'building'
      ? 'building'
      : d.status === 'pulling'
        ? 'pulling image'
        : d.status === 'pending' || d.status === 'placing'
          ? 'pending'
          : d.status;
  return { tone, label, pulse };
}

function computeDeployLabel(latest: DeploymentSummary | undefined): string {
  if (!latest) return 'Deploy';
  if (['pending', 'building', 'placing', 'pulling', 'starting'].includes(latest.status))
    return 'Deploying…';
  if (latest.status === 'running') return 'Redeploy';
  return 'Deploy';
}

function ServiceSubtitle({ service }: { service: ServiceSummary | undefined }) {
  if (!service) return <>—</>;
  if (service.source === 'git' && service.git_repo) {
    return (
      <span className="font-mono text-xs">
        {service.git_repo}
        {service.git_branch ? <>@{service.git_branch}</> : null}
        {service.git_commit ? (
          <span className="text-[var(--color-muted)]"> · {service.git_commit.slice(0, 7)}</span>
        ) : null}
      </span>
    );
  }
  return service.image_ref ? (
    <span className="font-mono text-xs">{service.image_ref}</span>
  ) : (
    <>—</>
  );
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

function Tabs({
  value,
  onChange,
  showConsole,
}: {
  value: TopTab;
  onChange: (t: TopTab) => void;
  showConsole: boolean;
}) {
  const tabs: { id: TopTab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'metrics', label: 'Metrics' },
    { id: 'deployments', label: 'Deployments' },
    { id: 'builds', label: 'Builds' },
    { id: 'logs', label: 'Logs' },
    ...(showConsole ? [{ id: 'console' as const, label: 'Console' }] : []),
    { id: 'networking', label: 'Networking' },
    { id: 'storage', label: 'Storage' },
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
                  <CopyableId
                    value={service.image_ref}
                    display={truncateImage(service.image_ref)}
                  />
                ) : (
                  <span className="text-[var(--color-muted)]">—</span>
                )
              }
            />
            <Row
              label="Source"
              value={<span className="font-mono text-xs">{service.source}</span>}
            />
            <Row
              label="Private host"
              value={<CopyableId value={service.private_hostname} display={service.private_hostname} />}
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
        <td className="max-w-sm px-4 py-2 text-xs text-[var(--color-muted)]">
          {d.reason ? <span className="whitespace-pre-wrap break-words">{d.reason}</span> : '—'}
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

/** Shrink a potentially very long image ref so it fits in a card row.
 * Keeps the registry host + the digest/tag prefix so you can tell
 * workspace / version at a glance; the full value is still in the DOM
 * for CopyableId to copy. */
function truncateImage(ref: string): string {
  if (ref.length <= 40) return ref;

  const host = ref.split('/')[0];

  // <host>/<path>@<digest> — digest is the useful tail
  const at = ref.indexOf('@');
  if (at !== -1) {
    const digest = ref.slice(at + 1);
    return `${host}/…@${digest.slice(0, 12)}…`;
  }

  // <host>/<path>:<tag> — tag is the useful tail. lastIndexOf because
  // the port in <host> (if any) also uses ':'. Guard: a ':' that's
  // still inside the path (before the final '/') is not the tag.
  const colon = ref.lastIndexOf(':');
  const slash = ref.lastIndexOf('/');
  if (colon > slash && colon !== -1) {
    const tag = ref.slice(colon + 1);
    const shownTag = tag.length > 24 ? `${tag.slice(0, 24)}…` : tag;
    return `${host}/…:${shownTag}`;
  }

  return ref.slice(0, 37) + '…';
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

function activityStatus(
  d: DeploymentSummary | undefined,
): 'building' | 'deploying' | 'pending' | 'errored' | null {
  if (!d) return null;
  if (d.status === 'building') return 'building';
  if (d.status === 'pending') return 'pending';
  if (d.status === 'placing' || d.status === 'pulling' || d.status === 'starting')
    return 'deploying';
  if (d.status === 'errored' || d.status === 'failing') return 'errored';
  return null;
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

/* ---------- console tab ---------- */

function ConsoleTab({
  deployments,
  activeId,
  onSelect,
}: {
  deployments: DeploymentSummary[];
  activeId: string | null;
  onSelect: (id: string) => void;
}) {
  const running = deployments.filter((d) => d.status === 'running');
  if (running.length === 0) {
    return (
      <Card className="px-6 py-10 text-center text-sm text-[var(--color-muted)]">
        No running deployments. Console requires a running container.
      </Card>
    );
  }
  const target =
    activeId && running.some((d) => d.id === activeId) ? activeId : running[0].id;
  return (
    <Stack gap={3}>
      <Card className="p-3">
        <label className="flex items-center gap-3 text-xs">
          <span className="text-[var(--color-muted)]">Deployment</span>
          <select
            value={target}
            onChange={(e) => onSelect(e.target.value)}
            className="flex-1 rounded-md border border-[var(--color-border)] bg-transparent px-2 py-1.5 text-xs focus:border-[var(--color-accent)] focus:outline-none"
          >
            {running.map((d) => (
              <option key={d.id} value={d.id}>
                {d.status} · {truncateImage(d.image_ref)} ·{' '}
                {new Date(d.created_at).toLocaleString()}
              </option>
            ))}
          </select>
        </label>
      </Card>
      <ServiceConsole key={target} deploymentId={target} />
    </Stack>
  );
}

function DeleteProgress({ volumeName }: { volumeName: string | null }) {
  return (
    <div className="rounded-lg border border-red-500/30 bg-red-500/[0.08] px-4 py-3">
      <div className="flex items-start gap-3">
        <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-red-500/30 bg-red-500/10">
          <Loader2 className="h-3.5 w-3.5 animate-spin text-red-300" />
        </div>
        <div className="min-w-0">
          <div className="text-sm font-medium text-red-100">
            {volumeName ? 'Deleting service and attached volume' : 'Deleting service'}
          </div>
          <p className="mt-1 text-xs leading-5 text-red-200/80">
            {volumeName
              ? `Removing ${volumeName}, detaching it from Hetzner if needed, then deleting the service. Keep this page open until the project view returns.`
              : 'Stopping related work, removing service records, then returning to the project view.'}
          </p>
        </div>
      </div>
    </div>
  );
}

interface LogEntry {
  stream: 'stdout' | 'stderr';
  ts: string;
  text: string;
}

type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal' | 'unknown';

interface ParsedLog {
  level: LogLevel;
  message: string;
  json: Record<string, unknown> | null;
}

const LEVEL_RE =
  /\b(TRACE|DEBUG|INFO(?:RMATION)?|NOTICE|WARN(?:ING)?|ERR(?:OR)?|FATAL|CRIT(?:ICAL)?|EMERG(?:ENCY)?|PANIC)\b/i;

function normalizeLevel(raw: string): LogLevel {
  const v = raw.toUpperCase();
  if (v.startsWith('TRACE')) return 'trace';
  if (v.startsWith('DEBUG')) return 'debug';
  if (v.startsWith('INFO') || v === 'NOTICE') return 'info';
  if (v.startsWith('WARN')) return 'warn';
  if (v.startsWith('ERR')) return 'error';
  if (v.startsWith('FATAL') || v.startsWith('CRIT') || v.startsWith('EMERG') || v === 'PANIC')
    return 'fatal';
  return 'unknown';
}

function parseLog(entry: LogEntry): ParsedLog {
  const text = entry.text;
  const trimmed = text.trim();

  if (trimmed.startsWith('{') && trimmed.endsWith('}')) {
    try {
      const obj = JSON.parse(trimmed) as Record<string, unknown>;
      const levelRaw =
        pickString(obj, ['level', 'lvl', 'severity', '@l', 'levelname']) ?? '';
      const msg =
        pickString(obj, ['msg', 'message', 'text', '@m']) ?? trimmed;
      const lvl = levelRaw ? normalizeLevel(levelRaw) : detectLevel(msg);
      return { level: lvl, message: msg, json: obj };
    } catch {
      // fall through
    }
  }

  return {
    level: entry.stream === 'stderr' ? detectLevel(text, 'error') : detectLevel(text),
    message: text,
    json: null,
  };
}

function pickString(
  obj: Record<string, unknown>,
  keys: string[],
): string | null {
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === 'string' && v.length) return v;
    if (typeof v === 'number') return String(v);
  }
  return null;
}

function detectLevel(text: string, fallback: LogLevel = 'unknown'): LogLevel {
  const m = LEVEL_RE.exec(text);
  if (m) return normalizeLevel(m[1]);
  return fallback;
}

const LEVEL_META: Record<
  LogLevel,
  { label: string; pill: string; text: string }
> = {
  trace: {
    label: 'TRC',
    pill: 'border-[var(--color-border)] text-[var(--color-subtle)]',
    text: 'text-[var(--color-subtle)]',
  },
  debug: {
    label: 'DBG',
    pill: 'border-[var(--color-border)] text-[var(--color-muted)]',
    text: 'text-[var(--color-muted)]',
  },
  info: {
    label: 'INF',
    pill: 'border-emerald-500/30 text-emerald-400',
    text: 'text-[var(--color-fg)]',
  },
  warn: {
    label: 'WRN',
    pill: 'border-amber-500/30 text-amber-300',
    text: 'text-amber-200',
  },
  error: {
    label: 'ERR',
    pill: 'border-red-500/40 text-red-300',
    text: 'text-red-200',
  },
  fatal: {
    label: 'FTL',
    pill: 'border-red-500/60 bg-red-500/10 text-red-200',
    text: 'text-red-100 font-semibold',
  },
  unknown: {
    label: '···',
    pill: 'border-[var(--color-border)] text-[var(--color-muted)]',
    text: 'text-[var(--color-fg)]',
  },
};

const LEVEL_ORDER: LogLevel[] = [
  'trace',
  'debug',
  'info',
  'warn',
  'error',
  'fatal',
];

function LogViewer({ deploymentId }: { deploymentId: string }) {
  const [lines, setLines] = useState<LogEntry[]>([]);
  const [connState, setConnState] = useState<'connecting' | 'open' | 'closed' | 'error'>(
    'connecting',
  );
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [filterLevels, setFilterLevels] = useState<Set<LogLevel>>(new Set());
  const [search, setSearch] = useState('');
  const [autoscroll, setAutoscroll] = useState(true);
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setLines([]);
    setSelectedIdx(null);
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

  const parsed = useMemo(() => lines.map(parseLog), [lines]);

  const counts = useMemo(() => {
    const c: Record<LogLevel, number> = {
      trace: 0,
      debug: 0,
      info: 0,
      warn: 0,
      error: 0,
      fatal: 0,
      unknown: 0,
    };
    for (const p of parsed) c[p.level]++;
    return c;
  }, [parsed]);

  const visibleIndices = useMemo(() => {
    const q = search.trim().toLowerCase();
    const out: number[] = [];
    for (let i = 0; i < lines.length; i++) {
      if (filterLevels.size && !filterLevels.has(parsed[i].level)) continue;
      if (q && !lines[i].text.toLowerCase().includes(q)) continue;
      out.push(i);
    }
    return out;
  }, [lines, parsed, filterLevels, search]);

  useEffect(() => {
    if (!autoscroll) return;
    const el = containerRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [visibleIndices, autoscroll]);

  function toggleLevel(l: LogLevel) {
    setFilterLevels((prev) => {
      const next = new Set(prev);
      if (next.has(l)) next.delete(l);
      else next.add(l);
      return next;
    });
  }

  const stateTone: SemanticStatus =
    connState === 'open' ? 'ok' : connState === 'error' ? 'error' : 'muted';

  const selected =
    selectedIdx != null && selectedIdx < lines.length
      ? { entry: lines[selectedIdx], parsed: parsed[selectedIdx] }
      : null;

  return (
    <Card className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--color-border)] px-4 py-2 text-xs">
        <div className="flex items-center gap-3">
          <CopyableId value={deploymentId} display={deploymentId.slice(0, 8)} />
          <StatusPill
            status={stateTone}
            label={connState}
            pulse={connState === 'connecting'}
          />
          <span className="text-[var(--color-muted)]">{lines.length} lines</span>
        </div>
        <div className="flex items-center gap-2">
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="filter…"
            className="h-7 w-40 rounded-md border border-[var(--color-border)] bg-transparent px-2 text-xs focus:border-[var(--color-accent)] focus:outline-none"
          />
          <button
            type="button"
            onClick={() => setAutoscroll((v) => !v)}
            className={[
              'rounded-md border px-2 py-1 transition-colors',
              autoscroll
                ? 'border-[var(--color-accent)] text-[var(--color-fg)]'
                : 'border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]',
            ].join(' ')}
            title="Toggle autoscroll"
          >
            ↓ follow
          </button>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1.5 border-b border-[var(--color-border)] px-4 py-2 text-[10px]">
        {LEVEL_ORDER.map((l) => {
          const meta = LEVEL_META[l];
          const active = filterLevels.has(l);
          return (
            <button
              key={l}
              type="button"
              onClick={() => toggleLevel(l)}
              className={[
                'rounded-md border px-1.5 py-0.5 font-mono uppercase tracking-wider transition-colors',
                meta.pill,
                active
                  ? 'bg-[var(--color-fg)]/5 ring-1 ring-inset ring-[var(--color-border-strong)]'
                  : 'opacity-70 hover:opacity-100',
              ].join(' ')}
              title={`${l} · ${counts[l]}`}
            >
              {meta.label} {counts[l]}
            </button>
          );
        })}
        {filterLevels.size > 0 || search ? (
          <button
            type="button"
            onClick={() => {
              setFilterLevels(new Set());
              setSearch('');
            }}
            className="ml-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            clear
          </button>
        ) : null}
        <span className="ml-auto text-[var(--color-muted)]">
          {visibleIndices.length} shown
        </span>
      </div>

      <div
        className={[
          'grid bg-black/30',
          selected ? 'grid-cols-[1fr_360px]' : 'grid-cols-1',
        ].join(' ')}
      >
        <div
          ref={containerRef}
          className="max-h-[32rem] overflow-auto p-2 font-mono text-xs"
        >
          {visibleIndices.length === 0 ? (
            <div className="px-2 py-3 text-[var(--color-muted)]">
              {lines.length === 0 ? 'Waiting for logs…' : 'Nothing matches the filters.'}
            </div>
          ) : (
            visibleIndices.map((i) => {
              const entry = lines[i];
              const p = parsed[i];
              const meta = LEVEL_META[p.level];
              const isSelected = selectedIdx === i;
              return (
                <button
                  key={i}
                  type="button"
                  onClick={() => setSelectedIdx(isSelected ? null : i)}
                  className={[
                    'flex w-full items-start gap-2 rounded-sm px-1.5 py-0.5 text-left transition-colors',
                    isSelected
                      ? 'bg-white/[0.07]'
                      : 'hover:bg-white/[0.03]',
                  ].join(' ')}
                >
                  <span className="shrink-0 text-[var(--color-subtle)]">
                    {new Date(entry.ts).toLocaleTimeString()}
                  </span>
                  <span
                    className={[
                      'shrink-0 rounded border px-1 text-[10px] uppercase tracking-wider',
                      meta.pill,
                    ].join(' ')}
                  >
                    {meta.label}
                  </span>
                  {entry.stream === 'stderr' ? (
                    <span className="shrink-0 text-[10px] uppercase text-red-400/60">
                      err
                    </span>
                  ) : null}
                  <span className={['min-w-0 flex-1 break-words', meta.text].join(' ')}>
                    {p.message}
                  </span>
                </button>
              );
            })
          )}
        </div>

        {selected ? (
          <LogDetailPanel
            entry={selected.entry}
            parsed={selected.parsed}
            onClose={() => setSelectedIdx(null)}
          />
        ) : null}
      </div>
    </Card>
  );
}

function LogDetailPanel({
  entry,
  parsed,
  onClose,
}: {
  entry: LogEntry;
  parsed: ParsedLog;
  onClose: () => void;
}) {
  const meta = LEVEL_META[parsed.level];
  const pretty = parsed.json ? JSON.stringify(parsed.json, null, 2) : null;

  async function copy(s: string) {
    try {
      await navigator.clipboard.writeText(s);
    } catch {
      // ignore
    }
  }

  return (
    <aside className="flex max-h-[32rem] flex-col border-l border-[var(--color-border)] bg-[var(--color-surface-elevated)] font-mono text-xs">
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-2">
        <span className="flex items-center gap-2">
          <span
            className={[
              'rounded border px-1 text-[10px] uppercase tracking-wider',
              meta.pill,
            ].join(' ')}
          >
            {meta.label}
          </span>
          <span className="text-[var(--color-muted)]">log entry</span>
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close"
          className="text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          ✕
        </button>
      </div>

      <div className="flex-1 overflow-y-auto">
        <Field2 label="Timestamp" value={new Date(entry.ts).toLocaleString()} />
        <Field2 label="Stream" value={entry.stream} />
        <Field2 label="Level" value={parsed.level} />
        <Field2 label="Message" value={parsed.message} multiline />

        {pretty ? (
          <div className="border-t border-[var(--color-border)] px-3 py-2">
            <div className="mb-1 flex items-center justify-between text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
              <span>JSON</span>
              <button
                type="button"
                onClick={() => copy(pretty)}
                className="text-[var(--color-muted)] hover:text-[var(--color-fg)]"
              >
                copy
              </button>
            </div>
            <pre className="overflow-x-auto whitespace-pre rounded bg-black/40 p-2 text-[11px] leading-relaxed">
              {pretty}
            </pre>
          </div>
        ) : null}

        <div className="border-t border-[var(--color-border)] px-3 py-2">
          <div className="mb-1 flex items-center justify-between text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            <span>Raw</span>
            <button
              type="button"
              onClick={() => copy(entry.text)}
              className="text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              copy
            </button>
          </div>
          <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded bg-black/40 p-2 text-[11px] leading-relaxed">
            {entry.text}
          </pre>
        </div>
      </div>
    </aside>
  );
}

function Field2({
  label,
  value,
  multiline,
}: {
  label: string;
  value: string;
  multiline?: boolean;
}) {
  return (
    <div className="border-b border-[var(--color-border)] px-3 py-2">
      <div className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
        {label}
      </div>
      <div
        className={[
          'mt-0.5 text-[11px] text-[var(--color-fg)]',
          multiline ? 'whitespace-pre-wrap break-words' : 'truncate',
        ].join(' ')}
      >
        {value}
      </div>
    </div>
  );
}

/* ---------- builds tab ---------- */

function BuildsTab({
  workspaceSlug,
  projectSlug,
  serviceSlug,
  canManage,
  onViewLogs,
}: {
  workspaceSlug: string;
  projectSlug: string;
  serviceSlug: string;
  canManage: boolean;
  onViewLogs: (deploymentId: string) => void;
}) {
  const builds = useQuery({
    ...buildsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 3000,
  });
  const cancel = useCancelBuild(workspaceSlug, projectSlug, serviceSlug);

  if (!builds.data || builds.data.length === 0) {
    return (
      <Card className="px-6 py-10 text-center text-sm text-[var(--color-muted)]">
        No builds yet. Set the service source to Git and hit Deploy to kick one off.
      </Card>
    );
  }

  return (
    <Card className="overflow-hidden">
      <table className="w-full text-sm">
        <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          <tr>
            <th className="px-4 py-2.5 font-medium">Status</th>
            <th className="px-4 py-2.5 font-medium">Trigger</th>
            <th className="px-4 py-2.5 font-medium">Commit</th>
            <th className="px-4 py-2.5 font-medium">Image</th>
            <th className="px-4 py-2.5 font-medium">Started</th>
            <th className="px-4 py-2.5 font-medium">Duration</th>
            <th className="px-4 py-2.5 font-medium">Reason</th>
            <th className="px-4 py-2.5" />
          </tr>
        </thead>
        <tbody>
          {builds.data.map((b) => (
            <BuildRow
              key={b.id}
              b={b}
              canManage={canManage}
              onViewLogs={onViewLogs}
              onCancel={() => cancel.mutate(b.id)}
              cancelling={cancel.isPending && cancel.variables === b.id}
            />
          ))}
        </tbody>
      </table>
    </Card>
  );
}

function BuildRow({
  b,
  canManage,
  onViewLogs,
  onCancel,
  cancelling,
}: {
  b: BuildSummary;
  canManage: boolean;
  onViewLogs: (deploymentId: string) => void;
  onCancel: () => void;
  cancelling: boolean;
}) {
  const tone = buildTone(b.status);
  const cancellable = canManage && isBuildCancellable(b.status);
  return (
    <tr className="border-t border-[var(--color-border)]">
      <td className="px-4 py-2">
        <StatusPill status={tone.tone} label={b.status} pulse={tone.pulse} />
      </td>
      <td className="px-4 py-2 text-xs text-[var(--color-muted)]">
        {b.trigger_kind === 'github_push' ? (
          <div>
            <span className="text-[var(--color-fg)]">GitHub push</span>
            {b.git_ref ? (
              <div className="mt-0.5 font-mono text-[11px]">
                {b.git_ref.replace('refs/heads/', '')}
              </div>
            ) : null}
          </div>
        ) : (
          'Manual'
        )}
      </td>
      <td className="px-4 py-2 font-mono text-xs">
        {b.git_commit || b.git_sha ? (b.git_commit ?? b.git_sha)!.slice(0, 7) : '—'}
      </td>
      <td className="px-4 py-2">
        {b.image_digest ? (
          <CopyableId value={b.image_digest} display={shortDigest(b.image_digest)} />
        ) : b.image_tag ? (
          <span className="font-mono text-xs text-[var(--color-muted)]">
            {truncateImage(b.image_tag)}
          </span>
        ) : (
          <span className="text-xs text-[var(--color-muted)]">—</span>
        )}
      </td>
      <td className="px-4 py-2 text-xs text-[var(--color-muted)]">
        <RelativeTime date={b.created_at} />
      </td>
      <td className="px-4 py-2 font-mono text-xs text-[var(--color-muted)]">
        {formatBuildDuration(b)}
      </td>
      <td className="max-w-sm px-4 py-2 text-xs text-[var(--color-muted)]">
        {b.reason ? <span className="whitespace-pre-wrap break-words">{b.reason}</span> : '—'}
      </td>
      <td className="px-4 py-2 text-right">
        <div className="flex items-center justify-end gap-2">
          {cancellable ? (
            <Button
              variant="danger"
              onClick={onCancel}
              disabled={cancelling}
            >
              {cancelling ? 'Cancelling…' : 'Cancel'}
            </Button>
          ) : null}
          {b.deployment_id ? (
            <Button variant="secondary" onClick={() => onViewLogs(b.deployment_id!)}>
              View logs
            </Button>
          ) : null}
        </div>
      </td>
    </tr>
  );
}

function formatBuildDuration(b: BuildSummary): string {
  const start = b.started_at ? new Date(b.started_at).getTime() : null;
  if (!start) return '—';
  const end = b.finished_at ? new Date(b.finished_at).getTime() : Date.now();
  const secs = Math.round((end - start) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m${secs % 60}s`;
  return `${Math.round(mins / 60)}h`;
}

function shortDigest(digest: string): string {
  const at = digest.indexOf(':');
  return at === -1 ? digest.slice(0, 12) : digest.slice(at + 1, at + 13);
}

/* ---------- settings tab ---------- */
