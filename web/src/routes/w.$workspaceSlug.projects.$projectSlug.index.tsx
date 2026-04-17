import { createFileRoute, Link } from '@tanstack/react-router';
import { useQueries, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { LayoutTemplate, Plus } from 'lucide-react';
import { Button, EmptyState } from '@/components/ui';
import { projectQuery } from '@/lib/projects';
import { servicesQuery, serviceDeploymentsQuery } from '@/lib/services';
import { canWrite, workspaceQuery } from '@/lib/workspaces';
import { domainsQuery } from '@/lib/domains';
import { BoardToolbar } from '@/components/board/board-toolbar';
import { ServiceNode, type ServiceNodeState } from '@/components/board/service-node';
import type { DeploymentSummary, DomainSummary } from '@/lib/types';

export const Route = createFileRoute('/w/$workspaceSlug/projects/$projectSlug/')({
  component: ProjectBoard,
});

function ProjectBoard() {
  const { workspaceSlug, projectSlug } = Route.useParams();

  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const project = useQuery(projectQuery(workspaceSlug, projectSlug));
  const services = useQuery({
    ...servicesQuery(workspaceSlug, projectSlug),
    refetchInterval: 5000,
  });

  const serviceList = services.data ?? [];

  const deployments = useQueries({
    queries: serviceList.map((s) => ({
      ...serviceDeploymentsQuery(workspaceSlug, projectSlug, s.slug),
      refetchInterval: 5000,
    })),
  });

  const domains = useQueries({
    queries: serviceList.map((s) => domainsQuery(workspaceSlug, projectSlug, s.slug)),
  });

  const nodes: ServiceNodeState[] = useMemo(
    () =>
      serviceList.map((s, i) => {
        const latest = (deployments[i]?.data as DeploymentSummary[] | undefined)?.[0];
        const firstDomain = (domains[i]?.data as DomainSummary[] | undefined)?.[0];
        return {
          service: s,
          latestDeployment: latest,
          primaryDomain: firstDomain?.hostname ?? null,
        };
      }),
    [serviceList, deployments, domains],
  );

  const summary = useMemo(() => {
    let running = 0;
    let deploying = 0;
    let errored = 0;
    for (const n of nodes) {
      const s = n.latestDeployment?.status;
      if (s === 'running') running++;
      else if (s === 'pending' || s === 'placing' || s === 'pulling' || s === 'starting')
        deploying++;
      else if (s === 'errored' || s === 'failing') errored++;
    }
    return { total: nodes.length, running, deploying, errored };
  }, [nodes]);

  const canCreate = canWrite(workspace.data);

  return (
    <div className="flex flex-col gap-6">
      <div>
        <BoardToolbar
          projectName={project.data?.name ?? projectSlug}
          projectSlug={projectSlug}
          summary={summary}
          canCreate={canCreate}
          createTrigger={
            canCreate ? (
              <div className="flex items-center gap-2">
                <Link
                  to="/w/$workspaceSlug/projects/$projectSlug/templates"
                  params={{ workspaceSlug, projectSlug }}
                >
                  <Button variant="secondary">
                    <LayoutTemplate className="mr-1 h-3.5 w-3.5" /> Templates
                  </Button>
                </Link>
                <Link
                  to="/w/$workspaceSlug/projects/$projectSlug/new"
                  params={{ workspaceSlug, projectSlug }}
                >
                  <Button>
                    <Plus className="mr-1 h-3.5 w-3.5" /> Add service
                  </Button>
                </Link>
              </div>
            ) : null
          }
        />
      </div>

      {nodes.length === 0 ? (
        <EmptyState
          title="No services yet"
          body="A service runs a container in this project. Add one to deploy."
          cta={
            canCreate ? (
              <div className="flex items-center gap-2">
                <Link
                  to="/w/$workspaceSlug/projects/$projectSlug/templates"
                  params={{ workspaceSlug, projectSlug }}
                >
                  <Button variant="secondary">
                    <LayoutTemplate className="mr-1 h-3.5 w-3.5" /> Start from template
                  </Button>
                </Link>
                <Link
                  to="/w/$workspaceSlug/projects/$projectSlug/new"
                  params={{ workspaceSlug, projectSlug }}
                >
                  <Button>
                    <Plus className="mr-1 h-3.5 w-3.5" /> Blank service
                  </Button>
                </Link>
              </div>
            ) : null
          }
        />
      ) : (
        <div
          className="grid gap-4"
          style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))' }}
        >
          {nodes.map((node) => (
            <ServiceNode
              key={node.service.id}
              state={node}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
            />
          ))}
        </div>
      )}
    </div>
  );
}
