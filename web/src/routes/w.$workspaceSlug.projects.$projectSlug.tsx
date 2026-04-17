import { createFileRoute, Outlet } from '@tanstack/react-router';
import { projectQuery } from '@/lib/projects';
import { workspaceQuery } from '@/lib/workspaces';

export const Route = createFileRoute('/w/$workspaceSlug/projects/$projectSlug')({
  component: ProjectLayout,
  loader: ({ context, params }) =>
    Promise.all([
      context.queryClient.ensureQueryData(workspaceQuery(params.workspaceSlug)),
      context.queryClient.ensureQueryData(
        projectQuery(params.workspaceSlug, params.projectSlug),
      ),
    ]),
});

function ProjectLayout() {
  return (
    <div className="min-w-0">
      <Outlet />
    </div>
  );
}
