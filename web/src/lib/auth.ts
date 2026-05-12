import { queryOptions, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, ApiError } from './api';
import type { Me } from './types';

export const meQuery = queryOptions({
  queryKey: ['auth', 'me'] as const,
  queryFn: async ({ signal }): Promise<Me | null> => {
    try {
      return await api<Me>('/auth/me', { signal });
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) return null;
      throw e;
    }
  },
  staleTime: 30_000,
});

export function useMe() {
  return useQuery(meQuery);
}

export interface LoginInput {
  email: string;
  password: string;
}

export function useLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: LoginInput) => api<Me>('/auth/login', { method: 'POST', body: input }),
    onSuccess: (me) => {
      qc.setQueryData(meQuery.queryKey, me);
    },
  });
}

export interface SignupInput extends LoginInput {
  display_name: string;
  invite_token?: string;
  setup_token?: string;
}

/** Shape of POST /auth/signup. `pending: true` means the account was
 * created but is waiting for platform-admin approval — no session is
 * issued, so the UI should show a "pending" screen instead of
 * redirecting into the app. */
export type SignupResult = Me | { pending: true; email: string };

export function isPendingSignup(r: SignupResult): r is { pending: true; email: string } {
  return 'pending' in r && r.pending === true;
}

export function useSignup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: SignupInput) =>
      api<SignupResult>('/auth/signup', { method: 'POST', body: input }),
    onSuccess: (result) => {
      if (!isPendingSignup(result)) {
        qc.setQueryData(meQuery.queryKey, result);
      }
    },
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api<void>('/auth/logout', { method: 'POST' }),
    onSuccess: () => {
      qc.setQueryData(meQuery.queryKey, null);
      qc.invalidateQueries();
    },
  });
}
