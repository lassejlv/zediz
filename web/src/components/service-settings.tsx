import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { Check, Eye, EyeOff, Plus, Trash2 } from 'lucide-react';
import {
  useUpdateService,
  useVariableReferences,
  type UpdateServiceInput,
} from '@/lib/services';
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
  EnvVars,
  Resources,
  RestartPolicy,
  ServiceBuilder,
  ServiceSummary,
  VariableReferencesResponse,
} from '@/lib/types';

interface Props {
  service: ServiceSummary;
  workspaceSlug: string;
  projectSlug: string;
  canManage: boolean;
}

export function ServiceSettingsTab({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  return (
    <Stack gap={4}>
      <GeneralSection
        service={service}
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        canManage={canManage}
      />
      <EnvVarsSection
        service={service}
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        canManage={canManage}
      />
      {service.source === 'image' ? (
        <ImageSourceSection
          service={service}
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          canManage={canManage}
        />
      ) : (
        <GitSourceSection
          service={service}
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          canManage={canManage}
        />
      )}
      <ResourcesSection
        service={service}
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        canManage={canManage}
      />
      <RuntimeSection
        service={service}
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        canManage={canManage}
      />
    </Stack>
  );
}

/* ---------- section wrapper ---------- */

function SectionCard({
  title,
  description,
  children,
  footer,
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <Card className="p-5">
      <div className="mb-4">
        <h3 className="text-sm font-medium">{title}</h3>
        {description ? (
          <p className="mt-0.5 text-xs text-[var(--color-muted)]">{description}</p>
        ) : null}
      </div>
      {children}
      {footer ? (
        <div className="mt-4 flex items-center justify-end gap-3 border-t border-[var(--color-border)] pt-4">
          {footer}
        </div>
      ) : null}
    </Card>
  );
}

/** Hook that wires a per-section form: tracks dirty state, submission, and
 * a transient "Saved" confirmation. The parent supplies the update payload
 * to send on save. */
function useSectionSave(
  workspaceSlug: string,
  projectSlug: string,
  serviceSlug: string,
) {
  const update = useUpdateService(workspaceSlug, projectSlug, serviceSlug);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState(0);

  useEffect(() => {
    if (!savedAt) return;
    const id = setTimeout(() => setSavedAt(0), 2000);
    return () => clearTimeout(id);
  }, [savedAt]);

  async function save(patch: UpdateServiceInput) {
    setError(null);
    try {
      await update.mutateAsync(patch);
      setSavedAt(Date.now());
      return true;
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : 'Failed',
      );
      return false;
    }
  }

  return {
    save,
    saving: update.isPending,
    saved: savedAt !== 0,
    error,
    clearError: () => setError(null),
  };
}

function SaveRow({
  dirty,
  saving,
  saved,
  error,
  onReset,
  canManage,
}: {
  dirty: boolean;
  saving: boolean;
  saved: boolean;
  error: string | null;
  onReset: () => void;
  canManage: boolean;
}) {
  return (
    <>
      {error ? <ErrorText>{error}</ErrorText> : null}
      {saved ? (
        <span className="inline-flex items-center gap-1 text-xs text-emerald-400">
          <Check className="h-3.5 w-3.5" /> Saved
        </span>
      ) : null}
      {dirty ? (
        <Button type="button" variant="ghost" onClick={onReset} disabled={saving}>
          Reset
        </Button>
      ) : null}
      <Button type="submit" disabled={!canManage || saving || !dirty}>
        {saving ? 'Saving…' : 'Save'}
      </Button>
    </>
  );
}

/* ---------- general ---------- */

function GeneralSection({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const { save, saving, saved, error } = useSectionSave(
    workspaceSlug,
    projectSlug,
    service.slug,
  );
  const [name, setName] = useState(service.name);
  useEffect(() => setName(service.name), [service.name]);
  const dirty = name.trim() !== service.name && name.trim().length > 0;

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!dirty) return;
        void save({ name: name.trim() });
      }}
    >
      <SectionCard
        title="General"
        description="Shown in the UI. The slug is immutable."
        footer={
          <SaveRow
            dirty={dirty}
            saving={saving}
            saved={saved}
            error={error}
            onReset={() => setName(service.name)}
            canManage={canManage}
          />
        }
      >
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label="Display name" htmlFor="svc-name">
            <Input
              id="svc-name"
              value={name}
              disabled={!canManage}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label="Slug" htmlFor="svc-slug" hint="Used in URLs and container names.">
            <Input id="svc-slug" value={service.slug} disabled />
          </Field>
        </div>
      </SectionCard>
    </form>
  );
}

