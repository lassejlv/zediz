import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './api';
import type {
  DeploymentSummary,
  EnvVars,
  PortMap,
  Resources,
  RestartPolicy,
  ServiceSummary,
} from './types';

function serviceBase(workspaceSlug: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}/services`;
}

export function servicesQuery(workspaceSlug: string, projectSlug: string) {
  return queryOptions({
    queryKey: ['workspace', workspaceSlug, 'project', projectSlug, 'services'] as const,
    queryFn: ({ signal }) =>
      api<ServiceSummary[]>(serviceBase(workspaceSlug, projectSlug), { signal }),
  });
}

export function useServices(workspaceSlug: string, projectSlug: string) {
  return useQuery(servicesQuery(workspaceSlug, projectSlug));
}

export function serviceQuery(
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
    ] as const,
    queryFn: ({ signal }) =>
      api<ServiceSummary>(
        `${serviceBase(workspaceSlug, projectSlug)}/${encodeURIComponent(serviceSlug)}`,
        { signal },
      ),
  });
}

export interface CreateServiceInput {
  slug: string;
  name: string;
  source?: 'image' | 'git';
  image_ref?: string;
  env_vars?: EnvVars;
  ports?: PortMap[];
  resources?: Resources;
  replicas?: number;
  restart_policy?: RestartPolicy;
  git_repo?: string;
  git_branch?: string;
  dockerfile_path?: string;
  root_dir?: string;
  builder?: 'dockerfile' | 'railpack';
  registry_repo?: string;
  github_credential_id?: string;
  registry_credential_id?: string;
}

export function useCreateService(workspaceSlug: string, projectSlug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateServiceInput) =>
      api<ServiceSummary>(serviceBase(workspaceSlug, projectSlug), {
        method: 'POST',
        body: input,
      }),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: ['workspace', workspaceSlug, 'project', projectSlug, 'services'],
      });
    },
  });
}

export function useDeleteService(workspaceSlug: string, projectSlug: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (serviceSlug: string) =>
      api<void>(
        `${serviceBase(workspaceSlug, projectSlug)}/${encodeURIComponent(serviceSlug)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: ['workspace', workspaceSlug, 'project', projectSlug, 'services'],
      });
    },
  });
}

export function useDeployService(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () =>
      api<DeploymentSummary>(
        `${serviceBase(workspaceSlug, projectSlug)}/${encodeURIComponent(serviceSlug)}/deploy`,
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
          'deployments',
        ],
      });
    },
  });
}

export function serviceDeploymentsQuery(
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
      'deployments',
    ] as const,
    queryFn: ({ signal }) =>
      api<DeploymentSummary[]>(
        `${serviceBase(workspaceSlug, projectSlug)}/${encodeURIComponent(serviceSlug)}/deployments`,
        { signal },
      ),
  });
}
