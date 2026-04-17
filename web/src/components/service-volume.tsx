import { useEffect, useState, type FormEvent } from 'react';
import { useQuery } from '@tanstack/react-query';
import { HardDrive, Plus, Trash2 } from 'lucide-react';
import {
  serviceVolumeQuery,
  useAttachVolume,
  useDetachVolume,
  useDeleteVolume,
  workspaceVolumesQuery,
  type VolumeSummary,
} from '@/lib/volumes';
import { ApiError } from '@/lib/api';
import {
  Button,
  Card,
  EmptyState,
  ErrorText,
  Field,
  Input,
  Stack,
  StatusPill,
  type SemanticStatus,
} from '@/components/ui';
import { CreateVolumeSheet } from '@/components/create-volume-sheet';
import type { ServiceSummary } from '@/lib/types';

interface Props {
  service: ServiceSummary;
  workspaceSlug: string;
  projectSlug: string;
  canManage: boolean;
}

export function ServiceVolumeTab({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const attached = useQuery(serviceVolumeQuery(workspaceSlug, projectSlug, service.slug));
  const workspaceVolumes = useQuery(workspaceVolumesQuery(workspaceSlug));
  const detach = useDetachVolume(workspaceSlug, projectSlug, service.slug);

  if (attached.data) {
    return (
      <Stack gap={4}>
        <AttachedVolumeCard
          volume={attached.data}
          canManage={canManage}
          onDetach={() => detach.mutate()}
          detaching={detach.isPending}
        />

        {service.replicas > 1 ? (
          <p className="text-xs text-amber-400">
            This service has {service.replicas} replicas, but Hetzner block volumes can only be
            mounted on one server at a time. The scheduler will place a single replica.
          </p>
        ) : null}

        <p className="text-xs text-[var(--color-muted)]">
          Volume changes take effect on the next deploy. Hit{' '}
          <span className="font-mono">Redeploy</span> to apply.
        </p>
      </Stack>
    );
  }

  const availableVolumes =
    workspaceVolumes.data?.filter(
      (v) => v.status === 'available' && !v.attached_service_id,
    ) ?? [];

  return (
    <Stack gap={4}>
      <div className="flex items-end justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium">Volume</h2>
          <p className="mt-0.5 text-xs text-[var(--color-muted)]">
            Attach one Hetzner block volume for persistent storage.
          </p>
        </div>
        {canManage ? (
          <CreateVolumeSheet workspaceSlug={workspaceSlug}>
            <Button>
              <Plus className="mr-1.5 h-4 w-4" /> Create volume
            </Button>
          </CreateVolumeSheet>
        ) : null}
      </div>

      {availableVolumes.length === 0 ? (
        <EmptyState
          title="No volumes attached"
          body="Create a volume to get persistent storage for this service. Data survives redeploys and can move with the service to another node."
          cta={
            canManage ? (
              <CreateVolumeSheet workspaceSlug={workspaceSlug}>
                <Button>
                  <Plus className="mr-1.5 h-4 w-4" /> Create volume
                </Button>
              </CreateVolumeSheet>
            ) : null
          }
        />
      ) : (
        <Stack gap={2}>
          <div className="text-[11px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
            Available in this workspace
          </div>
          {availableVolumes.map((v) => (
            <AvailableVolumeRow
              key={v.id}
              volume={v}
              canManage={canManage}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              serviceSlug={service.slug}
            />
          ))}
        </Stack>
      )}

      <p className="text-xs text-[var(--color-muted)]">
        Volume changes take effect on the next deploy.
      </p>
    </Stack>
  );
}

/* ---------- attached view ---------- */

function AttachedVolumeCard({
  volume,
  canManage,
  onDetach,
  detaching,
}: {
  volume: VolumeSummary;
  canManage: boolean;
  onDetach: () => void;
  detaching: boolean;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const id = setTimeout(() => setArmed(false), 3000);
    return () => clearTimeout(id);
  }, [armed]);

  return (
    <Card className="p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2.5">
            <HardDrive className="h-4 w-4 text-[var(--color-muted)]" />
            <span className="font-medium">{volume.name}</span>
            <StatusPill
              status={volumeTone(volume.status)}
              label={volume.status}
              pulse={volume.status === 'creating' || volume.status === 'detaching'}
            />
          </div>
          <dl className="mt-3 grid grid-cols-1 gap-2 text-sm sm:grid-cols-2">
            <Row label="Size" value={`${volume.size_gb} GB`} />
            <Row
              label="Mount path"
              value={
                volume.mount_path ? (
                  <span className="font-mono text-xs">{volume.mount_path}</span>
                ) : (
                  '—'
                )
              }
            />
            <Row label="Location" value={volume.hetzner_location} />
            <Row
              label="Attached to node"
              value={
                volume.attached_node_id ? (
                  <span className="font-mono text-xs">
                    {volume.attached_node_id.slice(0, 12)}
                  </span>
                ) : (
                  '—'
                )
              }
            />
          </dl>
        </div>
        {canManage ? (
          armed ? (
            <Button variant="danger" onClick={onDetach} disabled={detaching}>
              {detaching ? 'Detaching…' : 'Confirm detach'}
            </Button>
          ) : (
            <Button variant="secondary" onClick={() => setArmed(true)}>
              Detach
            </Button>
          )
        ) : null}
      </div>
      {volume.reason ? (
        <p className="mt-3 text-xs text-red-400">{volume.reason}</p>
      ) : null}
    </Card>
  );
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <dt className="text-xs text-[var(--color-muted)]">{label}</dt>
      <dd className="font-mono text-xs">{value}</dd>
    </div>
  );
}

