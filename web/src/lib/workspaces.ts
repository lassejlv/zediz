import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type { CreatedInvite, InviteSummary, MemberRow, Role, WorkspaceSummary } from './types';

/**
 * True when the caller has write permission in the workspace: can create
 * projects, deploy services, add domains. Viewers are excluded.
 */
export function canWrite(ws: { role: Role } | undefined | null): boolean {
  return !!ws && ws.role !== 'viewer';
}

/**
 * True when the caller can administer the workspace: provision nodes, manage
 * members, rotate credentials. Only owners and admins.
 */
export function canAdmin(ws: { role: Role } | undefined | null): boolean {
  return !!ws && (ws.role === 'owner' || ws.role === 'admin');
}

export const workspacesQuery = queryOptions({
  queryKey: ['workspaces'] as const,
  queryFn: ({ signal }) => api<WorkspaceSummary[]>('/workspaces', { signal }),
  staleTime: 30_000,
});

export function useWorkspaces() {
  return useQuery(workspacesQuery);
}

export function workspaceQuery(slug: string) {
  return queryOptions({
    queryKey: ['workspace', slug] as const,
    queryFn: ({ signal }) =>
      api<WorkspaceSummary>(`/workspaces/${encodeURIComponent(slug)}`, { signal }),
  });
}

export function useCreateWorkspace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { slug: string; name: string }) =>
      api<WorkspaceSummary>('/workspaces', { method: 'POST', body: input }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: workspacesQuery.queryKey });
    },
  });
}

export interface UpdateWorkspaceInput {
  name?: string;
  hetzner_location?: string;
  default_server_type?: string;
  max_nodes?: number;
  max_monthly_euro?: number;
  autoscale_idle_ttl_seconds?: number;
}

export function useUpdateWorkspace(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: UpdateWorkspaceInput) =>
      api<WorkspaceSummary>(`/workspaces/${encodeURIComponent(slug)}`, {
        method: 'PATCH',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug] });
      qc.invalidateQueries({ queryKey: workspacesQuery.queryKey });
    },
  });
}

export function membersQuery(slug: string) {
  return queryOptions({
    queryKey: ['workspace', slug, 'members'] as const,
    queryFn: ({ signal }) =>
      api<MemberRow[]>(`/workspaces/${encodeURIComponent(slug)}/members`, { signal }),
  });
}

export function invitesQuery(slug: string) {
  return queryOptions({
    queryKey: ['workspace', slug, 'invites'] as const,
    queryFn: ({ signal }) =>
      api<InviteSummary[]>(`/workspaces/${encodeURIComponent(slug)}/invites`, { signal }),
  });
}

export function useCreateInvite(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { email: string; role: Role }) =>
      api<CreatedInvite>(`/workspaces/${encodeURIComponent(slug)}/invites`, {
        method: 'POST',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'invites'] });
    },
  });
}

export function useRevokeInvite(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/workspaces/${encodeURIComponent(slug)}/invites/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'invites'] });
    },
  });
}

export function useUpdateMemberRole(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { user_id: string; role: Role }) =>
      api<void>(
        `/workspaces/${encodeURIComponent(slug)}/members/${encodeURIComponent(input.user_id)}`,
        { method: 'PATCH', body: { role: input.role } },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'members'] });
    },
  });
}

export function useRemoveMember(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (user_id: string) =>
      api<void>(
        `/workspaces/${encodeURIComponent(slug)}/members/${encodeURIComponent(user_id)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'members'] });
    },
  });
}

export function useAcceptInvite() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (token: string) =>
      api<{ workspace_slug: string }>(`/invites/${encodeURIComponent(token)}/accept`, {
        method: 'POST',
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: workspacesQuery.queryKey });
    },
  });
}
