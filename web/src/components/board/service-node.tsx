import { Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { Activity, Rocket, ScrollText } from 'lucide-react';
import type { ServiceSummary, DeploymentSummary } from '@/lib/types';
import { RelativeTime, Sparkline, StatusPill } from '@/components/ui';
import { deploymentMetricsQuery, deploymentTone } from '@/lib/deployments';

export interface ServiceNodeState {
  service: ServiceSummary;
  latestDeployment?: DeploymentSummary;
  primaryDomain?: string | null;
}

const BORDER_BY_TONE: Record<string, string> = {
  ok: 'border-l-emerald-500/70',
  info: 'border-l-indigo-500',
  warn: 'border-l-amber-500',
  error: 'border-l-red-500',
  muted: '',
  accent: 'border-l-emerald-500/70',
};

function deploymentStatus(d: DeploymentSummary | undefined): {
  tone: ReturnType<typeof deploymentTone>['tone'];
  label: string;
  pulse: boolean;
  borderClass: string;
} {
  const { tone, pulse } = deploymentTone(d?.status);
  const label = !d
    ? 'never deployed'
    : d.status === 'pulling'
      ? 'pulling image'
      : d.status === 'pending' || d.status === 'placing'
        ? 'pending'
        : d.status;
  return { tone, pulse, label, borderClass: BORDER_BY_TONE[tone] ?? '' };
}

interface Props {
  state: ServiceNodeState;
  workspaceSlug: string;
  projectSlug: string;
}

export function ServiceNode({ state, workspaceSlug, projectSlug }: Props) {
  const { service, latestDeployment, primaryDomain } = state;
  const status = deploymentStatus(latestDeployment);
  const primaryPort = service.ports[0]?.container_port;

  const metricsEnabled = latestDeployment?.status === 'running';
  const metrics = useQuery({
    ...deploymentMetricsQuery(latestDeployment?.id ?? '', 15),
    enabled: metricsEnabled && !!latestDeployment?.id,
    refetchInterval: metricsEnabled ? 30_000 : false,
    staleTime: 20_000,
  });

  const samples = metrics.data ?? [];
  const cpuSeries = samples.map((s) => s.cpu_percent);
  const memSeries = samples.map((s) => s.memory_bytes);
  const latestCpu = cpuSeries.at(-1);
  const latestMem = samples.at(-1)?.memory_bytes;

  return (
    <Link
      to="/w/$workspaceSlug/projects/$projectSlug/$serviceSlug"
      params={{ workspaceSlug, projectSlug, serviceSlug: service.slug }}
      className={[
        'group relative flex flex-col gap-2 overflow-hidden rounded-lg border bg-[var(--color-surface)] px-4 py-3 text-left transition-colors',
        'border-[var(--color-border)] hover:border-[var(--color-border-strong)]',
        'border-l-4',
        status.borderClass || 'border-l-[var(--color-border)]',
      ].join(' ')}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-[15px] font-semibold tracking-tight">
            {service.name}
          </div>
          <div className="mt-0.5 truncate font-mono text-[11px] text-[var(--color-muted)]">
            {service.image_ref ?? '—'}
          </div>
        </div>
        <span
          className={[
            'shrink-0 rounded-sm px-1.5 py-0.5 font-mono text-[10px]',
            service.replicas > 0
              ? 'bg-black/5 text-[var(--color-fg)] dark:bg-white/10'
              : 'bg-red-500/10 text-red-400',
          ].join(' ')}
          title={`${service.replicas} replica${service.replicas === 1 ? '' : 's'}`}
        >
          ↑ {service.replicas}
        </span>
      </div>

      <div className="flex items-center justify-between gap-2 text-[11px]">
        <span className="min-w-0 truncate text-[var(--color-muted)]">
          {primaryDomain ? (
            <span className="font-mono text-[var(--color-fg)]">{primaryDomain}</span>
          ) : (
            'no domain'
          )}
        </span>
        {primaryPort ? (
          <span className="shrink-0 font-mono text-[var(--color-muted)]">:{primaryPort}</span>
        ) : null}
      </div>

      {metricsEnabled ? (
        <div className="flex items-center gap-3 text-[10px] text-[var(--color-muted)]">
          <span className="flex items-center gap-1.5">
            <span className="font-mono uppercase tracking-wider">cpu</span>
            <Sparkline
              values={cpuSeries}
              width={48}
              height={14}
              className="text-emerald-500"
            />
            <span className="font-mono text-[var(--color-fg)]">
              {latestCpu != null ? `${latestCpu.toFixed(0)}%` : '—'}
            </span>
          </span>
          <span className="flex items-center gap-1.5">
            <span className="font-mono uppercase tracking-wider">mem</span>
            <Sparkline
              values={memSeries}
              width={48}
              height={14}
              className="text-indigo-500"
            />
            <span className="font-mono text-[var(--color-fg)]">
              {latestMem != null ? formatBytes(latestMem) : '—'}
            </span>
          </span>
        </div>
      ) : (
        <div className="flex h-[14px] items-center text-[10px] text-[var(--color-subtle)]">
          <Activity className="mr-1 h-3 w-3" />
          {latestDeployment ? 'not running' : 'never deployed'}
        </div>
      )}

      <div className="flex items-center justify-between gap-2">
        <StatusPill status={status.tone} label={status.label} pulse={status.pulse} />
        <span className="font-mono text-[10px] text-[var(--color-muted)]">
          {latestDeployment ? (
            <RelativeTime date={latestDeployment.created_at} />
          ) : (
            'never'
          )}
        </span>
      </div>

      <div className="pointer-events-none absolute inset-x-0 bottom-0 translate-y-full bg-gradient-to-t from-[var(--color-surface-elevated)] to-transparent px-4 pb-2 pt-3 opacity-0 transition-all duration-150 group-hover:pointer-events-auto group-hover:translate-y-0 group-hover:opacity-100">
        <div className="flex items-center justify-end gap-1.5 text-[11px]">
          <QuickAction
            label="Deploy"
            icon={Rocket}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              // navigate the parent Link by changing location to the service detail
              window.location.assign(
                `/w/${workspaceSlug}/projects/${projectSlug}/${service.slug}`,
              );
            }}
          />
          <QuickAction
            label="Logs"
            icon={ScrollText}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              window.location.assign(
                `/w/${workspaceSlug}/projects/${projectSlug}/${service.slug}?tab=activity`,
              );
            }}
          />
        </div>
      </div>
    </Link>
  );
}

function QuickAction({
  label,
  icon: Icon,
  onClick,
}: {
  label: string;
  icon: typeof Rocket;
  onClick: (e: React.MouseEvent) => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-2 py-1 text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
    >
      <Icon className="h-3 w-3" />
      {label}
    </button>
  );
}

function formatBytes(b: number): string {
  if (b < 1024) return `${b}B`;
  const k = b / 1024;
  if (k < 1024) return `${k.toFixed(0)}K`;
  const m = k / 1024;
  if (m < 1024) return `${m.toFixed(0)}M`;
  return `${(m / 1024).toFixed(1)}G`;
}
