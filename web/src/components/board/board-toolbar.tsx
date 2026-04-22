import { Plus } from 'lucide-react';
import type { ReactNode } from 'react';
import { Button, StatusDot } from '@/components/ui';

interface Props {
  projectName: string;
  projectSlug: string;
  summary: {
    total: number;
    running: number;
    deploying: number;
    errored: number;
  };
  canCreate: boolean;
  createTrigger?: ReactNode;
}

export function BoardToolbar({
  projectName,
  projectSlug,
  summary,
  canCreate,
  createTrigger,
}: Props) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 pb-6">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <h1 className="truncate text-xl font-semibold tracking-tight">{projectName}</h1>
          <span className="font-mono text-xs text-[var(--color-muted)]">{projectSlug}</span>
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--color-muted)]">
          <span>
            {summary.total} service{summary.total === 1 ? '' : 's'}
          </span>
          {summary.running > 0 ? (
            <span className="flex items-center gap-1.5">
              <StatusDot status="ok" />
              {summary.running} running
            </span>
          ) : null}
          {summary.deploying > 0 ? (
            <span className="flex items-center gap-1.5">
              <StatusDot status="warn" pulse />
              {summary.deploying} deploying
            </span>
          ) : null}
          {summary.errored > 0 ? (
            <span className="flex items-center gap-1.5">
              <StatusDot status="error" />
              {summary.errored} errored
            </span>
          ) : null}
        </div>
      </div>
      {canCreate && createTrigger ? (
        createTrigger
      ) : canCreate ? (
        <Button>
          <Plus className="mr-1 h-3.5 w-3.5" /> Add service
        </Button>
      ) : null}
    </div>
  );
}
