import { createFileRoute, Link, redirect } from '@tanstack/react-router';
import { Github } from 'lucide-react';
import { meQuery, useMe } from '@/lib/auth';
import { workspacesQuery } from '@/lib/workspaces';
import { Button } from '@/components/ui';

export const Route = createFileRoute('/')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (me) {
      const workspaces = await context.queryClient.ensureQueryData(workspacesQuery);
      if (workspaces.length > 0) {
        throw redirect({
          to: '/w/$workspaceSlug',
          params: { workspaceSlug: workspaces[0].slug },
        });
      }
    }
  },
  component: Home,
});

function Home() {
  const me = useMe();
  return me.data ? <NoWorkspaces /> : <Landing />;
}

function Landing() {
  return (
    <section className="mx-auto max-w-xl pt-20 pb-24 text-center">
      <div className="mx-auto mb-6 inline-flex items-center gap-2 rounded-full border border-[var(--color-border)] px-3 py-1 text-xs text-[var(--color-muted)]">
        <span className="relative inline-flex h-1.5 w-1.5">
          <span className="absolute inline-flex h-full w-full rounded-full bg-[var(--color-accent)] opacity-60 animate-status-pulse" />
          <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-[var(--color-accent)]" />
        </span>
        self-hosted
      </div>

      <h1 className="text-4xl font-semibold tracking-tight sm:text-5xl">
        A tiny PaaS on your own Hetzner nodes.
      </h1>

      <p className="mx-auto mt-5 max-w-md text-[15px] leading-relaxed text-[var(--color-muted)]">
        Push a Docker image or a Git repo. Driftbase picks a node, pulls, routes, and keeps the
        domain up across redeploys.
      </p>

      <div className="mt-8 flex items-center justify-center gap-3">
        <Link to="/login">
          <Button>Sign in</Button>
        </Link>
        <Link to="/signup">
          <Button variant="secondary">Create account</Button>
        </Link>
      </div>

      <dl className="mx-auto mt-16 grid max-w-lg grid-cols-1 gap-6 text-left sm:grid-cols-3">
        <Feature
          title="Rolling deploys"
          body="Old container keeps serving until the new one is live — no 502 window."
        />
        <Feature
          title="Autoscaled nodes"
          body="Provisions Hetzner VMs when capacity runs out, tears them down when idle."
        />
        <Feature
          title="Live metrics"
          body="CPU, memory, and network graphs straight from the container cgroups."
        />
      </dl>

      <a
        href="https://github.com/lassejlv/driftbase"
        target="_blank"
        rel="noreferrer"
        className="mt-16 inline-flex items-center gap-1.5 text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
      >
        <Github className="h-3.5 w-3.5" />
        github.com/lassejlv/driftbase
      </a>
    </section>
  );
}

function Feature({ title, body }: { title: string; body: string }) {
  return (
    <div>
      <dt className="font-mono text-xs uppercase tracking-wider text-[var(--color-fg)]">
        {title}
      </dt>
      <dd className="mt-1 text-xs leading-relaxed text-[var(--color-muted)]">{body}</dd>
    </div>
  );
}

function NoWorkspaces() {
  return (
    <section className="mx-auto max-w-md space-y-4 pt-16 text-center">
      <h1 className="text-2xl font-semibold tracking-tight">No workspaces yet</h1>
      <p className="text-sm text-[var(--color-muted)]">
        Create your first workspace to deploy services, invite teammates, and manage nodes.
      </p>
      <Link to="/w/new">
        <Button>Create workspace</Button>
      </Link>
    </section>
  );
}
