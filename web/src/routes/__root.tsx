import {
  createRootRouteWithContext,
  Outlet,
  Link,
  useLocation,
} from '@tanstack/react-router';
import type { QueryClient } from '@tanstack/react-query';
import { ChevronRight } from 'lucide-react';
import { ThemeToggle } from '@/components/ThemeToggle';
import { useMe, useLogout } from '@/lib/auth';
import { Button } from '@/components/ui';
import {
  ActivityStrip,
  ActivityStripProvider,
  CommandKTrigger,
  CommandPaletteProvider,
} from '@/components/app-shell';
import { CommandPalette } from '@/components/command-palette';

interface RouterContext {
  queryClient: QueryClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootComponent,
});

function RootComponent() {
  const me = useMe();
  const logout = useLogout();
  const location = useLocation();
  const crumb = deriveCrumb(location.pathname);

  return (
    <CommandPaletteProvider>
      <ActivityStripProvider>
        <div className="min-h-full">
          <header className="sticky top-0 z-10 flex h-12 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg)]/85 px-6 backdrop-blur">
            <div className="flex min-w-0 items-center gap-3">
              <Link to="/" className="flex shrink-0 items-center gap-2">
                <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-accent)]" />
                <span className="font-mono text-sm tracking-tight">driftbase</span>
              </Link>
              {crumb ? (
                <>
                  <ChevronRight className="h-3.5 w-3.5 shrink-0 text-[var(--color-subtle)]" />
                  <nav className="flex min-w-0 items-center gap-1.5 text-xs text-[var(--color-muted)]">
                    <Link
                      to="/w/$workspaceSlug"
                      params={{ workspaceSlug: crumb.workspace }}
                      className="truncate font-medium text-[var(--color-fg)] hover:text-[var(--color-fg)]"
                    >
                      {crumb.workspace}
                    </Link>
                    {crumb.project ? (
                      <>
                        <span className="text-[var(--color-subtle)]">/</span>
                        <Link
                          to="/w/$workspaceSlug/projects/$projectSlug"
                          params={{
                            workspaceSlug: crumb.workspace,
                            projectSlug: crumb.project,
                          }}
                          className="truncate hover:text-[var(--color-fg)]"
                        >
                          {crumb.project}
                        </Link>
                      </>
                    ) : null}
                    {crumb.service ? (
                      <>
                        <span className="text-[var(--color-subtle)]">/</span>
                        <span className="truncate text-[var(--color-fg)]">{crumb.service}</span>
                      </>
                    ) : null}
                  </nav>
                </>
              ) : null}
            </div>
            <div className="flex shrink-0 items-center gap-3">
              {me.data ? <CommandKTrigger /> : null}
              {me.data ? (
                <>
                  {me.data.is_platform_admin ? (
                    <Link
                      to="/admin"
                      className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                    >
                      Admin
                    </Link>
                  ) : null}
                  <span className="text-xs text-[var(--color-muted)]">{me.data.email}</span>
                  <Button
                    variant="ghost"
                    onClick={() => logout.mutate()}
                    disabled={logout.isPending}
                  >
                    Sign out
                  </Button>
                </>
              ) : null}
              <ThemeToggle />
            </div>
          </header>
          <ActivityStrip />
          <main className="mx-auto max-w-6xl px-6 py-10">
            <Outlet />
          </main>
          <CommandPalette />
        </div>
      </ActivityStripProvider>
    </CommandPaletteProvider>
  );
}

function deriveCrumb(
  pathname: string,
): { workspace: string; project?: string; service?: string } | null {
  const m = pathname.match(
    /^\/w\/([^/]+)(?:\/projects\/([^/]+)(?:\/([^/]+))?)?/,
  );
  if (!m) return null;
  return {
    workspace: decodeURIComponent(m[1]),
    project: m[2] ? decodeURIComponent(m[2]) : undefined,
    service: m[3] ? decodeURIComponent(m[3]) : undefined,
  };
}
