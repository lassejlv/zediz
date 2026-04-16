import { createFileRoute, redirect, useNavigate } from '@tanstack/react-router';
import { useState, type FormEvent } from 'react';
import { meQuery } from '@/lib/auth';
import { useCreateWorkspace } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input } from '@/components/ui';

export const Route = createFileRoute('/workspaces/new')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) throw redirect({ to: '/login' });
  },
  component: NewWorkspacePage,
});

function NewWorkspacePage() {
  const navigate = useNavigate();
  const create = useCreateWorkspace();
  const [name, setName] = useState('');
  const [slug, setSlug] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const ws = await create.mutateAsync({ slug, name });
      await navigate({ to: '/w/$workspaceSlug', params: { workspaceSlug: ws.slug } });
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <div className="mx-auto max-w-md pt-10">
      <Card className="p-6">
        <h1 className="mb-5 text-lg font-semibold tracking-tight">New workspace</h1>
        <form onSubmit={onSubmit} className="space-y-4">
          <Field label="Name" htmlFor="name">
            <Input
              id="name"
              required
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                if (!slug) setSlug(slugify(e.target.value));
              }}
            />
          </Field>
          <Field label="Slug" htmlFor="slug" hint="Lowercase letters, digits, and dashes.">
            <Input
              id="slug"
              required
              value={slug}
              onChange={(e) => setSlug(e.target.value)}
            />
          </Field>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <Button type="submit" disabled={create.isPending} className="w-full">
            {create.isPending ? 'Creating…' : 'Create workspace'}
          </Button>
        </form>
      </Card>
    </div>
  );
}

function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40);
}
