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
}

export function useSignup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: SignupInput) => api<Me>('/auth/signup', { method: 'POST', body: input }),
    onSuccess: (me) => {
      qc.setQueryData(meQuery.queryKey, me);
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
