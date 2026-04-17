import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { serviceDeploymentsQuery } from '@/lib/services';
import { Card, EmptyState, Stack } from '@/components/ui';
import type { RuntimeMetrics, ServiceSummary } from '@/lib/types';

interface Props {
  service: ServiceSummary;
  workspaceSlug: string;
  projectSlug: string;
  serviceSlug: string;
}

/** How many samples to keep in the in-memory ring buffer. At a ~10s
 * agent heartbeat this is ≈10 minutes of history. Reloading the tab
 * starts a fresh buffer — no server-side history is kept. */
const MAX_SAMPLES = 60;

interface Sample {
  ts: number; // epoch millis
  cpu: number; // percent
  memory: number; // bytes
  memoryLimit: number | null;
  rx: number; // cumulative bytes
  tx: number; // cumulative bytes
}

interface RateSample {
  ts: number;
  cpu: number;
  memory: number;
  memoryLimit: number | null;
  rxRate: number; // bytes/sec
  txRate: number; // bytes/sec
}

export function ServiceMetricsTab({
  workspaceSlug,
  projectSlug,
  serviceSlug,
}: Props) {
  const deployments = useQuery({
    ...serviceDeploymentsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 3_000,
  });

  const running = deployments.data?.find((d) => d.status === 'running');
  const metrics = running?.runtime_metrics ?? null;

  const [samples, setSamples] = useState<Sample[]>([]);
  const deploymentIdRef = useRef<string | null>(null);

  // Reset the buffer if the running deployment changes (redeploy cutover).
  useEffect(() => {
    const id = running?.id ?? null;
    if (id !== deploymentIdRef.current) {
      deploymentIdRef.current = id;
      setSamples([]);
    }
  }, [running?.id]);

  // Append each new sample; dedupe by timestamp.
  useEffect(() => {
    if (!metrics) return;
    setSamples((prev) => appendSample(prev, metrics));
  }, [metrics]);

  const rateSamples = useMemo(() => toRateSamples(samples), [samples]);
  const latest = rateSamples[rateSamples.length - 1];

  if (!running) {
    return (
      <EmptyState
        title="No running deployment"
        body="Metrics appear here once a deployment is live on a node."
      />
    );
  }

  if (rateSamples.length === 0) {
    return (
      <Stack gap={3}>
        <EmptyState
          title="Waiting for the first sample"
          body="The agent reports container CPU, memory, and network on each heartbeat (≈ every 10s)."
        />
      </Stack>
    );
  }

  const cpuValues = rateSamples.map((s) => s.cpu);
  const memValues = rateSamples.map((s) => s.memory);
  const rxValues = rateSamples.map((s) => s.rxRate);
  const txValues = rateSamples.map((s) => s.txRate);
  const netValues = rateSamples.map((s) => Math.max(s.rxRate, s.txRate));

  const memMax = latest.memoryLimit ?? Math.max(1, ...memValues);
  const cpuMax = Math.max(5, ...cpuValues) * 1.1;
  const netMax = Math.max(1024, ...netValues) * 1.1;

  return (
    <Stack gap={3}>
      <MetricCard
        label="CPU"
        value={`${latest.cpu.toFixed(1)}%`}
        hint={`peak ${Math.max(...cpuValues).toFixed(1)}% · ${rateSamples.length} sample${rateSamples.length === 1 ? '' : 's'}`}
        tone="accent"
      >
        <Chart values={cpuValues} max={cpuMax} tone="accent" yFormat={(v) => `${v.toFixed(0)}%`} />
      </MetricCard>

      <MetricCard
        label="Memory"
        value={formatBytes(latest.memory)}
        hint={
          latest.memoryLimit
            ? `limit ${formatBytes(latest.memoryLimit)} · ${((latest.memory / latest.memoryLimit) * 100).toFixed(1)}%`
            : `peak ${formatBytes(Math.max(...memValues))}`
        }
        tone="info"
      >
        <Chart
          values={memValues}
          max={memMax}
          tone="info"
          yFormat={(v) => formatBytes(v, { compact: true })}
        />
      </MetricCard>

      <MetricCard
        label="Network"
        value={`↓ ${formatRate(latest.rxRate)}  ↑ ${formatRate(latest.txRate)}`}
        hint={`cumulative ↓ ${formatBytes(samples[samples.length - 1]?.rx ?? 0)} · ↑ ${formatBytes(samples[samples.length - 1]?.tx ?? 0)}`}
        tone="warn"
      >
        <Chart
          series={[
            { values: rxValues, tone: 'accent', label: 'rx' },
            { values: txValues, tone: 'warn', label: 'tx' },
          ]}
          max={netMax}
          yFormat={(v) => formatRate(v, { compact: true })}
        />
      </MetricCard>
    </Stack>
  );
}

