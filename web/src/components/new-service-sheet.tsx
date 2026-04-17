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
import { useCreateService, type CreateServiceInput } from '@/lib/services';
import { useCredentials } from '@/lib/credentials';
import { ApiError } from '@/lib/api';
import type { CredentialSummary, RestartPolicy } from '@/lib/types';

interface Props {
  workspaceSlug: string;
  projectSlug: string;
  children: ReactNode;
}

type Source = 'image' | 'git';

export function NewServiceSheet({ workspaceSlug, projectSlug, children }: Props) {
  const create = useCreateService(workspaceSlug, projectSlug);
  const credentials = useCredentials(workspaceSlug);
  const [open, setOpen] = useState(false);

  const [source, setSource] = useState<Source>('image');

  const [slug, setSlug] = useState('');
  const [name, setName] = useState('');
  const [image, setImage] = useState('');
  const [restartPolicy, setRestartPolicy] = useState<RestartPolicy>('on-failure');
  const [portsRaw, setPortsRaw] = useState('');
  const [envRaw, setEnvRaw] = useState('');
  const [cpuMillis, setCpuMillis] = useState('');
  const [memoryMb, setMemoryMb] = useState('');
  const [diskMb, setDiskMb] = useState('');

  // Git-only fields
  const [gitRepo, setGitRepo] = useState('');
  const [gitBranch, setGitBranch] = useState('main');
  const [dockerfilePath, setDockerfilePath] = useState('Dockerfile');
  const [buildContext, setBuildContext] = useState('.');
  const [registryRepo, setRegistryRepo] = useState('');
  const [githubCredId, setGithubCredId] = useState('');
  const [registryCredId, setRegistryCredId] = useState('');

  const [error, setError] = useState<string | null>(null);

  function reset() {
    setSource('image');
    setSlug('');
    setName('');
    setImage('');
    setRestartPolicy('on-failure');
    setPortsRaw('');
    setEnvRaw('');
    setCpuMillis('');
    setMemoryMb('');
    setDiskMb('');
    setGitRepo('');
    setGitBranch('main');
    setDockerfilePath('Dockerfile');
    setBuildContext('.');
    setRegistryRepo('');
    setGithubCredId('');
    setRegistryCredId('');
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

      const base: CreateServiceInput = {
        slug,
        name,
        source,
        restart_policy: restartPolicy,
        ports,
        env_vars,
        resources,
      };

      if (source === 'image') {
        if (!image.trim()) throw new Error('image is required');
        base.image_ref = image.trim();
      } else {
        if (!gitRepo.trim()) throw new Error('git repo URL is required');
        if (!registryCredId) throw new Error('pick a registry credential');
        base.git_repo = gitRepo.trim();
        base.git_branch = gitBranch.trim() || 'main';
        base.dockerfile_path = dockerfilePath.trim() || 'Dockerfile';
        base.build_context = buildContext.trim() || '.';
        if (registryRepo.trim()) base.registry_repo = registryRepo.trim();
        base.registry_credential_id = registryCredId;
        if (githubCredId) base.github_credential_id = githubCredId;
      }

      await create.mutateAsync(base);
      reset();
      setOpen(false);
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : err instanceof Error ? err.message : 'Failed',
      );
    }
  }

  const registryCreds = (credentials.data ?? []).filter((c) => c.kind === 'registry');
  const githubCreds = (credentials.data ?? []).filter((c) => c.kind === 'github_pat');

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
            Run a container from a published image, or build one from a Git repo on deploy.
          </SheetDescription>
        </SheetHeader>
        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-4">
          <SourceToggle value={source} onChange={setSource} />

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

          {source === 'image' ? (
            <Field
              label="Image"
              htmlFor="svc-image"
              hint="e.g. nginx:latest or ghcr.io/org/app:v1"
            >
              <Input
                id="svc-image"
                required
                placeholder="nginx:latest"
                value={image}
                onChange={(e) => setImage(e.target.value)}
              />
            </Field>
          ) : (
            <GitFields
              registryCreds={registryCreds}
              githubCreds={githubCreds}
              gitRepo={gitRepo}
              setGitRepo={setGitRepo}
              gitBranch={gitBranch}
              setGitBranch={setGitBranch}
              dockerfilePath={dockerfilePath}
              setDockerfilePath={setDockerfilePath}
              buildContext={buildContext}
              setBuildContext={setBuildContext}
              registryRepo={registryRepo}
              setRegistryRepo={setRegistryRepo}
              githubCredId={githubCredId}
              setGithubCredId={setGithubCredId}
              registryCredId={registryCredId}
              setRegistryCredId={setRegistryCredId}
            />
          )}

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

