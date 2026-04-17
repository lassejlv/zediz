import { createFileRoute, Outlet } from '@tanstack/react-router';
import { projectQuery } from '@/lib/projects';
import { workspaceQuery } from '@/lib/workspaces';
import { ProjectSidebar } from '@/components/board/project-sidebar';

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
  const { workspaceSlug, projectSlug } = Route.useParams();

  return (
    <div className="grid grid-cols-[220px_1fr] gap-8">
      <ProjectSidebar workspaceSlug={workspaceSlug} projectSlug={projectSlug} />
      <div className="min-w-0">
        <Outlet />
      </div>
    </div>
  );
}
