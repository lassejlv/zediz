import { queryOptions, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type { SemanticStatus } from '@/components/ui';
import type { DeploymentStatus, DeploymentSummary } from './types';

export interface MetricsSample {
  ts: string;
  cpu_percent: number;
  memory_bytes: number;
  memory_limit_bytes: number | null;
  rx_bytes: number;
  tx_bytes: number;
  /** Bytes/sec since the previous sample. Null on the first row
   * returned or when the counter reset (container was recreated). */
  rx_rate: number | null;
  tx_rate: number | null;
}

export function deploymentMetricsQuery(deploymentId: string, minutes: number) {
  return queryOptions({
    queryKey: ['deployment', deploymentId, 'metrics', minutes] as const,
    queryFn: ({ signal }) =>
      api<MetricsSample[]>(
        `/deployments/${encodeURIComponent(deploymentId)}/metrics?minutes=${minutes}`,
        { signal },
      ),
  });
}

/**
 * Map a deployment's lifecycle status to a UI tone + whether it should pulse.
 * A missing status is treated as "never deployed" (muted, no pulse).
 */
export function deploymentTone(s: DeploymentStatus | undefined): {
  tone: SemanticStatus;
  pulse: boolean;
} {
  if (!s) return { tone: 'muted', pulse: false };
  switch (s) {
    case 'running':
      return { tone: 'ok', pulse: false };
    case 'pending':
    case 'placing':
      return { tone: 'info', pulse: true };
    case 'building':
    case 'pulling':
    case 'starting':
    case 'failing':
      return { tone: 'warn', pulse: true };
    case 'errored':
      return { tone: 'error', pulse: false };
    case 'stopped':
      return { tone: 'muted', pulse: false };
  }
}

export function useStopDeployment() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<DeploymentSummary>(`/deployments/${encodeURIComponent(id)}/stop`, { method: 'POST' }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace'] });
    },
  });
}

export function useRestartDeployment() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<DeploymentSummary>(`/deployments/${encodeURIComponent(id)}/restart`, {
        method: 'POST',
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace'] });
    },
  });
}
