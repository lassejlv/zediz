import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { workspaceQuery } from '@/lib/workspaces';
import { projectsQuery } from '@/lib/projects';
import { nodesQuery } from '@/lib/nodes';
import { PageHeader, StatCard, Stack } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/')({
  component: OverviewPage,
});

function OverviewPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const projects = useQuery(projectsQuery(workspaceSlug));
  const nodes = useQuery(nodesQuery(workspaceSlug));

  const projectCount = projects.data?.length ?? 0;
  const nodeCount = nodes.data?.length ?? 0;
  const readyNodes = nodes.data?.filter((n) => n.status === 'ready').length ?? 0;
  const provisioningNodes =
    nodes.data?.filter((n) => n.status === 'provisioning').length ?? 0;
  const drainingNodes = nodes.data?.filter((n) => n.status === 'draining').length ?? 0;

  return (
    <Stack gap={8}>
      <PageHeader
        title={workspace.data?.name ?? workspaceSlug}
        subtitle={`Your role: ${workspace.data?.role ?? '—'}`}
      />
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCard label="Projects" value={projectCount} mono />
        <StatCard label="Nodes" value={nodeCount} mono />
        <StatCard
          label="Ready"
          value={readyNodes}
          mono
          hint={
            provisioningNodes > 0
              ? `${provisioningNodes} provisioning`
              : drainingNodes > 0
                ? `${drainingNodes} draining`
                : 'all healthy'
          }
        />
        <StatCard label="Role" value={workspace.data?.role ?? '—'} />
      </div>
    </Stack>
  );
}