/* ---------- env vars ---------- */

interface EnvRow {
  key: string;
  value: string;
  reveal: boolean;
}

function envMapToRows(env: EnvVars): EnvRow[] {
  return Object.entries(env).map(([key, value]) => ({ key, value, reveal: false }));
}

function rowsToEnvMap(rows: EnvRow[]): EnvVars {
  const out: EnvVars = {};
  for (const { key, value } of rows) {
    const k = key.trim();
    if (!k) continue;
    out[k] = value;
  }
  return out;
}

function sameEnv(a: EnvVars, b: EnvVars): boolean {
  const ak = Object.keys(a);
  const bk = Object.keys(b);
  if (ak.length !== bk.length) return false;
  for (const k of ak) if (a[k] !== b[k]) return false;
  return true;
}

function EnvVarsSection({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const { save, saving, saved, error } = useSectionSave(
    workspaceSlug,
    projectSlug,
    service.slug,
  );
  const variableReferences = useVariableReferences(workspaceSlug, projectSlug);
  const [rows, setRows] = useState<EnvRow[]>(() => envMapToRows(service.env_vars));
  const initialRef = useRef(service.env_vars);

  useEffect(() => {
    initialRef.current = service.env_vars;
    setRows(envMapToRows(service.env_vars));
  }, [service.env_vars]);

  const currentMap = useMemo(() => rowsToEnvMap(rows), [rows]);
  const dirty = !sameEnv(currentMap, initialRef.current);
  const dupKey = useMemo(() => findDuplicateKey(rows), [rows]);
  const invalidKey = useMemo(() => rows.find((r) => r.key && !isValidKey(r.key)), [rows]);

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!dirty || dupKey || invalidKey) return;
        void save({ env_vars: currentMap });
      }}
    >
      <SectionCard
        title="Environment variables"
        description="Injected into the container on the next deploy. Existing containers keep their current values until redeployed."
        footer={
          <SaveRow
            dirty={dirty}
            saving={saving}
            saved={saved}
            error={error}
            onReset={() => setRows(envMapToRows(initialRef.current))}
            canManage={canManage}
          />
        }
      >
        {rows.length === 0 ? (
          <p className="text-sm text-[var(--color-muted)]">No variables set yet.</p>
        ) : (
          <Stack gap={2}>
            {rows.map((row, i) => (
              <EnvRowEditor
                key={i}
                row={row}
                disabled={!canManage}
                duplicate={!!row.key && dupKey === row.key}
                invalid={!!row.key && !isValidKey(row.key)}
                references={variableReferences.data}
                currentEnv={currentMap}
                onChange={(patch) =>
                  setRows((prev) => prev.map((r, idx) => (idx === i ? { ...r, ...patch } : r)))
                }
                onRemove={() =>
                  setRows((prev) => prev.filter((_, idx) => idx !== i))
                }
              />
            ))}
          </Stack>
        )}

        {dupKey ? (
          <div className="mt-3">
            <ErrorText>
              Duplicate key <span className="font-mono">{dupKey}</span>
            </ErrorText>
          </div>
        ) : invalidKey ? (
          <div className="mt-3">
            <ErrorText>
              Invalid key <span className="font-mono">{invalidKey.key}</span> — use letters,
              digits, and underscores only.
            </ErrorText>
          </div>
        ) : null}

        {canManage ? (
          <div className="mt-3">
            <Button
              type="button"
              variant="secondary"
              onClick={() =>
                setRows((prev) => [...prev, { key: '', value: '', reveal: true }])
              }
            >
              <Plus className="mr-1.5 h-4 w-4" /> Add variable
            </Button>
          </div>
        ) : null}
      </SectionCard>
    </form>
  );
}