/* ---------- card + chart ---------- */

type Tone = 'accent' | 'info' | 'warn';

function toneStroke(tone: Tone): string {
  return (
    {
      accent: 'stroke-emerald-500',
      info: 'stroke-indigo-400',
      warn: 'stroke-amber-400',
    }
  )[tone];
}

function toneFill(tone: Tone): string {
  return (
    {
      accent: 'fill-emerald-500',
      info: 'fill-indigo-400',
      warn: 'fill-amber-400',
    }
  )[tone];
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
  tone: Tone;
  children: React.ReactNode;
}) {
  return (
    <Card className="p-5">
      <div className="mb-3 flex items-end justify-between gap-4">
        <div>
          <div className="text-[11px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            {label}
          </div>
          <div className="mt-0.5 font-mono text-xl leading-tight">{value}</div>
          {hint ? (
            <div className="mt-0.5 text-xs text-[var(--color-muted)]">{hint}</div>
          ) : null}
        </div>
      </div>
      {children}
    </Card>
  );
}

type ChartProps =
  | {
      values: number[];
      max: number;
      tone: Tone;
      yFormat: (v: number) => string;
      series?: never;
    }
  | {
      series: { values: number[]; tone: Tone; label: string }[];
      max: number;
      yFormat: (v: number) => string;
      values?: never;
      tone?: never;
    };

function Chart(props: ChartProps) {
  const height = 120;
  const series =
    props.series ?? [{ values: props.values, tone: props.tone, label: '' }];
  const width = Math.max(2, series[0].values.length - 1);

  return (
    <div className="relative">
      <div className="pointer-events-none absolute inset-0 flex flex-col justify-between text-[10px] text-[var(--color-subtle)]">
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
        {/* baseline + mid grid lines */}
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
          const points = s.values
            .map((v, idx) => {
              const x = idx;
              const y = height - (clamp(v, 0, props.max) / props.max) * height;
              return `${x},${y}`;
            })
            .join(' ');
          const areaPoints = `0,${height} ${points} ${width},${height}`;
          return (
            <g key={i}>
              <polygon
                points={areaPoints}
                className={`${toneFill(s.tone)} opacity-15`}
              />
              <polyline
                points={points}
                fill="none"
                className={toneStroke(s.tone)}
                strokeWidth="1.5"
                strokeLinejoin="round"
                strokeLinecap="round"
                vectorEffect="non-scaling-stroke"
              />
            </g>
          );
        })}
      </svg>
      {props.series ? (
        <div className="mt-2 flex gap-4 text-xs text-[var(--color-muted)]">
          {props.series.map((s) => (
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

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/* ---------- ring buffer ---------- */

function appendSample(prev: Sample[], m: RuntimeMetrics): Sample[] {
  const ts = new Date(m.ts).getTime();
  if (prev.length > 0 && prev[prev.length - 1].ts === ts) return prev;
  const next = [
    ...prev,
    {
      ts,
      cpu: m.cpu_percent ?? 0,
      memory: m.memory_bytes ?? 0,
      memoryLimit: m.memory_limit_bytes ?? null,
      rx: m.rx_bytes ?? 0,
      tx: m.tx_bytes ?? 0,
    },
  ];
  return next.length > MAX_SAMPLES ? next.slice(next.length - MAX_SAMPLES) : next;
}

function toRateSamples(samples: Sample[]): RateSample[] {
  if (samples.length === 0) return [];
  const out: RateSample[] = [];
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i];
    let rxRate = 0;
    let txRate = 0;
    if (i > 0) {
      const prev = samples[i - 1];
      const dt = (s.ts - prev.ts) / 1000;
      if (dt > 0) {
        // Counters reset if the container was recreated; guard against negatives.
        rxRate = Math.max(0, (s.rx - prev.rx) / dt);
        txRate = Math.max(0, (s.tx - prev.tx) / dt);
      }
    }
    out.push({
      ts: s.ts,
      cpu: s.cpu,
      memory: s.memory,
      memoryLimit: s.memoryLimit,
      rxRate,
      txRate,
    });
  }
  return out;
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

