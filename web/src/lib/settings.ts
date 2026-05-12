import { queryOptions, useQuery } from '@tanstack/react-query';
import { api } from './api';

/** Non-secret server-side config exposed to the frontend. */
export interface PublicSettings {
  registry_site: string | null;
  github_app_configured: boolean;
  github_app_slug: string | null;
}

export function publicSettingsQuery() {
  return queryOptions({
    queryKey: ['public-settings'] as const,
    queryFn: ({ signal }) => api<PublicSettings>('/public-settings', { signal }),
    staleTime: 5 * 60_000,
  });
}

export function usePublicSettings() {
  return useQuery(publicSettingsQuery());
}
