import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState, type FormEvent } from 'react';
import {
  canAdmin,
  workspaceQuery,
  useUpdateWorkspace,
} from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input, PageHeader, Stack } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/settings')({
  component: SettingsPage,
});

function SettingsPage() {
  const { workspaceSlug } = Route.useParams();
  const ws = useQuery(workspaceQuery(workspaceSlug));
  const update = useUpdateWorkspace(workspaceSlug);

  const canManage = canAdmin(ws.data);

  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!ws.data) return;
    setName(ws.data.name);
  }, [ws.data]);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    setSaved(false);
    try {
      await update.mutateAsync({ name: name.trim() || undefined });
      setSaved(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Failed to save');
    }
  }

  if (!canManage) {
    return (
      <Stack gap={6}>
        <PageHeader title="Settings" />
        <Card className="p-5">
          <p className="text-sm text-[var(--color-muted)]">
            Only owners or admins can change workspace settings.
          </p>
        </Card>
      </Stack>
    );
  }

  return (
    <Stack gap={6}>
      <PageHeader
        title="Settings"
        subtitle="Workspace settings. Driftbase manages nodes and machine capacity."
      />

      <Card className="p-5">
        <form onSubmit={onSubmit} className="space-y-4">
          <Field label="Workspace name" htmlFor="ws-name">
            <Input
              id="ws-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>

          <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-3 py-2 text-xs text-[var(--color-muted)]">
            Regions are selected per project. Nodes, machine types, and autoscale
            limits are controlled by Driftbase.
          </div>

          {error ? <ErrorText>{error}</ErrorText> : null}
          {saved && !error ? (
            <p className="text-xs text-green-400">Saved.</p>
          ) : null}

          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? 'Saving…' : 'Save'}
          </Button>
        </form>
      </Card>
    </Stack>
  );
}