function SourceToggle({ value, onChange }: { value: Source; onChange: (v: Source) => void }) {
  return (
    <div
      role="tablist"
      className="inline-flex rounded-md border border-[var(--color-border)] p-0.5 text-xs"
    >
      {(['image', 'git'] as const).map((s) => (
        <button
          key={s}
          type="button"
          role="tab"
          aria-selected={value === s}
          onClick={() => onChange(s)}
          className={[
            'rounded px-3 py-1 transition-colors',
            value === s
              ? 'bg-black/5 text-[var(--color-fg)] dark:bg-white/10'
              : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
          ].join(' ')}
        >
          {s === 'image' ? 'Image' : 'Git repo'}
        </button>
      ))}
    </div>
  );
}

function GitFields({
  registryCreds,
  githubCreds,
  gitRepo,
  setGitRepo,
  gitBranch,
  setGitBranch,
  dockerfilePath,
  setDockerfilePath,
  buildContext,
  setBuildContext,
  registryRepo,
  setRegistryRepo,
  githubCredId,
  setGithubCredId,
  registryCredId,
  setRegistryCredId,
}: {
  registryCreds: CredentialSummary[];
  githubCreds: CredentialSummary[];
  gitRepo: string;
  setGitRepo: (v: string) => void;
  gitBranch: string;
  setGitBranch: (v: string) => void;
  dockerfilePath: string;
  setDockerfilePath: (v: string) => void;
  buildContext: string;
  setBuildContext: (v: string) => void;
  registryRepo: string;
  setRegistryRepo: (v: string) => void;
  githubCredId: string;
  setGithubCredId: (v: string) => void;
  registryCredId: string;
  setRegistryCredId: (v: string) => void;
}) {
  return (
    <div className="flex flex-col gap-3 rounded-md border border-[var(--color-border)] p-3">
      <Field
        label="Repo URL"
        htmlFor="svc-git-repo"
        hint="https://github.com/org/repo[.git]"
      >
        <Input
          id="svc-git-repo"
          required
          placeholder="https://github.com/org/repo"
          value={gitRepo}
          onChange={(e) => setGitRepo(e.target.value)}
        />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Branch" htmlFor="svc-git-branch">
          <Input
            id="svc-git-branch"
            placeholder="main"
            value={gitBranch}
            onChange={(e) => setGitBranch(e.target.value)}
          />
        </Field>
        <Field label="Dockerfile" htmlFor="svc-dockerfile">
          <Input
            id="svc-dockerfile"
            placeholder="Dockerfile"
            value={dockerfilePath}
            onChange={(e) => setDockerfilePath(e.target.value)}
          />
        </Field>
      </div>
      <Field
        label="Build context"
        htmlFor="svc-build-context"
        hint="Dir passed to `docker buildx build`, relative to the repo root."
      >
        <Input
          id="svc-build-context"
          placeholder="."
          value={buildContext}
          onChange={(e) => setBuildContext(e.target.value)}
        />
      </Field>
      <Field
        label="Registry credential"
        htmlFor="svc-registry-cred"
        hint={
          registryCreds.length === 0
            ? 'Add a credential (kind: registry) with the registry URL in metadata first.'
            : undefined
        }
      >
        <Select
          id="svc-registry-cred"
          required
          value={registryCredId}
          onChange={(e) => setRegistryCredId(e.target.value)}
        >
          <option value="">— select —</option>
          {registryCreds.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </Select>
      </Field>
      <Field
        label="GitHub PAT (for private repos)"
        htmlFor="svc-github-cred"
        hint="Leave blank for public repos."
      >
        <Select
          id="svc-github-cred"
          value={githubCredId}
          onChange={(e) => setGithubCredId(e.target.value)}
        >
          <option value="">— none —</option>
          {githubCreds.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </Select>
      </Field>
      <Field
        label="Registry repo override (optional)"
        htmlFor="svc-registry-repo"
        hint="Defaults to <credential-url>/<workspace>/<service>."
      >
        <Input
          id="svc-registry-repo"
          placeholder="registry.zediz.dev/team/api"
          value={registryRepo}
          onChange={(e) => setRegistryRepo(e.target.value)}
        />
      </Field>
    </div>
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
