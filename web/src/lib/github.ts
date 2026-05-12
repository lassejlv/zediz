import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './api';

export interface GitHubInstallationSummary {
  installation_id: number;
  account_login: string;
  account_type: string;
  repository_selection: string;
  active: boolean;
  html_url: string | null;
  updated_at: string;
}

export interface GitHubRepositorySummary {
  installation_id: number;
  repository_id: number;
  full_name: string;
  private: boolean;
  default_branch: string;
  clone_url: string;
  html_url: string;
  archived: boolean;
  disabled: boolean;
  pushed_at: string | null;
  updated_at: string;
}

export function githubInstallationsQuery(workspaceSlug: string) {
  return queryOptions({
    queryKey: ['workspace', workspaceSlug, 'github', 'installations'] as const,
    queryFn: ({ signal }) =>
      api<GitHubInstallationSummary[]>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/github/installations`,
        { signal },
      ),
  });
}

export function githubRepositoriesQuery(workspaceSlug: string) {
  return queryOptions({
    queryKey: ['workspace', workspaceSlug, 'github', 'repositories'] as const,
    queryFn: ({ signal }) =>
      api<GitHubRepositorySummary[]>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/github/repositories`,
        { signal },
      ),
  });
}

export function useGitHubInstallations(workspaceSlug: string) {
  return useQuery(githubInstallationsQuery(workspaceSlug));
}

export function useGitHubRepositories(workspaceSlug: string) {
  return useQuery(githubRepositoriesQuery(workspaceSlug));
}

export function githubConnectUrl(workspaceSlug: string) {
  return `/api/v1/workspaces/${encodeURIComponent(workspaceSlug)}/github/start`;
}

export function useSyncGitHubInstallation(workspaceSlug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (installationId: number) =>
      api<GitHubRepositorySummary[]>(
        `/workspaces/${encodeURIComponent(
          workspaceSlug,
        )}/github/installations/${installationId}/sync`,
        { method: 'POST' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: ['workspace', workspaceSlug, 'github'],
      });
    },
  });
}
