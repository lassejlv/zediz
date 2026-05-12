import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useState, type FormEvent } from 'react';
import { ArrowLeft, Container, GitBranch, Plus, Trash2 } from 'lucide-react';
import { meQuery } from '@/lib/auth';
import { projectQuery } from '@/lib/projects';
import {
  useCreateService,
  useVariableReferences,
  type CreateServiceInput,
} from '@/lib/services';
import { useCredentials } from '@/lib/credentials';
import { githubRepositoriesQuery, type GitHubRepositorySummary } from '@/lib/github';
import { usePublicSettings } from '@/lib/settings';
import { canWrite, workspaceQuery } from '@/lib/workspaces';
import { ApiError } from '@/lib/api';
import { EnvReferenceInput } from '@/components/env-reference-input';
import {
  Button,
  Card,
  ErrorText,
  Field,
  Input,
  Select,
  Stack,
} from '@/components/ui';
import type {
  CredentialSummary,
  PortMap,
  RestartPolicy,
  VariableReferencesResponse,
} from '@/lib/types';

export const Route = createFileRoute('/w/$workspaceSlug/projects/$projectSlug/new')({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) throw redirect({ to: '/login' });
  },
  component: NewServicePage,
});

type Source = 'image' | 'git';

interface PortRow {
  container: string;
  host: string;
  protocol: 'tcp' | 'udp';
}

interface EnvRow {
  key: string;
  value: string;
}