function EnvRowEditor({
  row,
  disabled,
  duplicate,
  invalid,
  references,
  currentEnv,
  onChange,
  onRemove,
}: {
  row: EnvRow;
  disabled: boolean;
  duplicate: boolean;
  invalid: boolean;
  references?: VariableReferencesResponse;
  currentEnv: EnvVars;
  onChange: (patch: Partial<EnvRow>) => void;
  onRemove: () => void;
}) {
  const keyErr = duplicate || invalid;
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)_auto_auto] items-center gap-2">
      <input
        aria-label="Key"
        placeholder="KEY"
        value={row.key}
        disabled={disabled}
        onChange={(e) => onChange({ key: e.target.value })}
        className={[
          'h-9 rounded-md border bg-transparent px-3 font-mono text-xs',
          'placeholder:text-[var(--color-muted)] focus:outline-none',
          keyErr
            ? 'border-red-500/60 focus:border-red-500'
            : 'border-[var(--color-border)] focus:border-[var(--color-accent)]',
        ].join(' ')}
      />
      <EnvReferenceInput
        aria-label="Value"
        placeholder="value"
        type={row.reveal ? 'text' : 'password'}
        value={row.value}
        disabled={disabled}
        onChange={(value) => onChange({ value })}
        references={references}
        currentEnv={currentEnv}
        includeCurrentServiceShorthand
        className={[
          'font-mono text-xs',
        ].join(' ')}
      />
      <button
        type="button"
        onClick={() => onChange({ reveal: !row.reveal })}
        aria-label={row.reveal ? 'Hide value' : 'Show value'}
        title={row.reveal ? 'Hide value' : 'Show value'}
        className="inline-flex h-8 w-8 items-center justify-center rounded-md text-[var(--color-muted)] hover:text-[var(--color-fg)]"
      >
        {row.reveal ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={onRemove}
        aria-label="Remove variable"
        title="Remove"
        className="inline-flex h-8 w-8 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-red-500/10 hover:text-red-400 disabled:hover:bg-transparent disabled:hover:text-[var(--color-muted)]"
      >
        <Trash2 className="h-4 w-4" />
      </button>
    </div>
  );
}

function isValidKey(key: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(key);
}

function findDuplicateKey(rows: EnvRow[]): string | null {
  const seen = new Set<string>();
  for (const r of rows) {
    const k = r.key.trim();
    if (!k) continue;
    if (seen.has(k)) return k;
    seen.add(k);
  }
  return null;
}

/* ---------- image source ---------- */

function ImageSourceSection({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const { save, saving, saved, error } = useSectionSave(
    workspaceSlug,
    projectSlug,
    service.slug,
  );
  const [imageRef, setImageRef] = useState(service.image_ref ?? '');
  useEffect(() => setImageRef(service.image_ref ?? ''), [service.image_ref]);
  const dirty = imageRef.trim() !== (service.image_ref ?? '') && imageRef.trim().length > 0;

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!dirty) return;
        void save({ image_ref: imageRef.trim() });
      }}
    >
      <SectionCard
        title="Image"
        description="The container image to pull on deploy. Include a tag or digest."
        footer={
          <SaveRow
            dirty={dirty}
            saving={saving}
            saved={saved}
            error={error}
            onReset={() => setImageRef(service.image_ref ?? '')}
            canManage={canManage}
          />
        }
      >
        <Field label="Image reference" htmlFor="svc-image">
          <Input
            id="svc-image"
            value={imageRef}
            disabled={!canManage}
            placeholder="ghcr.io/you/app:latest"
            onChange={(e) => setImageRef(e.target.value)}
          />
        </Field>
      </SectionCard>
    </form>
  );
}

/* ---------- git source ---------- */

