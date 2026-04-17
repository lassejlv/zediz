import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { serviceDeploymentsQuery } from '@/lib/services';
import { deploymentMetricsQuery } from '@/lib/deployments';
import { Card, EmptyState, Stack } from '@/components/ui';
import type { ServiceSummary } from '@/lib/types';

interface Props {
  service: ServiceSummary;
  workspaceSlug: string;
  projectSlug: string;
  serviceSlug: string;
}

type RangeKey = '5m' | '15m' | '1h';
const RANGES: { key: RangeKey; label: string; minutes: number }[] = [
  { key: '5m', label: '5m', minutes: 5 },
  { key: '15m', label: '15m', minutes: 15 },
  { key: '1h', label: '1h', minutes: 60 },
];

/** How often the chart re-polls for fresh samples. Matches the agent's
 * heartbeat cadence — no point asking more often than the agent sends. */
const REFRESH_MS = 10_000;

export function ServiceMetricsTab({
  workspaceSlug,
  projectSlug,
  serviceSlug,
}: Props) {
  const deployments = useQuery({
    ...serviceDeploymentsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: REFRESH_MS,
  });

  const running = deployments.data?.find((d) => d.status === 'running');
  const [range, setRange] = useState<RangeKey>('15m');
  const minutes = RANGES.find((r) => r.key === range)!.minutes;

  const history = useQuery({
    ...deploymentMetricsQuery(running?.id ?? '', minutes),
    enabled: !!running,
    refetchInterval: running ? REFRESH_MS : false,
  });

  if (!running) {
    return (
      <EmptyState
        title="No running deployment"
        body="Metrics appear here once a deployment is live on a node."
      />
    );
  }

  const samples = history.data ?? [];
  const hasSamples = samples.length > 0;
  const latest = hasSamples ? samples[samples.length - 1] : null;

  const cpuValues = samples.map((s) => s.cpu_percent);
  const memValues = samples.map((s) => s.memory_bytes);
  const rxRates = samples
    .map((s) => s.rx_rate)
    .map((v) => (v == null ? null : v));
  const txRates = samples
    .map((s) => s.tx_rate)
    .map((v) => (v == null ? null : v));
  const rxRatesDefined = rxRates.filter((v): v is number => v != null);
  const txRatesDefined = txRates.filter((v): v is number => v != null);

  const memLimit = latest?.memory_limit_bytes ?? null;
  const memMax = memLimit ?? Math.max(1, ...memValues);
  const cpuMax = cpuValues.length ? Math.max(5, ...cpuValues) * 1.1 : 5;
  const netMax = Math.max(
    1024,
    ...rxRatesDefined,
    ...txRatesDefined,
  ) * 1.1;

  const latestRx = latest?.rx_rate ?? null;
  const latestTx = latest?.tx_rate ?? null;

  return (
    <Stack gap={3}>
      <div className="flex items-center justify-between">
        <div className="text-xs text-[var(--color-muted)]">
          Live container stats from the agent, retained for the last hour.
        </div>
        <div className="inline-flex gap-1 rounded-md border border-[var(--color-border)] p-0.5">
          {RANGES.map((r) => (
            <button
              key={r.key}
              type="button"
              onClick={() => setRange(r.key)}
              className={[
                'rounded px-2.5 py-1 text-xs font-medium transition-colors',
                r.key === range
                  ? 'bg-[var(--color-surface-elevated)] text-[var(--color-fg)]'
                  : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
              ].join(' ')}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      {!hasSamples ? (
        <EmptyState
          title={history.isLoading ? 'Loading metrics…' : 'Waiting for the first sample'}
          body={
            history.isLoading
              ? undefined
              : 'The agent samples container stats on each heartbeat (~10s). Hang tight.'
          }
        />
      ) : (
        <>
          <MetricCard
            label="CPU"
            value={latest ? `${latest.cpu_percent.toFixed(1)}%` : '—'}
            hint={
              cpuValues.length
                ? `peak ${Math.max(...cpuValues).toFixed(1)}% · avg ${avg(cpuValues).toFixed(1)}% · ${samples.length} sample${samples.length === 1 ? '' : 's'}`
                : undefined
            }
          >
            <Chart
              values={cpuValues.map((v) => ({ value: v }))}
              max={cpuMax}
              tone="accent"
              yFormat={(v) => `${v.toFixed(0)}%`}
            />
          </MetricCard>

          <MetricCard
            label="Memory"
            value={latest ? formatBytes(latest.memory_bytes) : '—'}
            hint={
              memLimit
                ? `limit ${formatBytes(memLimit)} · ${((latest!.memory_bytes / memLimit) * 100).toFixed(1)}% used`
                : memValues.length
                  ? `peak ${formatBytes(Math.max(...memValues))}`
                  : undefined
            }
          >
            <Chart
              values={memValues.map((v) => ({ value: v }))}
              max={memMax}
              tone="info"
              yFormat={(v) => formatBytes(v, { compact: true })}
            />
          </MetricCard>

          <MetricCard
            label="Network"
            value={
              latestRx != null && latestTx != null
                ? `↓ ${formatRate(latestRx)}  ↑ ${formatRate(latestTx)}`
                : latest
                  ? 'warming up…'
                  : '—'
            }
            hint={
              latest
                ? `cumulative ↓ ${formatBytes(latest.rx_bytes)} · ↑ ${formatBytes(latest.tx_bytes)}`
                : undefined
            }
          >
            <Chart
              multi={[
                {
                  label: 'rx',
                  tone: 'accent',
                  values: rxRates.map((v) => ({ value: v })),
                },
                {
                  label: 'tx',
                  tone: 'warn',
                  values: txRates.map((v) => ({ value: v })),
                },
              ]}
              max={netMax}
              yFormat={(v) => formatRate(v, { compact: true })}
            />
          </MetricCard>
        </>
      )}
    </Stack>
  );
}

/* ---------- card + chart ---------- */

type Tone = 'accent' | 'info' | 'warn';

function toneStroke(tone: Tone): string {
  return {
    accent: 'stroke-emerald-500',
    info: 'stroke-indigo-400',
    warn: 'stroke-amber-400',
  }[tone];
}

function toneFill(tone: Tone): string {
  return {
    accent: 'fill-emerald-500',
    info: 'fill-indigo-400',
    warn: 'fill-amber-400',
  }[tone];
}

function MetricCard({
  label,
  value,
  hint,
  children,
}: {
  label: string;
  value: React.ReactNode;
  hint?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Card className="p-5">
      <div className="mb-3">
        <div className="text-[11px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
          {label}
        </div>
        <div className="mt-0.5 font-mono text-xl leading-tight">{value}</div>
        {hint ? (
          <div className="mt-0.5 text-xs text-[var(--color-muted)]">{hint}</div>
        ) : null}
      </div>
      {children}
    </Card>
  );
}

interface Point {
  value: number | null;
}

interface Series {
  label: string;
  tone: Tone;
  values: Point[];
}

type ChartProps =
  | {
      values: Point[];
      max: number;
      tone: Tone;
      yFormat: (v: number) => string;
      multi?: never;
    }
  | {
      multi: Series[];
      max: number;
      yFormat: (v: number) => string;
      values?: never;
      tone?: never;
    };

function Chart(props: ChartProps) {
  const height = 120;
  const series: Series[] =
    props.multi ?? [{ values: props.values, tone: props.tone, label: '' }];
  const len = series[0].values.length;
  const width = Math.max(2, len - 1);

  return (
    <div className="relative">
      <div className="pointer-events-none absolute inset-0 flex flex-col justify-between py-0.5 pr-1 text-right text-[10px] text-[var(--color-subtle)]">
        <span>{props.yFormat(props.max)}</span>
        <span>{props.yFormat(props.max / 2)}</span>
        <span>0</span>
      </div>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        className="block h-32 w-full"
        aria-hidden
      >
        <line
          x1={0}
          x2={width}
          y1={height}
          y2={height}
          className="stroke-[var(--color-border)]"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        <line
          x1={0}
          x2={width}
          y1={height / 2}
          y2={height / 2}
          className="stroke-[var(--color-border)]"
          strokeDasharray="2 4"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        {series.map((s, i) => {
          const segments = definedSegments(
            s.values.map((p) => p.value),
            props.max,
            height,
          );
          return (
            <g key={i}>
              {segments.map((seg, j) => (
                <g key={j}>
                  <polygon
                    points={`${seg.startX},${height} ${seg.path} ${seg.endX},${height}`}
                    className={`${toneFill(s.tone)} opacity-15`}
                  />
                  <polyline
                    points={seg.path}
                    fill="none"
                    className={toneStroke(s.tone)}
                    strokeWidth="1.5"
                    strokeLinejoin="round"
                    strokeLinecap="round"
                    vectorEffect="non-scaling-stroke"
                  />
                </g>
              ))}
            </g>
          );
        })}
      </svg>
      {props.multi ? (
        <div className="mt-2 flex gap-4 text-xs text-[var(--color-muted)]">
          {props.multi.map((s) => (
            <span key={s.label} className="inline-flex items-center gap-1.5">
              <span
                className={`${toneFill(s.tone)} inline-block h-1.5 w-3 rounded-sm`}
              />
              {s.label}
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** Split a value series into contiguous runs of defined numbers, rendering
 * each as its own polyline so `null` (no data / counter reset) leaves a
 * visible gap instead of a vertical spike. */
function definedSegments(
  values: (number | null)[],
  max: number,
  height: number,
): { path: string; startX: number; endX: number }[] {
  const out: { path: string; startX: number; endX: number }[] = [];
  let current: string[] = [];
  let startX = 0;
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (v == null) {
      if (current.length >= 2) {
        out.push({ path: current.join(' '), startX, endX: i - 1 });
      }
      current = [];
      continue;
    }
    if (current.length === 0) startX = i;
    const y = height - (clamp(v, 0, max) / max) * height;
    current.push(`${i},${y}`);
  }
  if (current.length >= 2) {
    out.push({ path: current.join(' '), startX, endX: values.length - 1 });
  }
  return out;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function avg(values: number[]): number {
  if (values.length === 0) return 0;
  return values.reduce((a, b) => a + b, 0) / values.length;
}

/* ---------- formatting ---------- */

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

function formatBytes(n: number, opts: { compact?: boolean } = {}): string {
  if (n <= 0) return '0 B';
  let idx = 0;
  let v = n;
  while (v >= 1024 && idx < UNITS.length - 1) {
    v /= 1024;
    idx++;
  }
  const digits = v >= 100 || idx === 0 ? 0 : v >= 10 ? 1 : 2;
  const formatted = v.toFixed(digits);
  return opts.compact ? `${formatted}${UNITS[idx]}` : `${formatted} ${UNITS[idx]}`;
}

function formatRate(n: number, opts: { compact?: boolean } = {}): string {
  return `${formatBytes(n, opts)}/s`;
}