function NewServicePage() {
  const { workspaceSlug, projectSlug } = Route.useParams();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const project = useQuery(projectQuery(workspaceSlug, projectSlug));
  const credentials = useCredentials(workspaceSlug);
  const settings = usePublicSettings();
  const githubRepositories = useQuery({
    ...githubRepositoriesQuery(workspaceSlug),
    enabled: !!settings.data?.github_app_configured,
  });
  const variableReferences = useVariableReferences(workspaceSlug, projectSlug);
  const create = useCreateService(workspaceSlug, projectSlug);
  const navigate = useNavigate();

  const canCreate = canWrite(workspace.data);

  const [source, setSource] = useState<Source>('image');

  const [slug, setSlug] = useState('');
  const [slugTouched, setSlugTouched] = useState(false);
  const [name, setName] = useState('');
  const [image, setImage] = useState('');

  const [gitRepo, setGitRepo] = useState('');
  const [gitBranch, setGitBranch] = useState('main');
  const [builder, setBuilder] = useState<'dockerfile' | 'railpack'>('dockerfile');
  const [dockerfilePath, setDockerfilePath] = useState('Dockerfile');
  const [rootDir, setRootDir] = useState('.');
  const [registryRepo, setRegistryRepo] = useState('');
  const [githubCredId, setGithubCredId] = useState('');
  const [githubRepoKey, setGithubRepoKey] = useState('');
  const [registryCredId, setRegistryCredId] = useState('');

  const [ports, setPorts] = useState<PortRow[]>([]);
  const [envRows, setEnvRows] = useState<EnvRow[]>([]);
  const [restartPolicy, setRestartPolicy] = useState<RestartPolicy>('on-failure');

  const [cpuMillis, setCpuMillis] = useState(500);
  const [memoryMb, setMemoryMb] = useState(256);
  const [diskMb, setDiskMb] = useState(1024);

  const [error, setError] = useState<string | null>(null);

  const registryCreds = useMemo(
    () => (credentials.data ?? []).filter((c) => c.kind === 'registry'),
    [credentials.data],
  );
  const githubCreds = useMemo(
    () => (credentials.data ?? []).filter((c) => c.kind === 'github_pat'),
    [credentials.data],
  );
  const githubRepos = githubRepositories.data ?? [];

  function onNameChange(next: string) {
    setName(next);
    if (!slugTouched) setSlug(slugify(next));
  }

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const parsedPorts = parsePorts(ports);
      const env_vars = parseEnv(envRows);

      const base: CreateServiceInput = {
        slug: slug.trim(),
        name: name.trim(),
        source,
        restart_policy: restartPolicy,
        ports: parsedPorts,
        env_vars,
        resources: {
          cpu_millis: cpuMillis,
          memory_mb: memoryMb,
          disk_mb: diskMb,
        },
      };

      if (source === 'image') {
        if (!image.trim()) throw new Error('Image reference is required');
        base.image_ref = image.trim();
      } else {
        const selectedGithubRepo = githubRepos.find(
          (repo) => githubRepoKey === githubRepoValue(repo),
        );
        if (!selectedGithubRepo && !gitRepo.trim()) {
          throw new Error('Git repo URL or GitHub App repository is required');
        }
        if (!registryCredId) throw new Error('Pick a registry credential');
        base.git_repo = selectedGithubRepo?.clone_url ?? gitRepo.trim();
        base.git_branch = gitBranch.trim() || 'main';
        base.builder = builder;
        base.root_dir = rootDir.trim() || '.';
        if (builder === 'dockerfile') {
          base.dockerfile_path = dockerfilePath.trim() || 'Dockerfile';
        }
        if (registryRepo.trim()) base.registry_repo = registryRepo.trim();
        base.registry_credential_id = registryCredId;
        if (githubCredId) base.github_credential_id = githubCredId;
        if (selectedGithubRepo) {
          base.github_installation_id = selectedGithubRepo.installation_id;
          base.github_repository_id = selectedGithubRepo.repository_id;
          base.github_repository_full_name = selectedGithubRepo.full_name;
          base.github_auto_deploy = true;
          base.github_statuses_enabled = true;
        }
      }

      const created = await create.mutateAsync(base);
      navigate({
        to: '/w/$workspaceSlug/projects/$projectSlug/$serviceSlug',
        params: {
          workspaceSlug,
          projectSlug,
          serviceSlug: created.slug,
        },
      });
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : 'Something went wrong',
      );
    }
  }

  if (!canCreate && workspace.data) {
    return (
      <div className="mx-auto max-w-2xl py-12 text-center">
        <p className="text-sm text-[var(--color-muted)]">
          You need write access to create a service in this project.
        </p>
      </div>
    );
  }

  return (
    <form
      onSubmit={onSubmit}
      className="mx-auto flex max-w-3xl flex-col gap-6 pb-40"
    >
      <div>
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug"
          params={{ workspaceSlug, projectSlug }}
          className="inline-flex items-center gap-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <ArrowLeft className="h-3 w-3" /> {project.data?.name ?? projectSlug}
        </Link>
        <h1 className="mt-2 text-2xl font-semibold tracking-tight">New service</h1>
        <p className="mt-1 text-sm text-[var(--color-muted)]">
          Run a container from a published image, or build one from a Git repo on every deploy.
        </p>
      </div>

      <Section title="Source" description="Where the container image comes from.">
        <SourcePicker value={source} onChange={setSource} />
      </Section>

      <Section title="Basics">
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label="Display name" htmlFor="svc-name" hint="Shown everywhere in the UI.">
            <Input
              id="svc-name"
              required
              placeholder="Web frontend"
              value={name}
              onChange={(e) => onNameChange(e.target.value)}
            />
          </Field>
          <Field
            label="Slug"
            htmlFor="svc-slug"
            hint="URL-safe; auto-filled from the name. Lowercase, digits, dashes."
          >
            <Input
              id="svc-slug"
              required
              placeholder="web"
              value={slug}
              onChange={(e) => {
                setSlug(e.target.value);
                setSlugTouched(true);
              }}
            />
          </Field>
        </div>
      </Section>

      {source === 'image' ? (
        <Section
          title="Image"
          description="The container image the agent pulls on every deploy."
        >
          <Field label="Image reference" htmlFor="svc-image">
            <Input
              id="svc-image"
              required
              placeholder="ghcr.io/org/app:v1"
              value={image}
              onChange={(e) => setImage(e.target.value)}
            />
          </Field>
        </Section>
      ) : (
        <Section title="Git source" description="Driftbase builds the image from this repo on deploy.">
          {settings.data?.github_app_configured ? (
            <Field
              label="GitHub App repository"
              htmlFor="svc-github-app-repo"
              hint={
                githubRepos.length
                  ? 'Pushes to the selected branch will auto-deploy.'
                  : 'Connect the GitHub App from Credentials to select repositories.'
              }
            >
              <Select
                id="svc-github-app-repo"
                value={githubRepoKey}
                onChange={(e) => {
                  const key = e.target.value;
                  setGithubRepoKey(key);
                  const repo = githubRepos.find((r) => githubRepoValue(r) === key);
                  if (repo) {
                    setGitRepo(repo.clone_url);
                    setGitBranch(repo.default_branch || 'main');
                  }
                }}
              >
                <option value="">— manual URL / PAT fallback —</option>
                {githubRepos.map((repo) => (
                  <option key={githubRepoValue(repo)} value={githubRepoValue(repo)}>
                    {repo.full_name}
                  </option>
                ))}
              </Select>
            </Field>
          ) : null}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-[2fr_1fr]">
            <Field label="Repository URL" htmlFor="svc-git-repo">
              <Input
                id="svc-git-repo"
                required
                placeholder="https://github.com/org/repo"
                value={gitRepo}
                disabled={!!githubRepoKey}
                onChange={(e) => setGitRepo(e.target.value)}
              />
            </Field>
            <Field label="Branch" htmlFor="svc-git-branch">
              <Input
                id="svc-git-branch"
                placeholder="main"
                value={gitBranch}
                onChange={(e) => setGitBranch(e.target.value)}
              />
            </Field>
          </div>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Field
              label="Root directory"
              htmlFor="svc-root"
              hint="Subdir for monorepos. The build and Railpack scan both run here."
            >
              <Input
                id="svc-root"
                placeholder="."
                value={rootDir}
                onChange={(e) => setRootDir(e.target.value)}
              />
            </Field>
            <Field
              label="Builder"
              htmlFor="svc-builder"
              hint={
                builder === 'railpack'
                  ? 'Railpack auto-detects the stack. No Dockerfile needed.'
                  : 'Dockerfile path is relative to the root directory.'
              }
            >
              <Select
                id="svc-builder"
                value={builder}
                onChange={(e) => setBuilder(e.target.value as 'dockerfile' | 'railpack')}
              >
                <option value="dockerfile">Dockerfile</option>
                <option value="railpack">Railpack (auto-detect)</option>
              </Select>
            </Field>
          </div>
          {builder === 'dockerfile' ? (
            <Field label="Dockerfile path" htmlFor="svc-dockerfile">
              <Input
                id="svc-dockerfile"
                placeholder="Dockerfile"
                value={dockerfilePath}
                onChange={(e) => setDockerfilePath(e.target.value)}
              />
            </Field>
          ) : null}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <Field
              label="Registry credential"
              htmlFor="svc-registry-cred"
              hint={
                registryCreds.length === 0
                  ? 'Add a "registry" credential first (in Credentials).'
                  : 'Where Driftbase pushes the built image.'
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
                  <CredOption key={c.id} c={c} />
                ))}
              </Select>
            </Field>
            <Field
              label="GitHub PAT"
              htmlFor="svc-github-cred"
              hint="Legacy fallback for private repos not connected through the GitHub App."
            >
              <Select
                id="svc-github-cred"
                value={githubCredId}
                onChange={(e) => setGithubCredId(e.target.value)}
              >
                <option value="">— none —</option>
                {githubCreds.map((c) => (
                  <CredOption key={c.id} c={c} />
                ))}
              </Select>
            </Field>
          </div>
          <Field
            label="Registry path override"
            htmlFor="svc-registry-repo"
            hint="Leave empty to use the default registry/workspace/service layout."
          >
            <Input
              id="svc-registry-repo"
              placeholder="registry.driftbase.app/team/api"
              value={registryRepo}
              onChange={(e) => setRegistryRepo(e.target.value)}
            />
          </Field>
        </Section>
      )}

      <Section
        title="Ports"
        description="Expose container ports. Host port is optional — domains reach the container via the internal network, so most services leave it blank."
      >
        <PortsEditor rows={ports} onChange={setPorts} />
      </Section>

      <Section
        title="Environment variables"
        description="Injected into the container at launch."
      >
        <EnvEditor
          rows={envRows}
          onChange={setEnvRows}
          references={variableReferences.data}
        />
      </Section>

      <Section title="Resources" description="Reserved on the node that runs this service.">
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          <NumberField
            id="svc-cpu"
            label="CPU"
            value={cpuMillis}
            onChange={setCpuMillis}
            min={100}
            max={8000}
            step={100}
            suffix="millis"
            hint={`≈ ${(cpuMillis / 1000).toFixed(2)} cores`}
          />
          <NumberField
            id="svc-mem"
            label="Memory"
            value={memoryMb}
            onChange={setMemoryMb}
            min={64}
            max={32_768}
            step={64}
            suffix="MB"
          />
          <NumberField
            id="svc-disk"
            label="Disk"
            value={diskMb}
            onChange={setDiskMb}
            min={512}
            max={102_400}
            step={256}
            suffix="MB"
          />
        </div>
      </Section>

      <Section title="Runtime">
        <Field label="Restart policy" htmlFor="svc-restart">
          <Select
            id="svc-restart"
            value={restartPolicy}
            onChange={(e) => setRestartPolicy(e.target.value as RestartPolicy)}
          >
            <option value="no">no (never restart)</option>
            <option value="on-failure">on-failure</option>
            <option value="always">always</option>
          </Select>
        </Field>
      </Section>

      {error ? <ErrorText>{error}</ErrorText> : null}

      <div className="fixed inset-x-0 bottom-0 z-10 border-t border-[var(--color-border)] bg-[var(--color-bg)]/90 backdrop-blur">
        <div className="mx-auto flex max-w-3xl items-center justify-between px-6 py-3">
          <span className="text-xs text-[var(--color-muted)]">
            Creates the service — nothing is deployed yet.
          </span>
          <div className="flex gap-2">
            <Link
              to="/w/$workspaceSlug/projects/$projectSlug"
              params={{ workspaceSlug, projectSlug }}
            >
              <Button type="button" variant="secondary">
                Cancel
              </Button>
            </Link>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? 'Creating…' : 'Create service'}
            </Button>
          </div>
        </div>
      </div>
    </form>
  );
}

