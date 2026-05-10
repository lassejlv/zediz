import {
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { api } from './api';
import type { SemanticStatus } from '@/components/ui';
import type { BuildStatus, BuildSummary } from './types';

export function buildsQuery(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  return queryOptions({
    queryKey: [
      'workspace',
      workspaceSlug,
      'project',
      projectSlug,
      'service',
      serviceSlug,
      'builds',
    ] as const,
    queryFn: ({ signal }) =>
      api<BuildSummary[]>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(
          projectSlug,
        )}/services/${encodeURIComponent(serviceSlug)}/builds`,
        { signal },
      ),
  });
}

export function useBuilds(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  return useQuery(buildsQuery(workspaceSlug, projectSlug, serviceSlug));
}

export function isBuildCancellable(status: BuildStatus | undefined): boolean {
  if (!status) return false;
  return !['succeeded', 'failed', 'cancelled'].includes(status);
}

export function useCancelBuild(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (buildId: string) =>
      api<BuildSummary>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(
          projectSlug,
        )}/services/${encodeURIComponent(serviceSlug)}/builds/${encodeURIComponent(
          buildId,
        )}/cancel`,
        { method: 'POST' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: [
          'workspace',
          workspaceSlug,
          'project',
          projectSlug,
          'service',
          serviceSlug,
          'builds',
        ],
      });
      // The cancel route also errors the deployment; invalidate that list too.
      qc.invalidateQueries({
        queryKey: [
          'workspace',
          workspaceSlug,
          'project',
          projectSlug,
          'service',
          serviceSlug,
          'deployments',
        ],
      });
    },
  });
}

/** Map a build's lifecycle status to a UI tone + pulse flag. */
export function buildTone(s: BuildStatus | undefined): {
  tone: SemanticStatus;
  pulse: boolean;
} {
  if (!s) return { tone: 'muted', pulse: false };
  switch (s) {
    case 'succeeded':
      return { tone: 'ok', pulse: false };
    case 'queued':
      return { tone: 'info', pulse: true };
    case 'cloning':
    case 'building':
    case 'pushing':
      return { tone: 'warn', pulse: true };
    case 'failed':
      return { tone: 'error', pulse: false };
    case 'cancelled':
      return { tone: 'muted', pulse: false };
  }
}
