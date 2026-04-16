import { createFileRoute, Link, Outlet, redirect } from '@tanstack/react-router';
import { meQuery } from '@/lib/auth';
import { useWorkspaces, workspaceQuery } from '@/lib/workspaces';

export const Route = createFileRoute('/w/$workspaceSlug')({
  beforeLoad: async ({ context, params }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) throw redirect({ to: '/login' });
    await context.queryClient.ensureQueryData(workspaceQuery(params.workspaceSlug));
  },
  component: WorkspaceLayout,
});

function WorkspaceLayout() {
  const { workspaceSlug } = Route.useParams();
  const workspaces = useWorkspaces();
  const current = workspaces.data?.find((w) => w.slug === workspaceSlug);

  return (
    <div className="grid grid-cols-[220px_1fr] gap-8">
      <aside className="space-y-6">
        <div>
          <div className="text-xs uppercase tracking-wider text-[var(--color-muted)]">
            Workspace
          </div>
          <select
            className="mt-1 w-full border-0 bg-transparent p-0 text-sm focus:outline-none"
            value={workspaceSlug}
            onChange={(e) => {
              window.location.href = `/w/${e.target.value}`;
            }}
          >
            {workspaces.data?.map((w) => (
              <option key={w.slug} value={w.slug}>
                {w.name}
              </option>
            ))}
          </select>
          {current ? (
            <div className="mt-1 text-xs text-[var(--color-muted)]">
              {current.role} · {current.slug}
            </div>
          ) : null}
        </div>
        <nav className="flex flex-col gap-1 text-sm">
          <SidebarLink to="/w/$workspaceSlug" params={{ workspaceSlug }} label="Overview" exact />
          <SidebarLink
            to="/w/$workspaceSlug/members"
            params={{ workspaceSlug }}
            label="Members"
          />
        </nav>
      </aside>
      <div>
        <Outlet />
      </div>
    </div>
  );
}

function SidebarLink({
  to,
  params,
  label,
  exact,
}: {
  to: '/w/$workspaceSlug' | '/w/$workspaceSlug/members';
  params: { workspaceSlug: string };
  label: string;
  exact?: boolean;
}) {
  return (
    <Link
      to={to}
      params={params}
      activeOptions={{ exact }}
      className="rounded-md px-2 py-1.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
      activeProps={{
        className:
          'rounded-md bg-black/5 px-2 py-1.5 text-[var(--color-fg)] dark:bg-white/5',
      }}
    >
      {label}
    </Link>
  );
}
