import { queryOptions, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './api';

export type VolumeStatus =
  | 'creating'
  | 'available'
  | 'attached'
  | 'detaching'
  | 'errored';

export interface VolumeSummary {
  id: string;
  workspace_id: string;
  name: string;
  size_gb: number;
  hetzner_volume_id: number | null;
  hetzner_location: string;
  attached_node_id: string | null;
  attached_service_id: string | null;
  mount_path: string | null;
  status: VolumeStatus;
  reason: string | null;
  created_at: string;
  updated_at: string;
}

export function workspaceVolumesQuery(workspaceSlug: string) {
  return queryOptions({
    queryKey: ['workspace', workspaceSlug, 'volumes'] as const,
    queryFn: ({ signal }) =>
      api<VolumeSummary[]>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/volumes`,
        { signal },
      ),
  });
}

export function serviceVolumeQuery(
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
      'volume',
    ] as const,
    queryFn: ({ signal }) =>
      api<VolumeSummary | null>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(
          projectSlug,
        )}/services/${encodeURIComponent(serviceSlug)}/volume`,
        { signal },
      ),
  });
}

export interface CreateVolumeInput {
  name: string;
  size_gb: number;
}

export function useCreateVolume(workspaceSlug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateVolumeInput) =>
      api<VolumeSummary>(`/workspaces/${encodeURIComponent(workspaceSlug)}/volumes`, {
        method: 'POST',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', workspaceSlug, 'volumes'] });
    },
  });
}

export function useDeleteVolume(workspaceSlug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (volumeId: string) =>
      api<void>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/volumes/${encodeURIComponent(volumeId)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', workspaceSlug, 'volumes'] });
    },
  });
}

export interface AttachVolumeInput {
  volume_id: string;
  mount_path: string;
}

export function useAttachVolume(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: AttachVolumeInput) =>
      api<VolumeSummary>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(
          projectSlug,
        )}/services/${encodeURIComponent(serviceSlug)}/volume`,
        { method: 'POST', body: input },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', workspaceSlug, 'volumes'] });
      qc.invalidateQueries({
        queryKey: [
          'workspace',
          workspaceSlug,
          'project',
          projectSlug,
          'service',
          serviceSlug,
          'volume',
        ],
      });
    },
  });
}

export function useDetachVolume(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () =>
      api<void>(
        `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(
          projectSlug,
        )}/services/${encodeURIComponent(serviceSlug)}/volume`,
        { method: 'DELETE' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['workspace', workspaceSlug, 'volumes'] });
      qc.invalidateQueries({
        queryKey: [
          'workspace',
          workspaceSlug,
          'project',
          projectSlug,
          'service',
          serviceSlug,
          'volume',
        ],
      });
    },
  });
}
