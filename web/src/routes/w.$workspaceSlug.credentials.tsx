import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Plus } from 'lucide-react';
import {
  credentialsQuery,
  useCreateCredential,
  useDeleteCredential,
} from '@/lib/credentials';
import { canAdmin, workspaceQuery } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import type { CredentialKind } from '@/lib/types';
import {
  Button,
  Card,
  ErrorText,
  Field,
  Input,
  Select,
  PageHeader,
  Stack,
  EmptyState,
  RelativeTime,
  CopyableId,
} from '@/components/ui';
import { usePublicSettings } from '@/lib/settings';
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/sheet';

export const Route = createFileRoute('/w/$workspaceSlug/credentials')({
  component: CredentialsPage,
});

const KIND_LABEL: Record<CredentialKind, string> = {
  hetzner_api_token: 'Hetzner API token',
  github_pat: 'GitHub PAT',
  registry: 'Registry',
};

function CredentialsPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const creds = useQuery(credentialsQuery(workspaceSlug));
  const settings = usePublicSettings();
  const del = useDeleteCredential(workspaceSlug);

  const canManage = canAdmin(workspace.data);
  const bundledRegistryHost = settings.data?.registry_site ?? null;

  const [sheetOpen, setSheetOpen] = useState(false);

  if (!canManage) {
    return (
      <Stack gap={6}>
        <PageHeader title="Credentials" />
        <Card className="p-5">
          <p className="text-sm text-[var(--color-muted)]">
            You need admin or owner role to view credentials.
          </p>
        </Card>
      </Stack>
    );
  }

  return (
    <Stack gap={6}>
      <PageHeader
        title="Credentials"
        subtitle="Encrypted at rest. Secrets are never shown again after creation."
        actions={
          <Button onClick={() => setSheetOpen(true)}>
            <Plus className="mr-1 h-3.5 w-3.5" /> Add credential
          </Button>
        }
      />

      {creds.data?.length ? (
        <Card className="overflow-hidden">
          <table className="w-full text-sm">
            <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
              <tr>
                <th className="px-4 py-2.5 font-medium">Kind</th>
                <th className="px-4 py-2.5 font-medium">Name</th>
                <th className="px-4 py-2.5 font-medium">Added</th>
                <th className="px-4 py-2.5" />
              </tr>
            </thead>
            <tbody>
              {creds.data.map((c) => {
                const bundled =
                  c.kind === 'registry' &&
                  bundledRegistryHost !== null &&
                  registryHostMatches(c.metadata, bundledRegistryHost);
                return (
                  <tr key={c.id} className="border-t border-[var(--color-border)]">
                    <td className="px-4 py-2 font-mono text-xs">{KIND_LABEL[c.kind]}</td>
                    <td className="px-4 py-2">
                      <div>{c.name}</div>
                      {bundled ? (
                        <div className="mt-1 text-[11px] text-[var(--color-muted)]">
                          <span>docker login user: </span>
                          <CopyableId value={c.id} />
                        </div>
                      ) : null}
                    </td>
                    <td className="px-4 py-2 text-xs text-[var(--color-muted)]">
                      <RelativeTime date={c.created_at} />
                    </td>
                    <td className="px-4 py-2 text-right">
                      <Button
                        variant="danger"
                        onClick={() => {
                          if (confirm(`Delete ${c.name}?`)) del.mutate(c.id);
                        }}
                      >
                        Delete
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>
      ) : (
        <EmptyState
          title="No credentials"
          body="Add a Hetzner API token to provision nodes, or a registry credential to pull private images."
          cta={
            <Button onClick={() => setSheetOpen(true)}>
              <Plus className="mr-1 h-3.5 w-3.5" /> Add credential
            </Button>
          }
        />
      )}

      <AddCredentialSheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        workspaceSlug={workspaceSlug}
      />
    </Stack>
  );
}

/** True when a credential's `metadata.url` points at the bundled registry. */
function registryHostMatches(
  metadata: Record<string, unknown> | null | undefined,
  bundledHost: string,
): boolean {
  const url = metadata && typeof metadata === 'object' ? (metadata as { url?: unknown }).url : null;
  if (typeof url !== 'string') return false;
  const host = url
    .trim()
    .replace(/^https?:\/\//, '')
    .split('/')[0]
    .toLowerCase();
  return host === bundledHost.toLowerCase();
}

function AddCredentialSheet({
  open,
  onOpenChange,
  workspaceSlug,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
}) {
  const create = useCreateCredential(workspaceSlug);
  const [kind, setKind] = useState<CredentialKind>('hetzner_api_token');
  const [name, setName] = useState('');
  const [secret, setSecret] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await create.mutateAsync({ kind, name, secret });
      setName('');
      setSecret('');
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>Add credential</SheetTitle>
          <SheetDescription>
            Secrets are encrypted with your workspace master key. The plain value cannot be
            retrieved after save.
          </SheetDescription>
        </SheetHeader>
        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-4">
          <Field label="Kind" htmlFor="cred-kind">
            <Select
              id="cred-kind"
              value={kind}
              onChange={(e) => setKind(e.target.value as CredentialKind)}
            >
              <option value="hetzner_api_token">Hetzner API token</option>
              <option value="github_pat">GitHub PAT</option>
              <option value="registry">Registry</option>
            </Select>
          </Field>
          <Field label="Name" htmlFor="cred-name">
            <Input
              id="cred-name"
              required
              placeholder="production"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label="Secret" htmlFor="cred-secret">
            <Input
              id="cred-secret"
              type="password"
              required
              autoComplete="off"
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
            />
          </Field>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <SheetFooter>
            <SheetClose asChild>
              <Button type="button" variant="ghost">
                Cancel
              </Button>
            </SheetClose>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? 'Saving…' : 'Save'}
            </Button>
          </SheetFooter>
        </form>
      </SheetContent>
    </Sheet>
  );
}
