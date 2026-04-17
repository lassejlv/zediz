import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Plus } from 'lucide-react';
import {
  invitesQuery,
  membersQuery,
  useCreateInvite,
  useRemoveMember,
  useRevokeInvite,
  useUpdateMemberRole,
  workspaceQuery,
} from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import type { Role } from '@/lib/types';
import {
  Button,
  Card,
  ErrorText,
  Field,
  Input,
  Select,
  PageHeader,
  Stack,
  CopyableId,
  RelativeTime,
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

export const Route = createFileRoute('/w/$workspaceSlug/members')({
  component: MembersPage,
});

function MembersPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const members = useQuery(membersQuery(workspaceSlug));
  const invites = useQuery(invitesQuery(workspaceSlug));
  const revoke = useRevokeInvite(workspaceSlug);
  const updateRole = useUpdateMemberRole(workspaceSlug);
  const removeMember = useRemoveMember(workspaceSlug);

  const canManage = workspace.data
    ? workspace.data.role === 'owner' || workspace.data.role === 'admin'
    : false;

  const [sheetOpen, setSheetOpen] = useState(false);

  return (
    <Stack gap={6}>
      <PageHeader
        title="Members"
        subtitle="Invite teammates and manage their roles."
        actions={
          canManage ? (
            <Button onClick={() => setSheetOpen(true)}>
              <Plus className="mr-1 h-3.5 w-3.5" /> Invite
            </Button>
          ) : null
        }
      />

      <Card className="overflow-hidden">
        <table className="w-full text-sm">
          <thead className="text-left text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            <tr>
              <th className="px-4 py-2.5 font-medium">Name</th>
              <th className="px-4 py-2.5 font-medium">Email</th>
              <th className="px-4 py-2.5 font-medium">Role</th>
              <th className="px-4 py-2.5" />
            </tr>
          </thead>
          <tbody>
            {members.data?.map((m) => (
              <tr key={m.user_id} className="border-t border-[var(--color-border)]">
                <td className="px-4 py-2">{m.display_name}</td>
                <td className="px-4 py-2 text-[var(--color-muted)]">{m.email}</td>
                <td className="px-4 py-2">
                  {canManage && m.role !== 'owner' ? (
                    <Select
                      value={m.role}
                      onChange={(e) =>
                        updateRole.mutate({ user_id: m.user_id, role: e.target.value as Role })
                      }
                      className="h-7 w-auto"
                    >
                      <option value="admin">admin</option>
                      <option value="member">member</option>
                      <option value="viewer">viewer</option>
                    </Select>
                  ) : (
                    <span className="font-mono text-xs">{m.role}</span>
                  )}
                </td>
                <td className="px-4 py-2 text-right">
                  {canManage && m.role !== 'owner' ? (
                    <Button
                      variant="danger"
                      onClick={() => {
                        if (confirm(`Remove ${m.email}?`)) removeMember.mutate(m.user_id);
                      }}
                    >
                      Remove
                    </Button>
                  ) : null}
                </td>
              </tr>
            ))}
            {!members.data?.length ? (
              <tr>
                <td colSpan={4} className="px-4 py-6 text-center text-sm text-[var(--color-muted)]">
                  No members.
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </Card>

      {canManage && invites.data && invites.data.length > 0 ? (
        <Card className="overflow-hidden">
          <div className="border-b border-[var(--color-border)] px-4 py-2.5 text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            Pending invites
          </div>
          <table className="w-full text-sm">
            <tbody>
              {invites.data.map((i) => (
                <tr key={i.id} className="border-t border-[var(--color-border)]">
                  <td className="px-4 py-2">{i.email}</td>
                  <td className="px-4 py-2 font-mono text-xs">{i.role}</td>
                  <td className="px-4 py-2 text-xs text-[var(--color-muted)]">
                    expires <RelativeTime date={i.expires_at} />
                  </td>
                  <td className="px-4 py-2 text-right">
                    <Button variant="danger" onClick={() => revoke.mutate(i.id)}>
                      Revoke
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      ) : null}

      <InviteSheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        workspaceSlug={workspaceSlug}
      />
    </Stack>
  );
}

function InviteSheet({
  open,
  onOpenChange,
  workspaceSlug,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
}) {
  const invite = useCreateInvite(workspaceSlug);
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<Role>('member');
  const [error, setError] = useState<string | null>(null);
  const [acceptUrl, setAcceptUrl] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const created = await invite.mutateAsync({ email, role });
      setEmail('');
      setAcceptUrl(created.accept_url);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Sheet
      open={open}
      onOpenChange={(v) => {
        if (!v) setAcceptUrl(null);
        onOpenChange(v);
      }}
    >
      <SheetContent>
        <SheetHeader>
          <SheetTitle>Invite a member</SheetTitle>
          <SheetDescription>
            The invite link appears after sending — share it directly with the recipient.
          </SheetDescription>
        </SheetHeader>

        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-4">
          <Field label="Email" htmlFor="invite-email">
            <Input
              id="invite-email"
              type="email"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
          <Field label="Role" htmlFor="invite-role">
            <Select
              id="invite-role"
              value={role}
              onChange={(e) => setRole(e.target.value as Role)}
            >
              <option value="admin">admin — can provision nodes and manage members</option>
              <option value="member">member — can deploy services</option>
              <option value="viewer">viewer — read-only</option>
            </Select>
          </Field>
          {error ? <ErrorText>{error}</ErrorText> : null}
          {acceptUrl ? (
            <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-3 py-2 text-xs">
              <div className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
                Share this link
              </div>
              <div className="mt-1">
                <CopyableId value={acceptUrl} />
              </div>
            </div>
          ) : null}
          <SheetFooter>
            <SheetClose asChild>
              <Button type="button" variant="ghost">
                Close
              </Button>
            </SheetClose>
            <Button type="submit" disabled={invite.isPending}>
              {invite.isPending ? 'Sending…' : 'Send invite'}
            </Button>
          </SheetFooter>
        </form>
      </SheetContent>
    </Sheet>
  );
}
