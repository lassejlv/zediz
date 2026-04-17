import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { buildsQuery } from '@/lib/builds';
import { domainsQuery } from '@/lib/domains';
import { serviceDeploymentsQuery } from '@/lib/services';
import {
  Card,
  EmptyState,
  RelativeTime,
  Stack,
  StatCard,
  StatusDot,
  type SemanticStatus,
} from '@/components/ui';
import type {
  BuildSummary,
  DeploymentSummary,
  ServiceSummary,
} from '@/lib/types';

interface Props {
  service: ServiceSummary;
  workspaceSlug: string;
  projectSlug: string;
  serviceSlug: string;
}

export function ServiceMetricsTab({
  service,
  workspaceSlug,
  projectSlug,
  serviceSlug,
}: Props) {
  const deployments = useQuery({
    ...serviceDeploymentsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 10_000,
  });
  const builds = useQuery(buildsQuery(workspaceSlug, projectSlug, serviceSlug));
  const domains = useQuery(domainsQuery(workspaceSlug, projectSlug, serviceSlug));

  const d = deployments.data ?? [];
  const b = builds.data ?? [];

  const running = d.find((x) => x.status === 'running');
  const buildStats = useMemo(() => computeBuildStats(b), [b]);
  const deployStats = useMemo(() => computeDeployStats(d), [d]);
  const deploySeries = useMemo(() => bucketDeploysPerDay(d, 30), [d]);

  return (
    <Stack gap={4}>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatCard
          label="Current uptime"
          value={<Uptime startedAt={running?.started_at ?? null} />}
          hint={running ? 'since last deploy went live' : 'no running deployment'}
        />
        <StatCard
          label="Deploys (7d)"
          value={String(deployStats.last7d)}
          hint={`${deployStats.last30d} in the last 30d`}
        />
        <StatCard
          label="Build success"
          value={
            buildStats.total > 0
              ? `${Math.round((buildStats.succeeded / buildStats.total) * 100)}%`
              : '—'
          }
          hint={
            buildStats.total > 0
              ? `${buildStats.succeeded} / ${buildStats.total} of last ${buildStats.total}`
              : 'no builds yet'
          }
        />
        <StatCard
          label="Avg build time"
          value={formatDuration(buildStats.avgDurationSec)}
          hint={
            buildStats.slowestSec != null
              ? `slowest ${formatDuration(buildStats.slowestSec)}`
              : '—'
          }
        />
      </div>

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <Card className="p-5">
          <SectionHeading
            title="Deploys per day"
            subtitle="Last 30 days"
            right={
              <span className="text-xs text-[var(--color-muted)]">
                {deployStats.total} total
              </span>
            }
          />
          {deploySeries.max === 0 ? (
            <EmptyBlock>No deploys in this window.</EmptyBlock>
          ) : (
            <DeployDailyBars series={deploySeries.buckets} max={deploySeries.max} />
          )}
        </Card>

        <Card className="p-5">
          <SectionHeading
            title="Build duration"
            subtitle={`Last ${Math.min(b.length, 20)} builds`}
          />
          {b.length === 0 ? (
            <EmptyBlock>No builds yet.</EmptyBlock>
          ) : (
            <BuildDurationBars builds={b.slice(0, 20)} />
          )}
        </Card>
      </div>

      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        <Card className="p-5">
          <SectionHeading title="Reserved resources" subtitle="Per replica" />
          <dl className="space-y-2 text-sm">
            <ResourceRow label="CPU" value={`${service.resources.cpu_millis}m`} />
            <ResourceRow label="Memory" value={`${service.resources.memory_mb} MB`} />
            <ResourceRow label="Disk" value={`${service.resources.disk_mb} MB`} />
            <ResourceRow label="Replicas" value={String(service.replicas)} />
          </dl>
        </Card>

        <Card className="p-5">
          <SectionHeading title="Traffic" subtitle="Hostnames routed here" />
          {domains.data && domains.data.length > 0 ? (
            <ul className="space-y-1.5 text-xs">
              {domains.data.map((h) => (
                <li key={h.id} className="flex items-center gap-2">
                  <StatusDot
                    status={
                      h.tls_status === 'active'
                        ? 'ok'
                        : h.tls_status === 'failed'
                          ? 'error'
                          : 'warn'
                    }
                    pulse={h.tls_status === 'pending'}
                  />
                  <a
                    href={`https://${h.hostname}`}
                    target="_blank"
                    rel="noreferrer"
                    className="truncate font-mono hover:text-[var(--color-accent)]"
                  >
                    {h.hostname}
                  </a>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-[var(--color-muted)]">No domains routed yet.</p>
          )}
        </Card>

        <Card className="p-5">
          <SectionHeading title="Timeline" subtitle="Recent activity" />
          <dl className="space-y-2 text-sm">
            <TimelineRow
              label="Last deploy"
              when={d[0]?.created_at ?? null}
              tone={d[0] ? deploymentTone(d[0].status) : 'muted'}
            />
            <TimelineRow
              label="Last successful build"
              when={
                b.find((x) => x.status === 'succeeded')?.finished_at ?? null
              }
              tone="ok"
            />
            <TimelineRow
              label="Service created"
              when={service.created_at}
              tone="muted"
            />
          </dl>
        </Card>
      </div>

      <p className="text-xs text-[var(--color-muted)]">
        Live container CPU and memory aren't captured yet — the agent doesn't sample cgroup
        stats. These metrics are derived from the deployment and build history.
      </p>
    </Stack>
  );
}

/* ---------- sections / rows ---------- */

function SectionHeading({
  title,
  subtitle,
  right,
}: {
  title: string;
  subtitle?: string;
  right?: React.ReactNode;
}) {
  return (
    <div className="mb-4 flex items-end justify-between gap-3">
      <div>
        <h3 className="text-sm font-medium">{title}</h3>
        {subtitle ? (
          <p className="mt-0.5 text-xs text-[var(--color-muted)]">{subtitle}</p>
        ) : null}
      </div>
      {right}
    </div>
  );
}

function EmptyBlock({ children }: { children: React.ReactNode }) {
  return (
    <EmptyState
      title={children as string}
      className="border-0 !px-0 !py-6 text-left"
    />
  );
}

function ResourceRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between">
      <dt className="text-xs uppercase tracking-wider text-[var(--color-muted)]">
        {label}
      </dt>
      <dd className="font-mono text-sm">{value}</dd>
    </div>
  );
}

function TimelineRow({
  label,
  when,
  tone,
}: {
  label: string;
  when: string | null;
  tone: SemanticStatus;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <dt className="flex items-center gap-2 text-xs uppercase tracking-wider text-[var(--color-muted)]">
        <StatusDot status={tone} />
        {label}
      </dt>
      <dd className="text-xs">
        {when ? (
          <RelativeTime date={when} className="!text-[var(--color-fg)]" />
        ) : (
          <span className="text-[var(--color-muted)]">—</span>
        )}
      </dd>
    </div>
  );
}

/* ---------- uptime ---------- */

function Uptime({ startedAt }: { startedAt: string | null }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!startedAt) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [startedAt]);

  if (!startedAt) return <span className="text-[var(--color-muted)]">—</span>;
  const secs = Math.max(0, Math.floor((Date.now() - new Date(startedAt).getTime()) / 1000));
  return <span className="font-mono">{formatUptime(secs)}</span>;
}

