import { createFileRoute, Link, redirect } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, ChevronRight, HardDrive, Zap } from 'lucide-react';
import { meQuery } from '@/lib/auth';
import { projectQuery } from '@/lib/projects';
import { canWrite, workspaceQuery } from '@/lib/workspaces';
import { Card, Stack } from '@/components/ui';
import { TEMPLATES, type Template } from '@/lib/templates';

export const Route = createFileRoute('/w/$workspaceSlug/projects/$projectSlug/templates/')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) throw redirect({ to: '/login' });
  },
  component: TemplatesGallery,
});

function TemplatesGallery() {
  const { workspaceSlug, projectSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const project = useQuery(projectQuery(workspaceSlug, projectSlug));

  if (workspace.data && !canWrite(workspace.data)) {
    return (
      <div className="mx-auto max-w-xl py-12 text-center text-sm text-[var(--color-muted)]">
        You need write access to deploy templates in this project.
      </div>
    );
  }

  return (
    <Stack gap={6} className="mx-auto max-w-4xl">
      <div>
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug"
          params={{ workspaceSlug, projectSlug }}
          className="inline-flex items-center gap-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <ArrowLeft className="h-3 w-3" /> {project.data?.name ?? projectSlug}
        </Link>
        <h1 className="mt-2 text-2xl font-semibold tracking-tight">Templates</h1>
        <p className="mt-1 text-sm text-[var(--color-muted)]">
          One-click configs for common services. Picks the image, env vars, ports, and a
          persistent volume so you don't have to.
        </p>
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        {TEMPLATES.map((t) => (
          <TemplateCard
            key={t.id}
            template={t}
            to={{ workspaceSlug, projectSlug, templateId: t.id }}
          />
        ))}
      </div>

      <p className="text-xs text-[var(--color-muted)]">
        Need something that isn't here? Use{' '}
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug/new"
          params={{ workspaceSlug, projectSlug }}
          className="text-[var(--color-fg)] underline-offset-4 hover:underline"
        >
          New service
        </Link>{' '}
        to configure it by hand.
      </p>
    </Stack>
  );
}

function TemplateCard({
  template,
  to,
}: {
  template: Template;
  to: { workspaceSlug: string; projectSlug: string; templateId: string };
}) {
  const Icon = template.icon;
  return (
    <Link
      to="/w/$workspaceSlug/projects/$projectSlug/templates/$templateId"
      params={to}
      className="group block"
    >
      <Card className="h-full p-5 transition-colors hover:border-[var(--color-border-strong)]">
        <div className="flex items-start gap-3">
          <span className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-[var(--color-border)] bg-black/[0.02] text-[var(--color-fg)] dark:bg-white/[0.02]">
            <Icon className="h-4 w-4" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="font-medium">{template.name}</span>
              <span className="font-mono text-[10px] text-[var(--color-muted)]">
                {template.image}
              </span>
            </div>
            <p className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">
              {template.description}
            </p>
          </div>
          <ChevronRight className="h-4 w-4 shrink-0 text-[var(--color-subtle)] transition-transform group-hover:translate-x-0.5 group-hover:text-[var(--color-muted)]" />
        </div>

        <dl className="mt-4 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-[var(--color-muted)]">
          <Fact icon={<Zap className="h-3 w-3" />} label={`${template.resources.cpu_millis}m CPU · ${template.resources.memory_mb} MB`} />
          {template.volume ? (
            <Fact
              icon={<HardDrive className="h-3 w-3" />}
              label={`${template.volume.defaultSizeGb} GB at ${template.volume.mountPath}`}
            />
          ) : null}
        </dl>
      </Card>
    </Link>
  );
}

function Fact({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 font-mono">
      {icon}
      {label}
    </span>
  );
}
