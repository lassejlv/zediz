import { useState, type FormEvent, type ReactNode } from 'react';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from '@/components/sheet';
import { Button, ErrorText, Field, Input, Select } from '@/components/ui';
import { useCreateService } from '@/lib/services';
import { ApiError } from '@/lib/api';
import type { RestartPolicy } from '@/lib/types';

interface Props {
  workspaceSlug: string;
  projectSlug: string;
  children: ReactNode;
}

export function NewServiceSheet({ workspaceSlug, projectSlug, children }: Props) {
  const create = useCreateService(workspaceSlug, projectSlug);
  const [open, setOpen] = useState(false);

  const [slug, setSlug] = useState('');
  const [name, setName] = useState('');
  const [image, setImage] = useState('');
  const [restartPolicy, setRestartPolicy] = useState<RestartPolicy>('on-failure');
  const [portsRaw, setPortsRaw] = useState('');
  const [envRaw, setEnvRaw] = useState('');
  const [cpuMillis, setCpuMillis] = useState('');
  const [memoryMb, setMemoryMb] = useState('');
  const [diskMb, setDiskMb] = useState('');
  const [error, setError] = useState<string | null>(null);

  function reset() {
    setSlug('');
    setName('');
    setImage('');
    setRestartPolicy('on-failure');
    setPortsRaw('');
    setEnvRaw('');
    setCpuMillis('');
    setMemoryMb('');
    setDiskMb('');
    setError(null);
  }

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const ports = parsePorts(portsRaw);
      const env_vars = parseEnv(envRaw);
      const resources =
        cpuMillis || memoryMb || diskMb
          ? {
              cpu_millis: parsePositiveInt(cpuMillis, 500, 'CPU millis'),
              memory_mb: parsePositiveInt(memoryMb, 256, 'memory MB'),
              disk_mb: parsePositiveInt(diskMb, 1024, 'disk MB'),
            }
          : undefined;
      await create.mutateAsync({
        slug,
        name,
        image_ref: image,
        restart_policy: restartPolicy,
        ports,
        env_vars,
        resources,
      });
      reset();
      setOpen(false);
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : err instanceof Error ? err.message : 'Failed',
      );
    }
  }

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) reset();
      }}
    >
      <SheetTrigger asChild>{children}</SheetTrigger>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>New service</SheetTitle>
          <SheetDescription>
            Run a container from an image. You can edit env and ports after creation.
          </SheetDescription>
        </SheetHeader>
        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-4">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Slug" htmlFor="svc-slug">
              <Input
                id="svc-slug"
                required
                placeholder="web"
                value={slug}
                onChange={(e) => setSlug(e.target.value)}
              />
            </Field>
            <Field label="Name" htmlFor="svc-name">
              <Input
                id="svc-name"
                required
                placeholder="Web frontend"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </Field>
          </div>
          <Field label="Image" htmlFor="svc-image" hint="e.g. nginx:latest or ghcr.io/org/app:v1">
            <Input
              id="svc-image"
              required
              placeholder="nginx:latest"
              value={image}
              onChange={(e) => setImage(e.target.value)}
            />
          </Field>
          <Field
            label="Ports"
            htmlFor="svc-ports"
            hint="One per line: 80 or 80:8080 or 80:8080/tcp"
          >
            <textarea
              id="svc-ports"
              rows={3}
              value={portsRaw}
              onChange={(e) => setPortsRaw(e.target.value)}
              className="w-full rounded-md border border-[var(--color-border)] bg-transparent p-2 font-mono text-xs focus:border-[var(--color-accent)] focus:outline-none"
            />
          </Field>
          <Field label="Env vars" htmlFor="svc-env" hint="One KEY=value per line">
            <textarea
              id="svc-env"
              rows={4}
              value={envRaw}
              onChange={(e) => setEnvRaw(e.target.value)}
              className="w-full rounded-md border border-[var(--color-border)] bg-transparent p-2 font-mono text-xs focus:border-[var(--color-accent)] focus:outline-none"
            />
          </Field>
          <Field label="Restart policy" htmlFor="svc-restart">
            <Select
              id="svc-restart"
              value={restartPolicy}
              onChange={(e) => setRestartPolicy(e.target.value as RestartPolicy)}
            >
              <option value="no">no</option>
              <option value="on-failure">on-failure</option>
              <option value="always">always</option>
            </Select>
          </Field>

          <div className="grid grid-cols-3 gap-3">
            <Field label="CPU (millis)" htmlFor="svc-cpu" hint="Default 500 (½ core)">
              <Input
                id="svc-cpu"
                type="number"
                min={1}
                placeholder="500"
                value={cpuMillis}
                onChange={(e) => setCpuMillis(e.target.value)}
              />
            </Field>
            <Field label="Memory (MB)" htmlFor="svc-mem" hint="Default 256">
              <Input
                id="svc-mem"
                type="number"
                min={1}
                placeholder="256"
                value={memoryMb}
                onChange={(e) => setMemoryMb(e.target.value)}
              />
            </Field>
            <Field label="Disk (MB)" htmlFor="svc-disk" hint="Default 1024">
              <Input
                id="svc-disk"
                type="number"
                min={1}
                placeholder="1024"
                value={diskMb}
                onChange={(e) => setDiskMb(e.target.value)}
              />
            </Field>
          </div>
          {error ? <ErrorText>{error}</ErrorText> : null}
          <div className="mt-auto flex justify-end gap-2 pt-4">
            <Button type="button" variant="secondary" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? 'Creating…' : 'Create service'}
            </Button>
          </div>
        </form>
      </SheetContent>
    </Sheet>
  );
}

function parsePositiveInt(raw: string, fallback: number, label: string): number {
  if (!raw.trim()) return fallback;
  const n = Number(raw);
  if (!Number.isFinite(n) || !Number.isInteger(n) || n < 1) {
    throw new Error(`${label} must be a positive integer`);
  }
  return n;
}

function parsePorts(raw: string) {
  const out: { container_port: number; host_port: number | null; protocol: string }[] = [];
  for (const line of raw.split('\n')) {
    const t = line.trim();
    if (!t) continue;
    const [mapping, proto = 'tcp'] = t.split('/');
    const parts = mapping.split(':');
    const cp = Number(parts[0]);
    if (!Number.isFinite(cp)) throw new Error(`invalid port: ${line}`);
    const hp = parts[1] ? Number(parts[1]) : null;
    if (hp !== null && !Number.isFinite(hp)) throw new Error(`invalid port: ${line}`);
    out.push({ container_port: cp, host_port: hp, protocol: proto });
  }
  return out;
}

function parseEnv(raw: string) {
  const out: Record<string, string> = {};
  for (const line of raw.split('\n')) {
    const t = line.trim();
    if (!t || t.startsWith('#')) continue;
    const eq = t.indexOf('=');
    if (eq < 0) throw new Error(`invalid env: ${line}`);
    out[t.slice(0, eq)] = t.slice(eq + 1);
  }
  return out;
}
