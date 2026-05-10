import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type { NodeSummary } from './types';

export function nodesQuery(slug: string) {
  return queryOptions({
    queryKey: ['workspace', slug, 'nodes'] as const,
    queryFn: ({ signal }) =>
      api<NodeSummary[]>(`/workspaces/${encodeURIComponent(slug)}/nodes`, { signal }),
  });
}

export function useNodes(slug: string) {
  return useQuery(nodesQuery(slug));
}

export function useDrainNode(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (nodeId: string) =>
      api<void>(
        `/workspaces/${encodeURIComponent(slug)}/nodes/${encodeURIComponent(nodeId)}/drain`,
        { method: 'POST' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'nodes'] });
    },
  });
}

export function useDeleteNode(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: { nodeId: string; force?: boolean }) => {
      const q = input.force ? '?force=true' : '';
      return api<void>(
        `/workspaces/${encodeURIComponent(slug)}/nodes/${encodeURIComponent(input.nodeId)}${q}`,
        { method: 'DELETE' },
      );
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'nodes'] });
    },
  });
}

export interface AgentUpdateResponse {
  status: string;
  update_available: boolean;
  target_image_ref: string | null;
  target_digest: string | null;
  error: string | null;
  command_id: string | null;
}

export function useCheckNodeAgentUpdate(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (nodeId: string) =>
      api<AgentUpdateResponse>(
        `/workspaces/${encodeURIComponent(slug)}/nodes/${encodeURIComponent(nodeId)}/agent-update/check`,
        { method: 'POST' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'nodes'] });
    },
  });
}

export function useUpdateNodeAgent(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (nodeId: string) =>
      api<AgentUpdateResponse>(
        `/workspaces/${encodeURIComponent(slug)}/nodes/${encodeURIComponent(nodeId)}/agent-update`,
        { method: 'POST' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'nodes'] });
    },
  });
}

export interface ProvisionNodeInput {
  server_type?: string;
  location?: string;
}

export interface ProvisionNodeResponse {
  node_id: string;
  hetzner_server_id: number;
}

export function useProvisionNode(slug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: ProvisionNodeInput) =>
      api<ProvisionNodeResponse>(`/workspaces/${encodeURIComponent(slug)}/nodes`, {
        method: 'POST',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', slug, 'nodes'] });
    },
  });
}