function GitSourceSection({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const { save, saving, saved, error } = useSectionSave(
    workspaceSlug,
    projectSlug,
    service.slug,
  );
  const [repo, setRepo] = useState(service.git_repo ?? '');
  const [branch, setBranch] = useState(service.git_branch ?? 'main');
  const [builder, setBuilder] = useState<ServiceBuilder>(service.builder);
  const [dockerfile, setDockerfile] = useState(service.dockerfile_path ?? '');
  const [rootDir, setRootDir] = useState(service.root_dir ?? '');

  useEffect(() => {
    setRepo(service.git_repo ?? '');
    setBranch(service.git_branch ?? 'main');
    setBuilder(service.builder);
    setDockerfile(service.dockerfile_path ?? '');
    setRootDir(service.root_dir ?? '');
  }, [service]);

  const dirty =
    repo.trim() !== (service.git_repo ?? '') ||
    branch.trim() !== (service.git_branch ?? '') ||
    builder !== service.builder ||
    (dockerfile.trim() || null) !== (service.dockerfile_path ?? null) ||
    (rootDir.trim() || null) !== (service.root_dir ?? null);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!dirty) return;
    const patch: UpdateServiceInput = {
      git_repo: repo.trim(),
      git_branch: branch.trim() || 'main',
      builder,
    };
    if (builder === 'dockerfile') {
      patch.dockerfile_path = dockerfile.trim();
    }
    patch.root_dir = rootDir.trim();
    void save(patch);
  }

  return (
    <form onSubmit={handleSubmit}>
      <SectionCard
        title="Git source"
        description="Where the builder clones from and how it builds the image."
        footer={
          <SaveRow
            dirty={dirty}
            saving={saving}
            saved={saved}
            error={error}
            onReset={() => {
              setRepo(service.git_repo ?? '');
              setBranch(service.git_branch ?? 'main');
              setBuilder(service.builder);
              setDockerfile(service.dockerfile_path ?? '');
              setRootDir(service.root_dir ?? '');
            }}
            canManage={canManage}
          />
        }
      >
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label="Repository" htmlFor="svc-repo">
            <Input
              id="svc-repo"
              value={repo}
              disabled={!canManage}
              placeholder="https://github.com/you/app"
              onChange={(e) => setRepo(e.target.value)}
            />
          </Field>
          <Field label="Branch" htmlFor="svc-branch">
            <Input
              id="svc-branch"
              value={branch}
              disabled={!canManage}
              placeholder="main"
              onChange={(e) => setBranch(e.target.value)}
            />
          </Field>
          <Field label="Builder" htmlFor="svc-builder">
            <Select
              id="svc-builder"
              value={builder}
              disabled={!canManage}
              onChange={(e) => setBuilder(e.target.value as ServiceBuilder)}
            >
              <option value="dockerfile">Dockerfile</option>
              <option value="railpack">Railpack (auto-detect)</option>
            </Select>
          </Field>
          <Field
            label="Root directory"
            htmlFor="svc-root"
            hint="Subdirectory inside the repo (leave empty for repo root)."
          >
            <Input
              id="svc-root"
              value={rootDir}
              disabled={!canManage}
              placeholder="."
              onChange={(e) => setRootDir(e.target.value)}
            />
          </Field>
          {builder === 'dockerfile' ? (
            <Field
              label="Dockerfile path"
              htmlFor="svc-dockerfile"
              hint="Relative to root directory. Defaults to Dockerfile."
            >
              <Input
                id="svc-dockerfile"
                value={dockerfile}
                disabled={!canManage}
                placeholder="Dockerfile"
                onChange={(e) => setDockerfile(e.target.value)}
              />
            </Field>
          ) : null}
        </div>
      </SectionCard>
    </form>
  );
}

/* ---------- resources ---------- */

