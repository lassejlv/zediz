import { createFileRoute, redirect, useNavigate } from '@tanstack/react-router';
import { useEffect, useState } from 'react';
import { meQuery } from '@/lib/auth';
import { useAcceptInvite } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText } from '@/components/ui';

export const Route = createFileRoute('/invite/$token')({
  beforeLoad: async ({ context, params }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) {
      throw redirect({
        to: '/signup',
        search: { invite: params.token },
      });
    }
  },
  component: InvitePage,
});

function InvitePage() {
  const { token } = Route.useParams();
  const accept = useAcceptInvite();
  const navigate = useNavigate();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await accept.mutateAsync(token);
        if (!cancelled) {
          await navigate({
            to: '/w/$workspaceSlug',
            params: { workspaceSlug: res.workspace_slug },
          });
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof ApiError ? err.message : 'Failed to accept invite');
        }
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  return (
    <div className="mx-auto max-w-md pt-16">
      <Card className="space-y-3 p-6 text-center">
        {error ? (
          <>
            <h1 className="text-lg font-semibold">Invite could not be accepted</h1>
            <ErrorText>{error}</ErrorText>
            <Button variant="secondary" onClick={() => navigate({ to: '/' })}>
              Go home
            </Button>
          </>
        ) : (
          <p className="text-sm text-[var(--color-muted)]">Accepting invite…</p>
        )}
      </Card>
    </div>
  );
}
