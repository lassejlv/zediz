import { useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type { SemanticStatus } from '@/components/ui';
import type { DeploymentStatus, DeploymentSummary } from './types';

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