function ResourcesSection({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const { save, saving, saved, error } = useSectionSave(
    workspaceSlug,
    projectSlug,
    service.slug,
  );
  const [cpu, setCpu] = useState(String(service.resources.cpu_millis));
  const [mem, setMem] = useState(String(service.resources.memory_mb));
  const [disk, setDisk] = useState(String(service.resources.disk_mb));

  useEffect(() => {
    setCpu(String(service.resources.cpu_millis));
    setMem(String(service.resources.memory_mb));
    setDisk(String(service.resources.disk_mb));
  }, [service.resources]);

  const parsed: Resources | null = (() => {
    const c = Number(cpu);
    const m = Number(mem);
    const d = Number(disk);
    if (![c, m, d].every((n) => Number.isFinite(n) && n > 0)) return null;
    return { cpu_millis: Math.round(c), memory_mb: Math.round(m), disk_mb: Math.round(d) };
  })();

  const dirty =
    !!parsed &&
    (parsed.cpu_millis !== service.resources.cpu_millis ||
      parsed.memory_mb !== service.resources.memory_mb ||
      parsed.disk_mb !== service.resources.disk_mb);

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!dirty || !parsed) return;
        void save({ resources: parsed });
      }}
    >
      <SectionCard
        title="Resources"
        description="Reserved on the node for this service. Applied on redeploy."
        footer={
          <SaveRow
            dirty={dirty}
            saving={saving}
            saved={saved}
            error={error}
            onReset={() => {
              setCpu(String(service.resources.cpu_millis));
              setMem(String(service.resources.memory_mb));
              setDisk(String(service.resources.disk_mb));
            }}
            canManage={canManage}
          />
        }
      >
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          <Field label="CPU (millicores)" htmlFor="svc-cpu" hint="1000 = 1 vCPU.">
            <Input
              id="svc-cpu"
              type="number"
              min={1}
              value={cpu}
              disabled={!canManage}
              onChange={(e) => setCpu(e.target.value)}
            />
          </Field>
          <Field label="Memory (MB)" htmlFor="svc-mem">
            <Input
              id="svc-mem"
              type="number"
              min={1}
              value={mem}
              disabled={!canManage}
              onChange={(e) => setMem(e.target.value)}
            />
          </Field>
          <Field label="Disk (MB)" htmlFor="svc-disk">
            <Input
              id="svc-disk"
              type="number"
              min={1}
              value={disk}
              disabled={!canManage}
              onChange={(e) => setDisk(e.target.value)}
            />
          </Field>
        </div>
      </SectionCard>
    </form>
  );
}

/* ---------- runtime ---------- */

function RuntimeSection({
  service,
  workspaceSlug,
  projectSlug,
  canManage,
}: Props) {
  const { save, saving, saved, error } = useSectionSave(
    workspaceSlug,
    projectSlug,
    service.slug,
  );
  const [replicas, setReplicas] = useState(String(service.replicas));
  const [policy, setPolicy] = useState<RestartPolicy>(service.restart_policy);

  useEffect(() => {
    setReplicas(String(service.replicas));
    setPolicy(service.restart_policy);
  }, [service.replicas, service.restart_policy]);

  const parsedReplicas = (() => {
    const n = Number(replicas);
    return Number.isInteger(n) && n >= 1 ? n : null;
  })();
  const dirty =
    (parsedReplicas !== null && parsedReplicas !== service.replicas) ||
    policy !== service.restart_policy;

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!dirty) return;
        const patch: UpdateServiceInput = {};
        if (parsedReplicas !== null && parsedReplicas !== service.replicas) {
          patch.replicas = parsedReplicas;
        }
        if (policy !== service.restart_policy) patch.restart_policy = policy;
        void save(patch);
      }}
    >
      <SectionCard
        title="Runtime"
        description="How the scheduler and Docker should treat this service."
        footer={
          <SaveRow
            dirty={dirty}
            saving={saving}
            saved={saved}
            error={error}
            onReset={() => {
              setReplicas(String(service.replicas));
              setPolicy(service.restart_policy);
            }}
            canManage={canManage}
          />
        }
      >
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field
            label="Replicas"
            htmlFor="svc-replicas"
            hint="Multi-replica isn't wired end-to-end yet; expect 1 in practice."
          >
            <Input
              id="svc-replicas"
              type="number"
              min={1}
              value={replicas}
              disabled={!canManage}
              onChange={(e) => setReplicas(e.target.value)}
            />
          </Field>
          <Field label="Restart policy" htmlFor="svc-restart">
            <Select
              id="svc-restart"
              value={policy}
              disabled={!canManage}
              onChange={(e) => setPolicy(e.target.value as RestartPolicy)}
            >
              <option value="no">no (never restart)</option>
              <option value="on-failure">on-failure</option>
              <option value="always">always</option>
            </Select>
          </Field>
        </div>
      </SectionCard>
    </form>
  );
}
