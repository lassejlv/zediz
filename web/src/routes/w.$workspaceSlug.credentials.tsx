import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Github, Plus, RefreshCw } from 'lucide-react';
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
  githubConnectUrl,
  githubInstallationsQuery,
  useSyncGitHubInstallation,
} from '@/lib/github';
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
  github_pat: 'GitHub PAT',
  registry: 'Registry',
};

function CredentialsPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const creds = useQuery(credentialsQuery(workspaceSlug));
  const settings = usePublicSettings();
  const githubInstallations = useQuery({
    ...githubInstallationsQuery(workspaceSlug),
    enabled: !!settings.data?.github_app_configured,
  });
  const syncGithub = useSyncGitHubInstallation(workspaceSlug);
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

      {settings.data?.github_app_configured ? (
        <Card className="p-5">
          <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <div className="flex items-center gap-2">
                <Github className="h-4 w-4" />
                <h2 className="text-sm font-medium">GitHub App</h2>
              </div>
              <p className="mt-1 max-w-2xl text-xs leading-5 text-[var(--color-muted)]">
                Connect repositories to Driftbase for Git builds, push webhooks, and commit
                statuses. Existing PAT credentials still work as a fallback.
              </p>
            </div>
            <a href={githubConnectUrl(workspaceSlug)}>
              <Button>Connect GitHub</Button>
            </a>
          </div>
          {githubInstallations.data?.length ? (
            <div className="mt-4 divide-y divide-[var(--color-border)] rounded-md border border-[var(--color-border)]">
              {githubInstallations.data.map((installation) => (
                <div
                  key={installation.installation_id}
                  className="flex items-center justify-between gap-3 px-3 py-2"
                >
                  <div className="min-w-0">
                    <div className="truncate text-sm">{installation.account_login}</div>
                    <div className="text-[11px] text-[var(--color-muted)]">
                      {installation.account_type} · {installation.repository_selection} repos ·{' '}
                      {installation.active ? 'active' : 'inactive'}
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant="secondary"
                    disabled={syncGithub.isPending}
                    onClick={() => syncGithub.mutate(installation.installation_id)}
                  >
                    <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
                    Sync
                  </Button>
                </div>
              ))}
            </div>
          ) : (
            <p className="mt-4 text-xs text-[var(--color-muted)]">
              No GitHub installations connected yet.
            </p>
          )}
        </Card>
      ) : null}

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
          body="Add registry or GitHub credentials for private images and builds."
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
  const settings = usePublicSettings();
  const bundledRegistryHost = settings.data?.registry_site ?? null;

  const [kind, setKind] = useState<CredentialKind>('registry');
  const [name, setName] = useState('');
  const [secret, setSecret] = useState('');
  const [registryUrl, setRegistryUrl] = useState('');
  const [registryUsername, setRegistryUsername] = useState('');
  const [error, setError] = useState<string | null>(null);

  // When the sheet opens, pre-fill the URL with the bundled host. Users who
  // bring an external registry just overwrite it.
  const effectiveUrl = registryUrl.trim() || bundledRegistryHost || '';
  const urlIsBundled =
    bundledRegistryHost !== null &&
    hostFromUrl(effectiveUrl) === bundledRegistryHost.toLowerCase();

  function reset() {
    setName('');
    setSecret('');
    setRegistryUrl('');
    setRegistryUsername('');
    setKind('registry');
    setError(null);
  }

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      let metadata: Record<string, unknown> | undefined;
      if (kind === 'registry') {
        if (!effectiveUrl) {
          setError('Registry URL is required');
          return;
        }
        // For bundled-registry creds the server will overwrite username with
        // the credential id; we only need username for external registries
        // (GHCR, Docker Hub, user-hosted).
        if (!urlIsBundled && !registryUsername.trim()) {
          setError('Username is required for external registries');
          return;
        }
        metadata = {
          url: effectiveUrl,
          ...(urlIsBundled ? {} : { username: registryUsername.trim() }),
        };
      }
      await create.mutateAsync({ kind, name, secret, metadata });
      reset();
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
        if (!next) reset();
      }}
    >
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
          {kind === 'registry' ? (
            <>
              <Field
                label="Registry URL"
                htmlFor="cred-registry-url"
                hint={
                  bundledRegistryHost
                    ? `Defaults to the bundled registry (${bundledRegistryHost}). Change it for GHCR / Docker Hub / self-hosted.`
                    : 'e.g. ghcr.io or docker.io'
                }
              >
                <Input
                  id="cred-registry-url"
                  placeholder={bundledRegistryHost ?? 'ghcr.io'}
                  value={registryUrl}
                  onChange={(e) => setRegistryUrl(e.target.value)}
                />
              </Field>
              {urlIsBundled ? (
                <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-3 py-2 text-[11px] text-[var(--color-muted)]">
                  Username will be auto-generated (the credential's ULID) so
                  the built-in auth proxy can scope access to this workspace.
                </div>
              ) : (
                <Field
                  label="Username"
                  htmlFor="cred-registry-username"
                  hint="Registry login username (e.g. your GitHub username for GHCR)."
                >
                  <Input
                    id="cred-registry-username"
                    required
                    value={registryUsername}
                    onChange={(e) => setRegistryUsername(e.target.value)}
                  />
                </Field>
              )}
            </>
          ) : null}
          <Field
            label={kind === 'registry' ? 'Password' : 'Secret'}
            htmlFor="cred-secret"
          >
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

function hostFromUrl(url: string): string {
  return url
    .trim()
    .replace(/^https?:\/\//, '')
    .split('/')[0]
    .toLowerCase();
}
