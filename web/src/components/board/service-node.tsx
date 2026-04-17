import { Link } from '@tanstack/react-router';
import type { ServiceSummary, DeploymentSummary } from '@/lib/types';
import { RelativeTime, StatusPill } from '@/components/ui';
import { deploymentTone } from '@/lib/deployments';

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

  return (
    <Link
      to="/w/$workspaceSlug/projects/$projectSlug/$serviceSlug"
      params={{ workspaceSlug, projectSlug, serviceSlug: service.slug }}
      className={[
        'group relative flex h-[120px] flex-col justify-between rounded-lg border bg-[var(--color-surface)] px-4 py-3 text-left transition-colors',
        'border-[var(--color-border)] hover:border-[var(--color-border-strong)]',
        'border-l-4',
        status.borderClass || 'border-l-[var(--color-border)]',
      ].join(' ')}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium">{service.name}</div>
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
    </Link>
  );
}
