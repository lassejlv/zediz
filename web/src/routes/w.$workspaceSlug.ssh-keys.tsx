import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Plus } from 'lucide-react';
import { sshKeysQuery, useCreateSshKey, useDeleteSshKey } from '@/lib/ssh-keys';
import { canAdmin, workspaceQuery } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import {
  Button,
  Card,
  ErrorText,
  Field,
  Input,
  PageHeader,
  Stack,
  EmptyState,
  CopyableId,
  StatusPill,
} from '@/components/ui';
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/sheet';

export const Route = createFileRoute('/w/$workspaceSlug/ssh-keys')({
  component: SshKeysPage,
});

function SshKeysPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const keys = useQuery(sshKeysQuery(workspaceSlug));
  const del = useDeleteSshKey(workspaceSlug);

  const canManage = canAdmin(workspace.data);

  const [sheetOpen, setSheetOpen] = useState(false);

  if (!canManage) {
    return (
      <Stack gap={6}>
        <PageHeader title="SSH keys" />
        <Card className="p-5">
          <p className="text-sm text-[var(--color-muted)]">
            You need admin or owner role to view SSH keys.
          </p>
        </Card>
      </Stack>
    );
  }

  return (
    <Stack gap={6}>
      <PageHeader
        title="SSH keys"
        subtitle="Public keys upload to Hetzner when provisioning. Private keys are encrypted and never shown back."
        actions={
          <Button onClick={() => setSheetOpen(true)}>
            <Plus className="mr-1 h-3.5 w-3.5" /> Add key
          </Button>
        }
      />

      {keys.data?.length ? (
        <Card className="overflow-hidden">
          <table className="w-full text-sm">
            <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
              <tr>
                <th className="px-4 py-2.5 font-medium">Name</th>
                <th className="px-4 py-2.5 font-medium">Fingerprint</th>
                <th className="px-4 py-2.5 font-medium">Private</th>
                <th className="px-4 py-2.5" />
              </tr>
            </thead>
            <tbody>
              {keys.data.map((k) => (
                <tr key={k.id} className="border-t border-[var(--color-border)]">
                  <td className="px-4 py-2">{k.name}</td>
                  <td className="px-4 py-2">
                    <CopyableId value={k.fingerprint} />
                  </td>
                  <td className="px-4 py-2">
                    {k.has_private_key ? (
                      <StatusPill status="ok" label="stored" />
                    ) : (
                      <span className="text-xs text-[var(--color-muted)]">—</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-right">
                    <Button
                      variant="danger"
                      onClick={() => {
                        if (confirm(`Delete ${k.name}?`)) del.mutate(k.id);
                      }}
                    >
                      Delete
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : (
        <EmptyState
          title="No SSH keys"
          body="Add at least one public key so you can SSH into provisioned nodes."
          cta={
            <Button onClick={() => setSheetOpen(true)}>
              <Plus className="mr-1 h-3.5 w-3.5" /> Add key
            </Button>
          }
        />
      )}

      <AddKeySheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        workspaceSlug={workspaceSlug}
      />
    </Stack>
  );
}

function AddKeySheet({
  open,
  onOpenChange,
  workspaceSlug,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
}) {
  const create = useCreateSshKey(workspaceSlug);
  const [name, setName] = useState('');
  const [publicKey, setPublicKey] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await create.mutateAsync({
        name,
        public_key: publicKey,
        private_key: privateKey.trim() ? privateKey : undefined,
      });
      setName('');
      setPublicKey('');
      setPrivateKey('');
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>Add SSH key</SheetTitle>
          <SheetDescription>
            Public keys are used when provisioning Hetzner VMs. Private key is optional.
          </SheetDescription>
        </SheetHeader>
        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-4">
          <Field label="Name" htmlFor="key-name">
            <Input
              id="key-name"
              required
              placeholder="deploy-key"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field
            label="Public key"
            htmlFor="key-public"
            hint="OpenSSH format, e.g. ssh-ed25519 AAAA… comment"
          >
            <textarea
              id="key-public"
              required
              rows={3}
              value={publicKey}
              onChange={(e) => setPublicKey(e.target.value)}
              className="w-full rounded-md border border-[var(--color-border)] bg-transparent p-2 font-mono text-xs focus:border-[var(--color-accent)] focus:outline-none"
            />
          </Field>
          <Field
            label="Private key (optional)"
            htmlFor="key-private"
            hint="PEM format. Stored encrypted."
          >
            <textarea
              id="key-private"
              rows={4}
              value={privateKey}
              onChange={(e) => setPrivateKey(e.target.value)}
              className="w-full rounded-md border border-[var(--color-border)] bg-transparent p-2 font-mono text-xs focus:border-[var(--color-accent)] focus:outline-none"
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
              {create.isPending ? 'Saving…' : 'Save key'}
            </Button>
          </SheetFooter>
        </form>
      </SheetContent>
    </Sheet>
  );
}
