import { createRootRouteWithContext, Outlet, Link } from '@tanstack/react-router';
import type { QueryClient } from '@tanstack/react-query';
import { ThemeToggle } from '@/components/ThemeToggle';
import { useMe, useLogout } from '@/lib/auth';
import { Button } from '@/components/ui';

interface RouterContext {
  queryClient: QueryClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootComponent,
});

function RootComponent() {
  const me = useMe();
  const logout = useLogout();

  return (
    <div className="min-h-full">
      <header className="sticky top-0 z-10 flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg)]/85 px-6 py-3 backdrop-blur">
        <Link to="/" className="flex items-center gap-2">
          <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-accent)]" />
          <span className="font-mono text-sm tracking-tight">driftbase</span>
        </Link>
        <div className="flex items-center gap-3">
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
      <main className="mx-auto max-w-6xl px-6 py-10">
        <Outlet />
      </main>
    </div>
  );
}