function formatUptime(secs: number): string {
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${s}s`;
  return `${s}s`;
}

/* ---------- bar charts ---------- */

function DeployDailyBars({
  series,
  max,
}: {
  series: { date: Date; label: string; count: number }[];
  max: number;
}) {
  return (
    <div className="flex h-24 items-end gap-0.5">
      {series.map((bucket) => {
        const pct = max === 0 ? 0 : (bucket.count / max) * 100;
        return (
          <div
            key={bucket.label}
            title={`${bucket.label}: ${bucket.count} deploy${bucket.count === 1 ? '' : 's'}`}
            className="group relative flex-1"
            style={{ minWidth: 0 }}
          >
            <div
              className={[
                'w-full rounded-sm transition-opacity',
                bucket.count === 0
                  ? 'h-[2px] bg-[var(--color-border)]'
                  : 'bg-[var(--color-accent)]/60 hover:bg-[var(--color-accent)]',
              ].join(' ')}
              style={bucket.count > 0 ? { height: `${Math.max(pct, 8)}%` } : undefined}
            />
          </div>
        );
      })}
    </div>
  );
}

function BuildDurationBars({ builds }: { builds: BuildSummary[] }) {
  const ordered = [...builds].reverse(); // oldest → newest
  const max = Math.max(
    1,
    ...ordered.map((b) => durationSec(b) ?? 0),
  );

  return (
    <div className="space-y-1">
      {ordered.map((b) => {
        const dur = durationSec(b);
        const pct = dur == null ? 0 : (dur / max) * 100;
        const tone =
          b.status === 'succeeded'
            ? 'bg-emerald-500/50'
            : b.status === 'failed'
              ? 'bg-red-500/50'
              : 'bg-neutral-500/40';
        return (
          <div
            key={b.id}
            className="flex items-center gap-3 text-xs"
            title={`${b.status}${dur != null ? ` · ${formatDuration(dur)}` : ''}${
              b.git_commit ? ` · ${b.git_commit.slice(0, 7)}` : ''
            }`}
          >
            <span className="w-12 shrink-0 font-mono text-[var(--color-muted)]">
              {b.git_commit ? b.git_commit.slice(0, 7) : '—'}
            </span>
            <div className="relative h-2 flex-1 overflow-hidden rounded bg-black/5 dark:bg-white/[0.04]">
              <div
                className={`h-full rounded ${tone}`}
                style={{ width: dur == null ? '100%' : `${Math.max(pct, 2)}%` }}
              />
            </div>
            <span className="w-12 shrink-0 text-right font-mono text-[var(--color-muted)]">
              {dur == null ? '—' : formatDuration(dur)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/* ---------- data aggregation ---------- */

function computeDeployStats(list: DeploymentSummary[]) {
  const now = Date.now();
  const day = 86_400_000;
  let last7d = 0;
  let last30d = 0;
  for (const d of list) {
    const age = now - new Date(d.created_at).getTime();
    if (age <= 7 * day) last7d++;
    if (age <= 30 * day) last30d++;
  }
  return { total: list.length, last7d, last30d };
}

function bucketDeploysPerDay(list: DeploymentSummary[], days: number) {
  const buckets: { date: Date; label: string; count: number }[] = [];
  const today = startOfDay(new Date());
  for (let i = days - 1; i >= 0; i--) {
    const date = new Date(today.getTime() - i * 86_400_000);
    buckets.push({
      date,
      label: date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }),
      count: 0,
    });
  }
  for (const d of list) {
    const created = startOfDay(new Date(d.created_at));
    const diffDays = Math.floor(
      (today.getTime() - created.getTime()) / 86_400_000,
    );
    if (diffDays < 0 || diffDays >= days) continue;
    const idx = days - 1 - diffDays;
    buckets[idx].count++;
  }
  return { buckets, max: buckets.reduce((m, b) => Math.max(m, b.count), 0) };
}

function computeBuildStats(list: BuildSummary[]) {
  const window = list.slice(0, 20);
  const completed = window.filter(
    (b) => b.status === 'succeeded' || b.status === 'failed',
  );
  const succeeded = completed.filter((b) => b.status === 'succeeded');
  const durations = succeeded
    .map(durationSec)
    .filter((v): v is number => v != null);
  const avg =
    durations.length === 0
      ? null
      : durations.reduce((a, b) => a + b, 0) / durations.length;
  const slowest = durations.length === 0 ? null : Math.max(...durations);
  return {
    total: completed.length,
    succeeded: succeeded.length,
    avgDurationSec: avg,
    slowestSec: slowest,
  };
}

function durationSec(b: BuildSummary): number | null {
  if (!b.started_at || !b.finished_at) return null;
  const ms = new Date(b.finished_at).getTime() - new Date(b.started_at).getTime();
  if (!Number.isFinite(ms) || ms < 0) return null;
  return Math.round(ms / 1000);
}

function startOfDay(d: Date): Date {
  const out = new Date(d);
  out.setHours(0, 0, 0, 0);
  return out;
}

function formatDuration(secs: number | null): string {
  if (secs == null) return '—';
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (m < 60) return s === 0 ? `${m}m` : `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem === 0 ? `${h}h` : `${h}h ${rem}m`;
}

function deploymentTone(status: DeploymentSummary['status']): SemanticStatus {
  switch (status) {
    case 'running':
      return 'ok';
    case 'errored':
    case 'failing':
      return 'error';
    case 'stopped':
      return 'muted';
    default:
      return 'warn';
  }
}
