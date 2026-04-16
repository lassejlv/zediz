import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
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
import { Button, Card, ErrorText, Field, Input, Select } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/members')({
  component: MembersPage,
});

function MembersPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const members = useQuery(membersQuery(workspaceSlug));
  const invites = useQuery(invitesQuery(workspaceSlug));
  const invite = useCreateInvite(workspaceSlug);
  const revoke = useRevokeInvite(workspaceSlug);
  const updateRole = useUpdateMemberRole(workspaceSlug);
  const removeMember = useRemoveMember(workspaceSlug);

  const canManage = workspace.data
    ? workspace.data.role === 'owner' || workspace.data.role === 'admin'
    : false;

  const [inviteEmail, setInviteEmail] = useState('');
  const [inviteRole, setInviteRole] = useState<Role>('member');
  const [inviteError, setInviteError] = useState<string | null>(null);
  const [lastAcceptUrl, setLastAcceptUrl] = useState<string | null>(null);

  async function onInvite(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setInviteError(null);
    setLastAcceptUrl(null);
    try {
      const created = await invite.mutateAsync({ email: inviteEmail, role: inviteRole });
      setInviteEmail('');
      setLastAcceptUrl(created.accept_url);
    } catch (err) {
      setInviteError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <section className="space-y-8">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Members</h1>
        <p className="mt-1 text-sm text-[var(--color-muted)]">
          Invite teammates and manage their roles.
        </p>
      </div>

      {canManage ? (
        <Card className="p-5">
          <h2 className="mb-4 text-sm font-medium">Invite someone</h2>
          <form onSubmit={onInvite} className="grid grid-cols-[1fr_140px_auto] items-end gap-3">
            <Field label="Email" htmlFor="invite-email">
              <Input
                id="invite-email"
                type="email"
                required
                value={inviteEmail}
                onChange={(e) => setInviteEmail(e.target.value)}
              />
            </Field>
            <Field label="Role" htmlFor="invite-role">
              <Select
                id="invite-role"
                value={inviteRole}
                onChange={(e) => setInviteRole(e.target.value as Role)}
              >
                <option value="admin">admin</option>
                <option value="member">member</option>
                <option value="viewer">viewer</option>
              </Select>
            </Field>
            <Button type="submit" disabled={invite.isPending}>
              {invite.isPending ? 'Sending…' : 'Send invite'}
            </Button>
          </form>
          {inviteError ? <div className="mt-3"><ErrorText>{inviteError}</ErrorText></div> : null}
          {lastAcceptUrl ? (
            <p className="mt-3 text-xs text-[var(--color-muted)]">
              Share link:{' '}
              <code className="break-all text-[var(--color-fg)]">{lastAcceptUrl}</code>
            </p>
          ) : null}
        </Card>
      ) : null}

      <Card className="overflow-hidden">
        <table className="w-full text-sm">
          <thead className="text-left text-xs uppercase tracking-wider text-[var(--color-muted)]">
            <tr>
              <th className="px-4 py-2 font-medium">Name</th>
              <th className="px-4 py-2 font-medium">Email</th>
              <th className="px-4 py-2 font-medium">Role</th>
              <th className="px-4 py-2" />
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
          </tbody>
        </table>
      </Card>

      {canManage && invites.data && invites.data.length > 0 ? (
        <Card className="overflow-hidden">
          <div className="border-b border-[var(--color-border)] px-4 py-2 text-xs uppercase tracking-wider text-[var(--color-muted)]">
            Pending invites
          </div>
          <table className="w-full text-sm">
            <tbody>
              {invites.data.map((i) => (
                <tr key={i.id} className="border-t border-[var(--color-border)]">
                  <td className="px-4 py-2">{i.email}</td>
                  <td className="px-4 py-2 font-mono text-xs">{i.role}</td>
                  <td className="px-4 py-2 text-[var(--color-muted)]">
                    expires {new Date(i.expires_at).toLocaleDateString()}
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
    </section>
  );
}
