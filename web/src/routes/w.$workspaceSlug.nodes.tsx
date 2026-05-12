import { createFileRoute } from '@tanstack/react-router';
import { Card, PageHeader, Stack } from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/nodes')({
  component: NodesPage,
});

function NodesPage() {
  return (
    <Stack gap={6}>
      <PageHeader title="Nodes" />
      <Card className="p-5">
        <p className="text-sm text-[var(--color-muted)]">
          Nodes are managed by Driftbase. Pick a region when creating a project;
          capacity is provisioned automatically.
        </p>
      </Card>
    </Stack>
  );
}
