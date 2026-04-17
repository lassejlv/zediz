import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState, type FormEvent } from 'react';
import {
  workspaceQuery,
  useUpdateWorkspace,
  type UpdateWorkspaceInput,
} from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input, PageHeader, Stack } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/settings')({
  component: SettingsPage,
});

const LOCATIONS = ['nbg1', 'fsn1', 'hel1', 'ash', 'hil', 'sin'];

function SettingsPage() {
  const { workspaceSlug } = Route.useParams();
  const ws = useQuery(workspaceQuery(workspaceSlug));
  const update = useUpdateWorkspace(workspaceSlug);

  const canManage = ws.data
    ? ws.data.role === 'owner' || ws.data.role === 'admin'
    : false;

  const [name, setName] = useState('');
  const [location, setLocation] = useState('nbg1');
  const [defaultServerType, setDefaultServerType] = useState('');
  const [maxNodes, setMaxNodes] = useState('3');
  const [maxMonthly, setMaxMonthly] = useState('50');
  const [idleTtl, setIdleTtl] = useState('600');
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!ws.data) return;
    setName(ws.data.name);
    setLocation(ws.data.hetzner_location ?? 'nbg1');
    setDefaultServerType(ws.data.default_server_type ?? '');
    setMaxNodes(String(ws.data.max_nodes ?? 3));
    setMaxMonthly(String(ws.data.max_monthly_euro ?? 50));
    setIdleTtl(String(ws.data.autoscale_idle_ttl_seconds ?? 600));
  }, [ws.data]);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    setSaved(false);
    const body: UpdateWorkspaceInput = {
      name: name.trim() || undefined,
      hetzner_location: location || undefined,
      default_server_type: defaultServerType.trim() || undefined,
      max_nodes: Number(maxNodes) || undefined,
      max_monthly_euro: Number(maxMonthly),
      autoscale_idle_ttl_seconds: Number(idleTtl) || undefined,
    };
    try {
      await update.mutateAsync(body);
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
        subtitle="Hetzner region, autoscale, and cost caps for this workspace."
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

          <Field label="Hetzner location" htmlFor="ws-location">
            <select
              id="ws-location"
              value={location}
              onChange={(e) => setLocation(e.target.value)}
              className="h-9 w-full rounded-md border border-[var(--color-border)] bg-transparent px-2 text-sm focus:border-[var(--color-accent)] focus:outline-none"
            >
              {LOCATIONS.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </Field>

          <Field
            label="Default server type"
            htmlFor="ws-server-type"
            hint="Leave empty to auto-pick the cheapest fit per request."
          >
            <Input
              id="ws-server-type"
              placeholder="cx22"
              value={defaultServerType}
              onChange={(e) => setDefaultServerType(e.target.value)}
            />
          </Field>

          <div className="grid grid-cols-3 gap-3">
            <Field label="Max nodes" htmlFor="ws-max-nodes">
              <Input
                id="ws-max-nodes"
                type="number"
                min={1}
                max={100}
                value={maxNodes}
                onChange={(e) => setMaxNodes(e.target.value)}
              />
            </Field>
            <Field label="Max monthly €" htmlFor="ws-max-monthly">
              <Input
                id="ws-max-monthly"
                type="number"
                min={0}
                value={maxMonthly}
                onChange={(e) => setMaxMonthly(e.target.value)}
              />
            </Field>
            <Field label="Idle TTL (s)" htmlFor="ws-idle-ttl">
              <Input
                id="ws-idle-ttl"
                type="number"
                min={60}
                max={86400}
                value={idleTtl}
                onChange={(e) => setIdleTtl(e.target.value)}
              />
            </Field>
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
