import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router';
import { useState, type FormEvent } from 'react';
import { meQuery, useLogin } from '@/lib/auth';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input } from '@/components/ui';

export const Route = createFileRoute('/login')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (me) throw redirect({ to: '/' });
  },
  component: LoginPage,
});

function LoginPage() {
  const navigate = useNavigate();
  const login = useLogin();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await login.mutateAsync({ email, password });
      await navigate({ to: '/' });
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <div className="mx-auto max-w-sm pt-10">
      <Card className="p-6">
        <h1 className="mb-5 text-lg font-semibold tracking-tight">Sign in to Driftbase</h1>
        <form onSubmit={onSubmit} className="space-y-4">
          <Field label="Email" htmlFor="email">
            <Input
              id="email"
              type="email"
              autoComplete="email"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
          <Field label="Password" htmlFor="password">
            <Input
              id="password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </Field>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" disabled={login.isPending} className="w-full">
            {login.isPending ? 'Signing in…' : 'Sign in'}
          </Button>
        </form>
        <p className="mt-4 text-xs text-[var(--color-muted)]">
          New here?{' '}
          <Link to="/signup" className="text-[var(--color-fg)] underline-offset-4 hover:underline">
            Create an account
          </Link>
        </p>
      </Card>
    </div>
  );
}
