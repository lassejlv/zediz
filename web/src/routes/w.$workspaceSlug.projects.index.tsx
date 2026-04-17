import { createFileRoute, Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Plus } from 'lucide-react';
import {
  projectsQuery,
  useCreateProject,
  useDeleteProject,
} from '@/lib/projects';
import { workspaceQuery } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import {
  Button,
  Card,
  ErrorText,
  Field,
  Input,
  PageHeader,
  EmptyState,
  Stack,
  RelativeTime,
  CopyableId,
} from '@/components/ui';
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/sheet';

export const Route = createFileRoute('/w/$workspaceSlug/projects/')({
  component: ProjectsPage,
});

function ProjectsPage() {
  const { workspaceSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const projects = useQuery(projectsQuery(workspaceSlug));
  const del = useDeleteProject(workspaceSlug);

  const canCreate = workspace.data ? workspace.data.role !== 'viewer' : false;
  const canDelete = workspace.data
    ? workspace.data.role === 'owner' || workspace.data.role === 'admin'
    : false;

  const [sheetOpen, setSheetOpen] = useState(false);

  return (
    <Stack gap={6}>
      <PageHeader
        title="Projects"
        subtitle="Group services together inside this workspace."
        actions={
          canCreate ? (
            <Button onClick={() => setSheetOpen(true)}>
              <Plus className="mr-1 h-3.5 w-3.5" /> New project
            </Button>
          ) : null
        }
      />

      {projects.data?.length ? (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {projects.data.map((p) => (
            <Card
              key={p.id}
              className="group flex flex-col gap-3 p-5 hover:border-[var(--color-border-strong)]"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <Link
                    to="/w/$workspaceSlug/projects/$projectSlug"
                    params={{ workspaceSlug, projectSlug: p.slug }}
                    className="truncate text-base font-medium hover:underline"
                  >
                    {p.name}
                  </Link>
                  <div className="mt-0.5">
                    <CopyableId value={p.slug} />
                  </div>
                </div>
                {canDelete ? (
                  <Button
                    variant="danger"
                    onClick={() => {
                      if (confirm(`Delete project ${p.slug}?`)) del.mutate(p.slug);
                    }}
                    className="opacity-0 transition-opacity group-hover:opacity-100"
                  >
                    Delete
                  </Button>
                ) : null}
              </div>
              <div className="flex items-center justify-between text-xs text-[var(--color-muted)]">
                <span>
                  Created <RelativeTime date={p.created_at} />
                </span>
                <Link
                  to="/w/$workspaceSlug/projects/$projectSlug"
                  params={{ workspaceSlug, projectSlug: p.slug }}
                  className="text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                >
                  Open →
                </Link>
              </div>
            </Card>
          ))}
        </div>
      ) : (
        <EmptyState
          title="No projects yet"
          body="Projects group services together. Create one to get started."
          cta={
            canCreate ? (
              <Button onClick={() => setSheetOpen(true)}>
                <Plus className="mr-1 h-3.5 w-3.5" /> New project
              </Button>
            ) : null
          }
        />
      )}

      <NewProjectSheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        workspaceSlug={workspaceSlug}
      />
    </Stack>
  );
}

function NewProjectSheet({
  open,
  onOpenChange,
  workspaceSlug,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
}) {
  const create = useCreateProject(workspaceSlug);
  const [slug, setSlug] = useState('');
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await create.mutateAsync({ slug, name });
      setSlug('');
      setName('');
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>New project</SheetTitle>
          <SheetDescription>
            A project groups services together. Name and slug can reflect the product
            or environment.
          </SheetDescription>
        </SheetHeader>
        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-4">
          <Field label="Slug" htmlFor="proj-slug" hint="lowercase, dashes allowed">
            <Input
              id="proj-slug"
              required
              placeholder="api"
              value={slug}
              onChange={(e) => setSlug(e.target.value)}
            />
          </Field>
          <Field label="Name" htmlFor="proj-name">
            <Input
              id="proj-name"
              required
              placeholder="Public API"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <SheetFooter>
            <SheetClose asChild>
              <Button type="button" variant="ghost">
                Cancel
              </Button>
            </SheetClose>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? 'Creating…' : 'Create project'}
            </Button>
          </SheetFooter>
        </form>
      </SheetContent>
    </Sheet>
  );
}