/* ---------- layout ---------- */

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <Card className="p-5">
      <div className="mb-4">
        <h2 className="text-sm font-medium">{title}</h2>
        {description ? (
          <p className="mt-0.5 text-xs text-[var(--color-muted)]">{description}</p>
        ) : null}
      </div>
      <Stack gap={4}>{children}</Stack>
    </Card>
  );
}

function SourcePicker({
  value,
  onChange,
}: {
  value: Source;
  onChange: (v: Source) => void;
}) {
  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <SourceOption
        selected={value === 'image'}
        onClick={() => onChange('image')}
        icon={<Container className="h-5 w-5" />}
        title="Container image"
        body="Pull a published image on each deploy. Fastest to set up."
      />
      <SourceOption
        selected={value === 'git'}
        onClick={() => onChange('git')}
        icon={<GitBranch className="h-5 w-5" />}
        title="Git repository"
        body="Driftbase clones, builds, and pushes to the workspace registry."
      />
    </div>
  );
}

function SourceOption({
  selected,
  onClick,
  icon,
  title,
  body,
}: {
  selected: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        'flex flex-col items-start gap-2 rounded-lg border p-4 text-left transition-colors',
        selected
          ? 'border-[var(--color-accent)] bg-[var(--color-accent)]/5'
          : 'border-[var(--color-border)] hover:border-[var(--color-border-strong)] hover:bg-black/[0.02] dark:hover:bg-white/[0.02]',
      ].join(' ')}
    >
      <span
        className={
          selected ? 'text-[var(--color-accent)]' : 'text-[var(--color-muted)]'
        }
      >
        {icon}
      </span>
      <div>
        <div className="text-sm font-medium">{title}</div>
        <div className="mt-0.5 text-xs text-[var(--color-muted)]">{body}</div>
      </div>
    </button>
  );
}

