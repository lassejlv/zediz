import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type { DomainSummary } from './types';

function base(workspaceSlug: string, projectSlug: string, serviceSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}/services/${encodeURIComponent(serviceSlug)}/domains`;
}

export function domainsQuery(
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
      'domains',
    ] as const,
    queryFn: ({ signal }) =>
      api<DomainSummary[]>(base(workspaceSlug, projectSlug, serviceSlug), { signal }),
  });
}

export function useDomains(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  return useQuery({
    ...domainsQuery(workspaceSlug, projectSlug, serviceSlug),
    refetchInterval: 5000,
  });
}

export function useAddDomain(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { hostname: string; container_port?: number }) =>
      api<DomainSummary>(base(workspaceSlug, projectSlug, serviceSlug), {
        method: 'POST',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: [
          'workspace',
          workspaceSlug,
          'project',
          projectSlug,
          'service',
          serviceSlug,
          'domains',
        ],
      });
    },
  });
}

export function useUpdateDomain(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { id: string; container_port?: number }) =>
      api<DomainSummary>(
        `${base(workspaceSlug, projectSlug, serviceSlug)}/${encodeURIComponent(input.id)}`,
        {
          method: 'PATCH',
          body: { container_port: input.container_port },
        },
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
          'domains',
        ],
      });
    },
  });
}

export function useRetryDomain(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<DomainSummary>(
        `${base(workspaceSlug, projectSlug, serviceSlug)}/${encodeURIComponent(id)}/retry`,
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
          'domains',
        ],
      });
    },
  });
}

export function useDeleteDomain(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`${base(workspaceSlug, projectSlug, serviceSlug)}/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      }),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: [
          'workspace',
          workspaceSlug,
          'project',
          projectSlug,
          'service',
          serviceSlug,
          'domains',
        ],
      });
    },
  });
}
