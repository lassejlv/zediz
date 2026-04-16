import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router';
import { useState, type FormEvent } from 'react';
import { meQuery, useSignup } from '@/lib/auth';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input } from '@/components/ui';

interface SignupSearch {
  invite?: string;
}

export const Route = createFileRoute('/signup')({
  validateSearch: (search: Record<string, unknown>): SignupSearch => ({
    invite: typeof search.invite === 'string' ? search.invite : undefined,
  }),
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (me) throw redirect({ to: '/' });
  },
  component: SignupPage,
});

function SignupPage() {
  const { invite } = Route.useSearch();
  const navigate = useNavigate();
  const signup = useSignup();
  const [email, setEmail] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await signup.mutateAsync({
        email,
        password,
        display_name: displayName,
        invite_token: invite,
      });
      await navigate({ to: '/' });
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <div className="mx-auto max-w-sm pt-10">
      <Card className="p-6">
        <h1 className="mb-5 text-lg font-semibold tracking-tight">Create your account</h1>
        <form onSubmit={onSubmit} className="space-y-4">
          <Field label="Display name" htmlFor="name">
            <Input
              id="name"
              autoComplete="name"
              required
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
            />
          </Field>
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
          <Field label="Password" htmlFor="password" hint="At least 8 characters.">
            <Input
              id="password"
              type="password"
              autoComplete="new-password"
              required
              minLength={8}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </Field>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" disabled={signup.isPending} className="w-full">
            {signup.isPending ? 'Creating…' : 'Create account'}
          </Button>
        </form>
        <p className="mt-4 text-xs text-[var(--color-muted)]">
          Already have an account?{' '}
          <Link to="/login" className="text-[var(--color-fg)] underline-offset-4 hover:underline">
            Sign in
          </Link>
        </p>
      </Card>
    </div>
  );
}