/* ---------- available-volume row ---------- */

function AvailableVolumeRow({
  volume,
  canManage,
  workspaceSlug,
  projectSlug,
  serviceSlug,
}: {
  volume: VolumeSummary;
  canManage: boolean;
  workspaceSlug: string;
  projectSlug: string;
  serviceSlug: string;
}) {
  const attach = useAttachVolume(workspaceSlug, projectSlug, serviceSlug);
  const del = useDeleteVolume(workspaceSlug);
  const [mountPath, setMountPath] = useState('/data');
  const [error, setError] = useState<string | null>(null);

  async function onAttach(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await attach.mutateAsync({ volume_id: volume.id, mount_path: mountPath });
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : 'Failed to attach',
      );
    }
  }

  return (
    <Card className="p-4">
      <div className="flex flex-wrap items-center gap-3">
        <HardDrive className="h-4 w-4 text-[var(--color-muted)]" />
        <span className="font-medium">{volume.name}</span>
        <span className="text-xs font-mono text-[var(--color-muted)]">
          {volume.size_gb} GB · {volume.hetzner_location}
        </span>
      </div>
      <form
        onSubmit={onAttach}
        className="mt-3 grid grid-cols-[1fr_auto_auto] items-end gap-2"
      >
        <Field label="Mount path" htmlFor={`mount-${volume.id}`}>
          <Input
            id={`mount-${volume.id}`}
            value={mountPath}
            onChange={(e) => setMountPath(e.target.value)}
            placeholder="/data"
            disabled={!canManage}
          />
        </Field>
        <Button type="submit" disabled={!canManage || attach.isPending}>
          {attach.isPending ? 'Attaching…' : 'Attach'}
        </Button>
        {canManage ? (
          <button
            type="button"
            onClick={() => {
              if (confirm(`Delete volume ${volume.name}? Data is wiped.`)) {
                del.mutate(volume.id);
              }
            }}
            className="inline-flex h-9 w-9 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-red-500/10 hover:text-red-400"
            aria-label="Delete volume"
            title="Delete volume"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        ) : null}
      </form>
      {error ? (
        <div className="mt-2">
          <ErrorText>{error}</ErrorText>
        </div>
      ) : null}
    </Card>
  );
}

function volumeTone(status: VolumeSummary['status']): SemanticStatus {
  switch (status) {
    case 'attached':
      return 'ok';
    case 'available':
      return 'info';
    case 'creating':
    case 'detaching':
      return 'warn';
    case 'errored':
      return 'error';
  }
}
