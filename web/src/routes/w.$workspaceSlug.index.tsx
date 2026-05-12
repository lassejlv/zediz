import { createFileRoute, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { Sparkles } from 'lucide-react';
import { canAdmin, workspaceQuery } from '@/lib/workspaces';
import { projectsQuery } from '@/lib/projects';
import { credentialsQuery } from '@/lib/credentials';
import { Button, Card, PageHeader, StatCard, Stack } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/')({
  component: OverviewPage,
});

function OverviewPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const projects = useQuery(projectsQuery(workspaceSlug));
  const isAdmin = canAdmin(workspace.data);
  const credentials = useQuery({
    ...credentialsQuery(workspaceSlug),
    enabled: isAdmin,
  });

  const projectCount = projects.data?.length ?? 0;
  const hasGithub = !!credentials.data?.some((c) => c.kind === 'github_pat');
  const hasRegistry = !!credentials.data?.some((c) => c.kind === 'registry');
  const showOnboarding =
    isAdmin &&
    credentials.isSuccess &&
    (!hasGithub || !hasRegistry);

  return (
    <Stack gap={8}>
      <PageHeader
        title={workspace.data?.name ?? workspaceSlug}
        subtitle={`Your role: ${workspace.data?.role ?? '—'}`}
      />
      {showOnboarding ? (
        <OnboardingBanner
          workspaceSlug={workspaceSlug}
          hasGithub={hasGithub}
          hasRegistry={hasRegistry}
        />
      ) : null}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <StatCard label="Projects" value={projectCount} mono />
        <StatCard label="Infrastructure" value="Managed" />
        <StatCard label="Role" value={workspace.data?.role ?? '—'} />
      </div>
    </Stack>
  );
}

function OnboardingBanner({
  workspaceSlug,
  hasGithub,
  hasRegistry,
}: {
  workspaceSlug: string;
  hasGithub: boolean;
  hasRegistry: boolean;
}) {
  const missing: string[] = [];
  if (!hasGithub) missing.push('GitHub PAT');
  if (!hasRegistry) missing.push('registry credential');
  const remaining = `Add a ${missing.join(', ')} to start deploying.`;
  return (
    <Card className="flex items-start gap-3 border-[var(--color-accent)]/30 bg-[var(--color-accent)]/5 p-4">
      <span className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 text-[var(--color-accent)]">
        <Sparkles className="h-4 w-4" />
      </span>
      <div className="flex-1">
        <div className="text-sm font-medium">Finish setting up this workspace</div>
        <p className="mt-0.5 text-xs text-[var(--color-muted)]">{remaining}</p>
      </div>
      <Link
        to="/w/$workspaceSlug/onboarding"
        params={{ workspaceSlug }}
        className="shrink-0"
      >
        <Button>Continue setup</Button>
      </Link>
    </Card>
  );
}
