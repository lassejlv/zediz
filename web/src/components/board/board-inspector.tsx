import { Link } from '@tanstack/react-router';
import { X, ExternalLink } from 'lucide-react';
import { useEffect } from 'react';
import type { ServiceNodeState } from './service-node';
import {
  Button,
  CopyableId,
  RelativeTime,
  StatusPill,
  type SemanticStatus,
} from '@/components/ui';

interface Props {
  state: ServiceNodeState;
  workspaceSlug: string;
  projectSlug: string;
  canDeploy: boolean;
  onClose: () => void;
  onDeploy: (serviceSlug: string) => void;
  deployPending?: boolean;
}

export function BoardInspector({
  state,
  workspaceSlug,
  projectSlug,
  canDeploy,
  onClose,
  onDeploy,
  deployPending,
}: Props) {
  const { service, latestDeployment, primaryDomain } = state;

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose();
    }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const status = deploymentStatus(latestDeployment);
  const deployLabel = latestDeployment?.status === 'running' ? 'Redeploy' : 'Deploy';
  const deploying = latestDeployment
    ? ['pending', 'placing', 'pulling', 'starting'].includes(latestDeployment.status)
    : false;

  return (
    <aside className="flex h-full w-[360px] shrink-0 flex-col border-l border-[var(--color-border)] bg-[var(--color-surface)]">
      <header className="flex items-start justify-between gap-2 border-b border-[var(--color-border)] px-4 py-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">{service.name}</div>
          <div className="mt-0.5 truncate font-mono text-[11px] text-[var(--color-muted)]">
            {service.slug}
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close"
          className="inline-flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-black/5 hover:text-[var(--color-fg)] dark:hover:bg-white/5"
        >
          <X className="h-4 w-4" />
        </button>
      </header>

      <div className="flex-1 overflow-y-auto">
        <Section label="Status">
          <StatusPill status={status.tone} label={status.label} pulse={status.pulse} />
          {latestDeployment ? (
            <div className="mt-1 text-[11px] text-[var(--color-muted)]">
              Last deploy <RelativeTime date={latestDeployment.created_at} />
            </div>
          ) : null}
        </Section>

        <Section label="Image">
          {service.image_ref ? (
            <CopyableId value={service.image_ref} />
          ) : (
            <span className="text-xs text-[var(--color-muted)]">—</span>
          )}
        </Section>

        <Section label="Resources">
          <dl className="space-y-1 text-xs">
            <Row label="CPU" value={`${service.resources.cpu_millis}m`} />
            <Row label="Memory" value={`${service.resources.memory_mb} MB`} />
            <Row label="Disk" value={`${service.resources.disk_mb} MB`} />
            <Row label="Replicas" value={String(service.replicas)} />
            <Row label="Restart" value={service.restart_policy} />
          </dl>
        </Section>

        <Section label="Routing">
          <dl className="space-y-1 text-xs">
            <Row
              label="Domain"
              value={
                primaryDomain ? (
                  <span className="font-mono">{primaryDomain}</span>
                ) : (
                  <span className="text-[var(--color-muted)]">—</span>
                )
              }
            />
            <Row
              label="Port"
              value={
                service.ports[0]?.container_port ? (
                  <span className="font-mono">:{service.ports[0].container_port}</span>
                ) : (
                  <span className="text-[var(--color-muted)]">—</span>
                )
              }
            />
          </dl>
        </Section>
      </div>

      <footer className="flex flex-col gap-2 border-t border-[var(--color-border)] px-4 py-3">
        {canDeploy ? (
          <Button onClick={() => onDeploy(service.slug)} disabled={deployPending || deploying}>
            {deployPending ? 'Deploying…' : deploying ? status.label : deployLabel}
          </Button>
        ) : null}
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug/$serviceSlug"
          params={{ workspaceSlug, projectSlug, serviceSlug: service.slug }}
          className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md border border-[var(--color-border)] px-3 text-sm text-[var(--color-fg)] hover:border-[var(--color-border-strong)] hover:bg-black/5 dark:hover:bg-white/5"
        >
          Open service <ExternalLink className="h-3.5 w-3.5" />
        </Link>
      </footer>
    </aside>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="border-b border-[var(--color-border)] px-4 py-3">
      <div className="mb-2 text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
        {label}
      </div>
      {children}
    </div>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <dt className="text-[var(--color-muted)]">{label}</dt>
      <dd className="font-mono">{value}</dd>
    </div>
  );
}

function deploymentStatus(
  d: { status: string } | undefined,
): { tone: SemanticStatus; label: string; pulse: boolean } {
  if (!d) return { tone: 'muted', label: 'never deployed', pulse: false };
  switch (d.status) {
    case 'running':
      return { tone: 'ok', label: 'running', pulse: false };
    case 'pending':
    case 'placing':
      return { tone: 'info', label: 'pending', pulse: true };
    case 'pulling':
      return { tone: 'warn', label: 'pulling image', pulse: true };
    case 'starting':
      return { tone: 'warn', label: 'starting', pulse: true };
    case 'failing':
      return { tone: 'warn', label: 'failing', pulse: true };
    case 'errored':
      return { tone: 'error', label: 'errored', pulse: false };
    case 'stopped':
      return { tone: 'muted', label: 'stopped', pulse: false };
    default:
      return { tone: 'muted', label: d.status, pulse: false };
  }
}
