import { Link } from '@tanstack/react-router';
import { useQueries, useQuery } from '@tanstack/react-query';
import { ArrowLeft, Plus } from 'lucide-react';
import { StatusDot } from '@/components/ui';
import { projectQuery } from '@/lib/projects';
import { servicesQuery, serviceDeploymentsQuery } from '@/lib/services';
import { deploymentTone } from '@/lib/deployments';
import { canWrite, workspaceQuery } from '@/lib/workspaces';
import type { DeploymentSummary } from '@/lib/types';

interface Props {
  workspaceSlug: string;
  projectSlug: string;
}

export function ProjectSidebar({ workspaceSlug, projectSlug }: Props) {
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const project = useQuery(projectQuery(workspaceSlug, projectSlug));
  const services = useQuery({
    ...servicesQuery(workspaceSlug, projectSlug),
    refetchInterval: 5000,
  });

  const serviceList = services.data ?? [];
  const canCreate = canWrite(workspace.data);

  const deployments = useQueries({
    queries: serviceList.map((s) => ({
      ...serviceDeploymentsQuery(workspaceSlug, projectSlug, s.slug),
      refetchInterval: 5000,
    })),
  });

  return (
    <aside className="sticky top-20 flex h-[calc(100vh-6rem)] w-[220px] shrink-0 flex-col gap-4 self-start">
      <div>
        <Link
          to="/w/$workspaceSlug/projects"
          params={{ workspaceSlug }}
          className="inline-flex items-center gap-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <ArrowLeft className="h-3 w-3" /> All projects
        </Link>
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug"
          params={{ workspaceSlug, projectSlug }}
          className="mt-1 block truncate text-sm font-semibold hover:underline"
          activeOptions={{ exact: true }}
        >
          {project.data?.name ?? projectSlug}
        </Link>
        <div className="mt-0.5 truncate font-mono text-[11px] text-[var(--color-muted)]">
          {projectSlug}
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="mb-2 flex items-center justify-between">
          <span className="text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            Services
          </span>
          {canCreate ? (
            <Link
              to="/w/$workspaceSlug/projects/$projectSlug/new"
              params={{ workspaceSlug, projectSlug }}
              className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-muted)] hover:bg-black/5 hover:text-[var(--color-fg)] dark:hover:bg-white/5"
              aria-label="Add service"
            >
              <Plus className="h-3.5 w-3.5" />
            </Link>
          ) : null}
        </div>

        <nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto text-sm">
          {serviceList.length === 0 ? (
            <p className="px-2 py-1 text-xs text-[var(--color-muted)]">No services yet.</p>
          ) : (
            serviceList.map((s, i) => {
              const latest = (deployments[i]?.data as DeploymentSummary[] | undefined)?.[0];
              const status = deploymentTone(latest?.status);
              return (
                <Link
                  key={s.id}
                  to="/w/$workspaceSlug/projects/$projectSlug/$serviceSlug"
                  params={{ workspaceSlug, projectSlug, serviceSlug: s.slug }}
                  className="group flex items-center gap-2 rounded-md px-2 py-1.5 text-[var(--color-muted)] transition-colors hover:text-[var(--color-fg)]"
                  activeProps={{
                    className:
                      'flex items-center gap-2 rounded-md bg-black/5 px-2 py-1.5 text-[var(--color-fg)] dark:bg-white/5',
                  }}
                >
                  <StatusDot status={status.tone} pulse={status.pulse} />
                  <span className="min-w-0 flex-1 truncate">{s.name}</span>
                </Link>
              );
            })
          )}
        </nav>
      </div>
    </aside>
  );
}

