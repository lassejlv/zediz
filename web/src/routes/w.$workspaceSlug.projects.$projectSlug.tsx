import { createFileRoute, Outlet } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
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
  const { workspaceSlug, projectSlug } = Route.useParams();
  // Prime queries so child routes see data immediately.
  useQuery(projectQuery(workspaceSlug, projectSlug));
  useQuery(workspaceQuery(workspaceSlug));

  return <Outlet />;
}
