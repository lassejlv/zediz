import { createFileRoute } from '@tanstack/react-router';
import { Card, PageHeader, Stack } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/ssh-keys')({
  component: SshKeysPage,
});

function SshKeysPage() {
  return (
    <Stack gap={6}>
      <PageHeader title="SSH keys" />
      <Card className="p-5">
        <p className="text-sm text-[var(--color-muted)]">
          SSH keys are managed by Driftbase for provisioned nodes.
        </p>
      </Card>
    </Stack>
  );
}
