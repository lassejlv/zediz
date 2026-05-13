import { createFileRoute, Outlet, redirect } from '@tanstack/react-router';
import { meQuery } from '@/lib/auth';

export const Route = createFileRoute('/admin')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) throw redirect({ to: '/login' });
    if (!me.is_platform_admin) throw redirect({ to: '/' });
  },
  component: () => <Outlet />,
});