/* ---------- ports editor ---------- */

function PortsEditor({
  rows,
  onChange,
}: {
  rows: PortRow[];
  onChange: (rows: PortRow[]) => void;
}) {
  function update(i: number, patch: Partial<PortRow>) {
    onChange(rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  }
  function remove(i: number) {
    onChange(rows.filter((_, idx) => idx !== i));
  }
  function add() {
    onChange([...rows, { container: '', host: '', protocol: 'tcp' }]);
  }

  if (rows.length === 0) {
    return (
      <div className="flex items-center gap-3 rounded-md border border-dashed border-[var(--color-border)] p-4 text-xs text-[var(--color-muted)]">
        <span>No ports exposed. Add one if the container listens on a port.</span>
        <Button type="button" variant="secondary" onClick={add} className="ml-auto">
          <Plus className="mr-1.5 h-4 w-4" /> Add port
        </Button>
      </div>
    );
  }

  return (
    <Stack gap={2}>
      <div className="grid grid-cols-[1fr_1fr_100px_auto] gap-2 text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
        <span>Container port</span>
        <span>Host port (optional)</span>
        <span>Protocol</span>
        <span />
      </div>
      {rows.map((r, i) => (
        <div key={i} className="grid grid-cols-[1fr_1fr_100px_auto] items-center gap-2">
          <Input
            type="number"
            min={1}
            max={65_535}
            placeholder="3000"
            value={r.container}
            onChange={(e) => update(i, { container: e.target.value })}
          />
          <Input
            type="number"
            min={1}
            max={65_535}
            placeholder="blank for internal only"
            value={r.host}
            onChange={(e) => update(i, { host: e.target.value })}
          />
          <Select
            value={r.protocol}
            onChange={(e) => update(i, { protocol: e.target.value as 'tcp' | 'udp' })}
          >
            <option value="tcp">TCP</option>
            <option value="udp">UDP</option>
          </Select>
          <button
            type="button"
            onClick={() => remove(i)}
            aria-label="Remove port"
            className="inline-flex h-9 w-9 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-red-500/10 hover:text-red-400"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      ))}
      <div>
        <Button type="button" variant="secondary" onClick={add}>
          <Plus className="mr-1.5 h-4 w-4" /> Add port
        </Button>
      </div>
    </Stack>
  );
}

/* ---------- env editor ---------- */

function EnvEditor({
  rows,
  onChange,
  references,
}: {
  rows: EnvRow[];
  onChange: (rows: EnvRow[]) => void;
  references?: VariableReferencesResponse;
}) {
  function update(i: number, patch: Partial<EnvRow>) {
    onChange(rows.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  }
  function remove(i: number) {
    onChange(rows.filter((_, idx) => idx !== i));
  }
  function add() {
    onChange([...rows, { key: '', value: '' }]);
  }

  if (rows.length === 0) {
    return (
      <div className="flex items-center gap-3 rounded-md border border-dashed border-[var(--color-border)] p-4 text-xs text-[var(--color-muted)]">
        <span>No environment variables set.</span>
        <Button type="button" variant="secondary" onClick={add} className="ml-auto">
          <Plus className="mr-1.5 h-4 w-4" /> Add variable
        </Button>
      </div>
    );
  }

  return (
    <Stack gap={2}>
      {rows.map((r, i) => (
        <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)_auto] gap-2">
          <Input
            placeholder="KEY"
            value={r.key}
            onChange={(e) => update(i, { key: e.target.value })}
            className="font-mono text-xs"
          />
          <EnvReferenceInput
            placeholder="value"
            value={r.value}
            onChange={(value) => update(i, { value })}
            references={references}
            className="font-mono text-xs"
          />
          <button
            type="button"
            onClick={() => remove(i)}
            aria-label="Remove variable"
            className="inline-flex h-9 w-9 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-red-500/10 hover:text-red-400"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      ))}
      <div>
        <Button type="button" variant="secondary" onClick={add}>
          <Plus className="mr-1.5 h-4 w-4" /> Add variable
        </Button>
      </div>
    </Stack>
  );
}

