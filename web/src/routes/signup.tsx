import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router';
import { useState, type FormEvent } from 'react';
import { Check } from 'lucide-react';
import { isPendingSignup, meQuery, useSignup } from '@/lib/auth';
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
  const [setupToken, setSetupToken] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [pendingEmail, setPendingEmail] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const result = await signup.mutateAsync({
        email,
        password,
        display_name: displayName,
        invite_token: invite,
        setup_token: setupToken.trim() || undefined,
      });
      if (isPendingSignup(result)) {
        setPendingEmail(result.email);
        return;
      }
      await navigate({ to: '/' });
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  if (pendingEmail) {
    return (
      <div className="mx-auto max-w-sm pt-10">
        <Card className="p-6 text-center">
          <div className="mx-auto mb-3 inline-flex h-10 w-10 items-center justify-center rounded-full border border-emerald-500/40 text-emerald-500">
            <Check className="h-5 w-5" />
          </div>
          <h1 className="text-lg font-semibold tracking-tight">You're on the waitlist</h1>
          <p className="mt-2 text-sm text-[var(--color-muted)]">
            We've created an account for{' '}
            <span className="font-mono text-[var(--color-fg)]">{pendingEmail}</span>. Sign-in
            will start working once an administrator approves it.
          </p>
          <div className="mt-5">
            <Link to="/login">
              <Button variant="secondary" className="w-full">
                Back to sign in
              </Button>
            </Link>
          </div>
        </Card>
      </div>
    );
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
          <Field label="Setup token" htmlFor="setup-token" hint="Only needed on locked installs.">
            <Input
              id="setup-token"
              type="password"
              autoComplete="off"
              value={setupToken}
              onChange={(e) => setSetupToken(e.target.value)}
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
        <p className="mt-3 text-[11px] leading-relaxed text-[var(--color-subtle)]">
          New accounts are held for admin approval before sign-in works.
        </p>
      </Card>
    </div>
  );
}
