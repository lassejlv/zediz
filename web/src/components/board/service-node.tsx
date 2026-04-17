import type { ServiceSummary, DeploymentSummary } from '@/lib/types';
import { RelativeTime, StatusPill, type SemanticStatus } from '@/components/ui';

export interface ServiceNodeState {
  service: ServiceSummary;
  latestDeployment?: DeploymentSummary;
  primaryDomain?: string | null;
}

function deploymentStatus(d: DeploymentSummary | undefined): {
  tone: SemanticStatus;
  label: string;
  pulse: boolean;
  borderClass: string;
} {
  if (!d)
    return { tone: 'muted', label: 'never deployed', pulse: false, borderClass: '' };
  switch (d.status) {
    case 'running':
      return {
        tone: 'ok',
        label: 'running',
        pulse: false,
        borderClass: 'border-l-emerald-500/70',
      };
    case 'pending':
    case 'placing':
      return {
        tone: 'info',
        label: 'pending',
        pulse: true,
        borderClass: 'border-l-indigo-500',
      };
    case 'pulling':
      return {
        tone: 'warn',
        label: 'pulling image',
        pulse: true,
        borderClass: 'border-l-amber-500',
      };
    case 'starting':
      return {
        tone: 'warn',
        label: 'starting',
        pulse: true,
        borderClass: 'border-l-amber-500',
      };
    case 'failing':
      return {
        tone: 'warn',
        label: 'failing',
        pulse: true,
        borderClass: 'border-l-amber-500',
      };
    case 'errored':
      return { tone: 'error', label: 'errored', pulse: false, borderClass: 'border-l-red-500' };
    case 'stopped':
      return { tone: 'muted', label: 'stopped', pulse: false, borderClass: '' };
  }
}

interface Props {
  state: ServiceNodeState;
  selected?: boolean;
}

export function ServiceNode({ state, selected }: Props) {
  const { service, latestDeployment, primaryDomain } = state;
  const status = deploymentStatus(latestDeployment);
  const primaryPort = service.ports[0]?.container_port;

  return (
    <div
      className={[
        'group relative flex h-full w-full cursor-grab flex-col justify-between rounded-lg border bg-[var(--color-surface)] px-4 py-3 text-left transition-colors active:cursor-grabbing',
        selected
          ? 'border-[var(--color-accent)] ring-1 ring-[var(--color-accent)]/30'
          : 'border-[var(--color-border)] hover:border-[var(--color-border-strong)]',
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
    </div>
  );
}