/* ---------- number field ---------- */

function NumberField({
  id,
  label,
  value,
  onChange,
  min,
  max,
  step,
  suffix,
  hint,
}: {
  id: string;
  label: string;
  value: number;
  onChange: (n: number) => void;
  min: number;
  max: number;
  step: number;
  suffix: string;
  hint?: string;
}) {
  return (
    <Field label={label} htmlFor={id} hint={hint}>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          type="number"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => {
            const n = Number(e.target.value);
            if (Number.isFinite(n)) onChange(n);
          }}
          className="font-mono"
        />
        <span className="text-xs text-[var(--color-muted)]">{suffix}</span>
      </div>
    </Field>
  );
}

function CredOption({ c }: { c: CredentialSummary }) {
  const url =
    typeof c.metadata === 'object' && c.metadata && 'url' in c.metadata
      ? String((c.metadata as Record<string, unknown>).url ?? '')
      : '';
  return (
    <option value={c.id}>
      {c.name}
      {url ? ` · ${url}` : ''}
    </option>
  );
}

/* ---------- helpers ---------- */

function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40);
}

function githubRepoValue(repo: GitHubRepositorySummary): string {
  return `${repo.installation_id}:${repo.repository_id}`;
}

function parsePorts(rows: PortRow[]): PortMap[] {
  const out: PortMap[] = [];
  for (const r of rows) {
    const container = r.container.trim();
    if (!container) continue;
    const cp = Number(container);
    if (!Number.isInteger(cp) || cp < 1 || cp > 65_535) {
      throw new Error(`Invalid container port: ${container}`);
    }
    let hp: number | null = null;
    if (r.host.trim()) {
      const n = Number(r.host);
      if (!Number.isInteger(n) || n < 1 || n > 65_535) {
        throw new Error(`Invalid host port: ${r.host}`);
      }
      hp = n;
    }
    out.push({ container_port: cp, host_port: hp, protocol: r.protocol });
  }
  return out;
}

function parseEnv(rows: EnvRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  const seen = new Set<string>();
  for (const r of rows) {
    const key = r.key.trim();
    if (!key) continue;
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      throw new Error(
        `Invalid env var name "${key}" — use letters, digits, underscore.`,
      );
    }
    if (seen.has(key)) throw new Error(`Duplicate env var: ${key}`);
    seen.add(key);
    out[key] = r.value;
  }
  return out;
}
