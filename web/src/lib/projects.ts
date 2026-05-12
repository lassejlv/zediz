import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type { ProjectSummary } from './types';

export function projectsQuery(slug: string) {
  return queryOptions({
    queryKey: ['workspace', slug, 'projects'] as const,
    queryFn: ({ signal }) =>
      api<ProjectSummary[]>(`/workspaces/${encodeURIComponent(slug)}/projects`, { signal }),
  });
}

export function useProjects(slug: string) {
  return useQuery(projectsQuery(slug));
}

export function projectQuery(slug: string, projectSlug: string) {
  return queryOptions({
    queryKey: ['workspace', slug, 'project', projectSlug] as const,
    queryFn: ({ signal }) =>
      api<ProjectSummary>(
        `/workspaces/${encodeURIComponent(slug)}/projects/${encodeURIComponent(projectSlug)}`,
        { signal },
      ),
  });
}

export function useCreateProject(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { slug: string; name: string; hetzner_location: string }) =>
      api<ProjectSummary>(`/workspaces/${encodeURIComponent(slug)}/projects`, {
        method: 'POST',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'projects'] });
    },
  });
}

export function useDeleteProject(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (projectSlug: string) =>
      api<void>(
        `/workspaces/${encodeURIComponent(slug)}/projects/${encodeURIComponent(projectSlug)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'projects'] });
    },
  });
}
