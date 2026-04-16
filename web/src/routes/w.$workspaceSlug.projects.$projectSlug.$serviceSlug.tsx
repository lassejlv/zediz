import { createFileRoute, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import {
  serviceQuery,
  serviceDeploymentsQuery,
  useDeployService,
} from '@/lib/services';
import { useRestartDeployment, useStopDeployment } from '@/lib/deployments';
import { workspaceQuery } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText } from '@/components/ui';
import { DomainsSection } from '@/components/domains-section';
import type { DeploymentStatus, DeploymentSummary } from '@/lib/types';

export const Route = createFileRoute(
  '/w/$workspaceSlug/projects/$projectSlug/$serviceSlug',
)({
  component: ServicePage,
});

const STATUS_COLOR: Record<DeploymentStatus, string> = {
  pending: 'text-[var(--color-muted)]',
  placing: 'text-[var(--color-muted)]',
  pulling: 'text-yellow-400',
  starting: 'text-yellow-400',
  running: 'text-green-400',
  failing: 'text-orange-400',
  stopped: 'text-[var(--color-muted)]',
  errored: 'text-red-400',
};

function ServicePage() {
  const { workspaceSlug, projectSlug, serviceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const service = useQuery(serviceQuery(workspaceSlug, projectSlug, serviceSlug));
  const deployments = useQuery({
    ...serviceDeploymentsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 3000,
  });
  const deploy = useDeployService(workspaceSlug, projectSlug, serviceSlug);
  const stop = useStopDeployment();
  const restart = useRestartDeployment();

  const canDeploy = workspace.data
    ? workspace.data.role !== 'viewer'
    : false;

  const [error, setError] = useState<string | null>(null);
  const [activeDeploymentId, setActiveDeploymentId] = useState<string | null>(null);

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

  const svc = service.data;

  return (
    <section className="space-y-6">
      <div>
        <div className="text-xs uppercase tracking-wider text-[var(--color-muted)]">
          <Link
            to="/w/$workspaceSlug/projects"
            params={{ workspaceSlug }}
            className="hover:underline"
          >
            Projects
          </Link>{' '}
          /{' '}
          <Link
            to="/w/$workspaceSlug/projects/$projectSlug"
            params={{ workspaceSlug, projectSlug }}
            className="hover:underline"
          >
            {projectSlug}
          </Link>{' '}
          /
        </div>
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-xl font-semibold tracking-tight">
              {svc?.name ?? serviceSlug}
            </h1>
            <p className="mt-1 font-mono text-xs text-[var(--color-muted)]">
              {svc?.image_ref ?? '—'}
            </p>
          </div>
          {canDeploy ? (
            <Button onClick={onDeploy} disabled={deploy.isPending}>
              {deploy.isPending ? 'Deploying…' : 'Deploy'}
            </Button>
          ) : null}
        </div>
        {error ? (
          <div className="mt-3">
            <ErrorText>{error}</ErrorText>
          </div>
        ) : null}
      </div>

      {svc ? (
        <Card className="p-4">
          <div className="grid grid-cols-2 gap-4 text-sm sm:grid-cols-4">
            <Stat label="CPU" value={`${svc.resources.cpu_millis}m`} />
            <Stat label="Memory" value={`${svc.resources.memory_mb}MB`} />
            <Stat label="Disk" value={`${svc.resources.disk_mb}MB`} />
            <Stat label="Replicas" value={String(svc.replicas)} />
          </div>
        </Card>
      ) : null}

      <DomainsSection
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        serviceSlug={serviceSlug}
        canManage={canDeploy}
        defaultPort={svc?.ports?.[0]?.container_port ?? null}
      />

      <div>
        <h2 className="mb-3 text-sm font-medium">Deployments</h2>
        <Card className="overflow-hidden">
          <table className="w-full text-sm">
            <thead className="text-left text-xs uppercase tracking-wider text-[var(--color-muted)]">
              <tr>
                <th className="px-4 py-2 font-medium">Status</th>
                <th className="px-4 py-2 font-medium">Image</th>
                <th className="px-4 py-2 font-medium">Created</th>
                <th className="px-4 py-2 font-medium">Reason</th>
                <th className="px-4 py-2" />
              </tr>
            </thead>
            <tbody>
              {deployments.data?.length ? (
                deployments.data.map((d) => (
                  <DeploymentRow
                    key={d.id}
                    d={d}
                    active={activeDeploymentId === d.id}
                    canManage={canDeploy}
                    onSelect={() => setActiveDeploymentId(d.id)}
                    onStop={() => stop.mutate(d.id)}
                    onRestart={() => restart.mutate(d.id)}
                  />
                ))
              ) : (
                <tr>
                  <td
                    colSpan={5}
                    className="px-4 py-6 text-center text-sm text-[var(--color-muted)]"
                  >
                    No deployments yet. Click Deploy to start one.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </Card>
      </div>

      {activeDeploymentId ? (
        <div>
          <h2 className="mb-3 text-sm font-medium">Logs</h2>
          <LogViewer deploymentId={activeDeploymentId} />
        </div>
      ) : null}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wider text-[var(--color-muted)]">{label}</div>
      <div className="mt-1 font-mono text-sm">{value}</div>
    </div>
  );
}

function DeploymentRow({
  d,
  active,
  canManage,
  onSelect,
  onStop,
  onRestart,
}: {
  d: DeploymentSummary;
  active: boolean;
  canManage: boolean;
  onSelect: () => void;
  onStop: () => void;
  onRestart: () => void;
}) {
  const stoppable = d.status === 'running' || d.status === 'starting' || d.status === 'pulling';
  return (
    <tr
      className={[
        'cursor-pointer border-t border-[var(--color-border)]',
        active ? 'bg-black/5 dark:bg-white/5' : '',
      ].join(' ')}
      onClick={onSelect}
    >
      <td className="px-4 py-2">
        <span className={`font-mono text-xs ${STATUS_COLOR[d.status]}`}>{d.status}</span>
      </td>
      <td className="px-4 py-2 font-mono text-xs">{d.image_ref}</td>
      <td className="px-4 py-2 text-[var(--color-muted)]">
        {new Date(d.created_at).toLocaleString()}
      </td>
      <td className="px-4 py-2 text-xs text-[var(--color-muted)]">{d.reason ?? '—'}</td>
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
    es.addEventListener('error', () => {
      setConnState('error');
    });
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

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-xs text-[var(--color-muted)]">
        <span className="font-mono">{deploymentId}</span>
        <span>{connState}</span>
      </div>
      <div
        ref={containerRef}
        className="max-h-96 overflow-auto bg-black/30 p-3 font-mono text-xs"
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
