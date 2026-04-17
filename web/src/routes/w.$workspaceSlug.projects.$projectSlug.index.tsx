import { createFileRoute, Link } from '@tanstack/react-router';
import { useQueries, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { Plus, ArrowLeft } from 'lucide-react';
import { NewServiceSheet } from '@/components/new-service-sheet';
import { Button, EmptyState } from '@/components/ui';
import { projectQuery } from '@/lib/projects';
import { servicesQuery, serviceDeploymentsQuery, useDeployService } from '@/lib/services';
import { workspaceQuery } from '@/lib/workspaces';
import { domainsQuery } from '@/lib/domains';
import { BoardToolbar } from '@/components/board/board-toolbar';
import { BoardCanvas } from '@/components/board/board-canvas';
import { BoardInspector } from '@/components/board/board-inspector';
import type { ServiceNodeState } from '@/components/board/service-node';
import { useBoardPositions } from '@/lib/board-positions';
import type { DeploymentSummary, DomainSummary } from '@/lib/types';

export const Route = createFileRoute('/w/$workspaceSlug/projects/$projectSlug/')({
  component: ProjectBoard,
  validateSearch: (search): { selected?: string } => ({
    selected: typeof search.selected === 'string' ? search.selected : undefined,
  }),
});

function ProjectBoard() {
  const { workspaceSlug, projectSlug } = Route.useParams();
  const { selected } = Route.useSearch();
  const navigate = Route.useNavigate();

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

  const canCreate = workspace.data ? workspace.data.role !== 'viewer' : false;
  const canDeploy = canCreate;

  const { positions, setPosition, clear, hasCustomLayout } = useBoardPositions(
    workspaceSlug,
    projectSlug,
  );

  const selectedNode = selected ? nodes.find((n) => n.service.slug === selected) ?? null : null;
  const setSelected = (slug: string | null) => {
    navigate({
      search: (prev) => ({ ...prev, selected: slug ?? undefined }),
      replace: true,
    });
  };

  const deploy = useDeployService(workspaceSlug, projectSlug, selectedNode?.service.slug ?? '');

  return (
    <div className="flex flex-col gap-4">
      <div>
        <div className="mb-3 flex items-center gap-1 text-xs text-[var(--color-muted)]">
          <Link
            to="/w/$workspaceSlug/projects"
            params={{ workspaceSlug }}
            className="inline-flex items-center gap-1 hover:text-[var(--color-fg)]"
          >
            <ArrowLeft className="h-3 w-3" /> All projects
          </Link>
        </div>
        <BoardToolbar
          projectName={project.data?.name ?? projectSlug}
          projectSlug={projectSlug}
          summary={summary}
          canCreate={canCreate}
          createTrigger={
            <div className="flex items-center gap-2">
              {hasCustomLayout ? (
                <Button variant="secondary" onClick={clear}>
                  Reset layout
                </Button>
              ) : null}
              {canCreate ? (
                <NewServiceSheet workspaceSlug={workspaceSlug} projectSlug={projectSlug}>
                  <Button>
                    <Plus className="mr-1 h-3.5 w-3.5" /> Add service
                  </Button>
                </NewServiceSheet>
              ) : null}
            </div>
          }
        />
      </div>

      <div className="flex min-h-[480px] gap-0 overflow-hidden rounded-lg border border-[var(--color-border)]">
        <div className="min-w-0 flex-1 overflow-auto">
          <BoardCanvas
            nodes={nodes}
            selectedSlug={selected ?? null}
            onSelect={setSelected}
            positions={positions}
            onPositionChange={setPosition}
            hasCustomLayout={hasCustomLayout}
            empty={
              <EmptyState
                title="No services yet"
                body="A service runs a container in this project. Add one to deploy."
                cta={
                  canCreate ? (
                    <NewServiceSheet workspaceSlug={workspaceSlug} projectSlug={projectSlug}>
                      <Button>
                        <Plus className="mr-1 h-3.5 w-3.5" /> Add service
                      </Button>
                    </NewServiceSheet>
                  ) : null
                }
              />
            }
          />
        </div>
        {selectedNode ? (
          <div className="sticky top-20 h-[calc(100vh-8rem)]">
            <BoardInspector
              state={selectedNode}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              canDeploy={canDeploy}
              onClose={() => setSelected(null)}
              onDeploy={() => deploy.mutate()}
              deployPending={deploy.isPending}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}
