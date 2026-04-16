import { createFileRoute, Link, redirect } from '@tanstack/react-router';
import { meQuery } from '@/lib/auth';
import { workspacesQuery } from '@/lib/workspaces';
import { Button } from '@/components/ui';

export const Route = createFileRoute('/')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) {
      throw redirect({ to: '/login' });
    }
    const workspaces = await context.queryClient.ensureQueryData(workspacesQuery);
    if (workspaces.length > 0) {
      throw redirect({
        to: '/w/$workspaceSlug',
        params: { workspaceSlug: workspaces[0].slug },
      });
    }
  },
  component: EmptyState,
});

function EmptyState() {
  return (
    <section className="mx-auto max-w-md space-y-4 pt-16 text-center">
      <h1 className="text-2xl font-semibold tracking-tight">No workspaces yet</h1>
      <p className="text-sm text-[var(--color-muted)]">
        Create your first workspace to deploy services, invite teammates, and manage nodes.
      </p>
      <Link to="/workspaces/new">
        <Button>Create workspace</Button>
      </Link>
    </section>
  );
}
