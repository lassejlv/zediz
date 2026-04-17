import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router';
import { useMemo, useState, type FormEvent } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, Dices, Loader2 } from 'lucide-react';
import { meQuery } from '@/lib/auth';
import { projectQuery } from '@/lib/projects';
import { canWrite, workspaceQuery } from '@/lib/workspaces';
import {
  useCreateService,
  useDeployService,
  type CreateServiceInput,
} from '@/lib/services';
import { useAttachVolume, useCreateVolume } from '@/lib/volumes';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input, Stack } from '@/components/ui';
import {
  findTemplate,
  generateRandomPassword,
  slugify,
  type Template,
  type TemplateEnv,
} from '@/lib/templates';

export const Route = createFileRoute(
  '/w/$workspaceSlug/projects/$projectSlug/templates/$templateId',
)({
  beforeLoad: async ({ context }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) throw redirect({ to: '/login' });
  },
  component: TemplateDeployPage,
});

type Phase = 'idle' | 'creating' | 'volume' | 'attach' | 'deploying';

const PHASE_LABEL: Record<Exclude<Phase, 'idle'>, string> = {
  creating: 'Creating service…',
  volume: 'Creating volume…',
  attach: 'Attaching volume…',
  deploying: 'Kicking off first deploy…',
};

function TemplateDeployPage() {
  const { workspaceSlug, projectSlug, templateId } = Route.useParams();
  const template = findTemplate(templateId);
  const navigate = useNavigate();

  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const project = useQuery(projectQuery(workspaceSlug, projectSlug));
  const createService = useCreateService(workspaceSlug, projectSlug);
  const createVolume = useCreateVolume(workspaceSlug);

  if (!template) {
    return (
      <div className="mx-auto max-w-xl py-12 text-center text-sm text-[var(--color-muted)]">
        That template doesn't exist.{' '}
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug/templates"
          params={{ workspaceSlug, projectSlug }}
          className="text-[var(--color-fg)] underline-offset-4 hover:underline"
        >
          Back to templates
        </Link>
      </div>
    );
  }

  return (
    <TemplateDeployForm
      template={template}
      workspaceSlug={workspaceSlug}
      projectSlug={projectSlug}
      projectName={project.data?.name ?? projectSlug}
      canDeploy={canWrite(workspace.data)}
      useAttachVolumeFactory={useAttachVolume}
      useDeployServiceFactory={useDeployService}
      createService={createService}
      createVolume={createVolume}
      onDone={(serviceSlug) =>
        navigate({
          to: '/w/$workspaceSlug/projects/$projectSlug/$serviceSlug',
          params: { workspaceSlug, projectSlug, serviceSlug },
        })
      }
    />
  );
}

interface FormProps {
  template: Template;
  workspaceSlug: string;
  projectSlug: string;
  projectName: string;
  canDeploy: boolean;
  createService: ReturnType<typeof useCreateService>;
  createVolume: ReturnType<typeof useCreateVolume>;
  useAttachVolumeFactory: typeof useAttachVolume;
  useDeployServiceFactory: typeof useDeployService;
  onDone: (serviceSlug: string) => void;
}

function TemplateDeployForm({
  template,
  workspaceSlug,
  projectSlug,
  projectName,
  canDeploy,
  createService,
  createVolume,
  useAttachVolumeFactory,
  useDeployServiceFactory,
  onDone,
}: FormProps) {
  const [name, setName] = useState(template.defaultName);
  const [slug, setSlug] = useState(template.defaultSlug);
  const [slugTouched, setSlugTouched] = useState(false);
  const [envValues, setEnvValues] = useState<Record<string, string>>(() =>
    initialEnvValues(template),
  );
  const [volumeSizeGb, setVolumeSizeGb] = useState(
    template.volume?.defaultSizeGb ?? 10,
  );

  // Attach + deploy hooks depend on the service slug — but the slug is
  // what the user types. We need the hook to be called unconditionally
  // (rules of hooks), so set it up with the current slug and only use
  // it after the service is created with that slug.
  const attach = useAttachVolumeFactory(workspaceSlug, projectSlug, slug);
  const deploy = useDeployServiceFactory(workspaceSlug, projectSlug, slug);

  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState<string | null>(null);
  const [partialHint, setPartialHint] = useState<string | null>(null);

  const hasVolume = !!template.volume;
  const Icon = template.icon;

  function onNameChange(next: string) {
    setName(next);
    if (!slugTouched) setSlug(slugify(next));
  }

  function updateEnv(key: string, value: string) {
    setEnvValues((prev) => ({ ...prev, [key]: value }));
  }

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    setPartialHint(null);

    const trimmedSlug = slug.trim();
    const trimmedName = name.trim();
    if (!trimmedSlug || !trimmedName) {
      setError('Name and slug are required.');
      return;
    }

    // Required env vars.
    for (const env of template.envVars) {
      if (env.required && !envValues[env.key]?.trim()) {
        setError(`${env.key} is required.`);
        return;
      }
    }

    const env_vars: Record<string, string> = {};
    for (const env of template.envVars) {
      const value = envValues[env.key];
      if (value === undefined || value === '') continue;
      env_vars[env.key] = value;
    }

    const payload: CreateServiceInput = {
      slug: trimmedSlug,
      name: trimmedName,
      source: 'image',
      image_ref: template.image,
      env_vars,
      ports: template.ports,
      resources: template.resources,
      restart_policy: 'on-failure',
    };

    try {
      setPhase('creating');
      const service = await createService.mutateAsync(payload);

      if (template.volume) {
        setPhase('volume');
        const volume = await createVolume
          .mutateAsync({
            name: `${service.slug}-${randomSuffix()}`,
            size_gb: volumeSizeGb,
          })
          .catch((err) => {
            setPartialHint(
              'The service was created, but the volume step failed. You can add storage from the Volume tab.',
            );
            throw err;
          });

        setPhase('attach');
        await attach
          .mutateAsync({
            volume_id: volume.id,
            mount_path: template.volume.mountPath,
          })
          .catch((err) => {
            setPartialHint(
              'Service + volume exist, but attaching failed. You can attach it from the Volume tab.',
            );
            throw err;
          });
      }

      setPhase('deploying');
      await deploy.mutateAsync().catch((err) => {
        setPartialHint(
          'Everything is configured, but the first deploy didn\'t start. Hit Redeploy on the service page.',
        );
        throw err;
      });

      onDone(service.slug);
    } catch (err) {
      setPhase('idle');
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : 'Something went wrong',
      );
    }
  }

  if (!canDeploy) {
    return (
      <div className="mx-auto max-w-xl py-12 text-center text-sm text-[var(--color-muted)]">
        You need write access to deploy templates in this project.
      </div>
    );
  }

  const running = phase !== 'idle';

  return (
    <form
      onSubmit={onSubmit}
      className="mx-auto flex max-w-3xl flex-col gap-6 pb-40"
    >
      <div>
        <Link
          to="/w/$workspaceSlug/projects/$projectSlug/templates"
          params={{ workspaceSlug, projectSlug }}
          className="inline-flex items-center gap-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <ArrowLeft className="h-3 w-3" /> Templates
        </Link>
        <div className="mt-2 flex items-center gap-3">
          <span className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-[var(--color-border)] bg-black/[0.02] dark:bg-white/[0.02]">
            <Icon className="h-5 w-5" />
          </span>
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">{template.name}</h1>
            <p className="text-sm text-[var(--color-muted)]">{template.description}</p>
          </div>
        </div>
        <p className="mt-2 font-mono text-xs text-[var(--color-muted)]">
          {template.image} · deploying into {projectName}
        </p>
      </div>

      <Section
        title="Service"
        description="How this shows up in the project. The slug is also the hostname other services use to reach it."
      >
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <Field label="Display name" htmlFor="tpl-name">
            <Input
              id="tpl-name"
              required
              value={name}
              onChange={(e) => onNameChange(e.target.value)}
              disabled={running}
            />
          </Field>
          <Field
            label="Slug"
            htmlFor="tpl-slug"
            hint="Used in URLs and as the hostname for other services."
          >
            <Input
              id="tpl-slug"
              required
              value={slug}
              onChange={(e) => {
                setSlug(e.target.value);
                setSlugTouched(true);
              }}
              disabled={running}
            />
          </Field>
        </div>
      </Section>

      {template.envVars.length > 0 ? (
        <Section
          title="Configuration"
          description="Environment variables passed to the container."
        >
          <Stack gap={4}>
            {template.envVars.map((env) => (
              <EnvInput
                key={env.key}
                env={env}
                value={envValues[env.key] ?? ''}
                onChange={(v) => updateEnv(env.key, v)}
                disabled={running}
              />
            ))}
          </Stack>
        </Section>
      ) : null}

      {template.volume ? (
        <Section
          title="Storage"
          description={`A Hetzner block volume mounted at ${template.volume.mountPath}.`}
        >
          <Field
            label="Size"
            htmlFor="tpl-size"
            hint={`${template.volume.defaultSizeGb} GB is plenty for most cases. Max 10240.`}
          >
            <div className="space-y-2">
              <input
                id="tpl-size-range"
                type="range"
                min={10}
                max={500}
                step={10}
                value={Math.min(Math.max(volumeSizeGb, 10), 500)}
                onChange={(e) => setVolumeSizeGb(Number(e.target.value))}
                disabled={running}
                className="w-full accent-[var(--color-accent)]"
              />
              <div className="flex items-center gap-2">
                <Input
                  id="tpl-size"
                  type="number"
                  min={10}
                  max={10240}
                  step={1}
                  value={volumeSizeGb}
                  onChange={(e) => setVolumeSizeGb(Number(e.target.value) || 0)}
                  disabled={running}
                  className="w-28"
                />
                <span className="text-sm text-[var(--color-muted)]">GB</span>
              </div>
            </div>
          </Field>
        </Section>
      ) : null}

      {template.notes ? (
        <Card className="border-dashed p-4 text-xs leading-relaxed text-[var(--color-muted)]">
          {template.notes}
        </Card>
      ) : null}

      {error ? <ErrorText>{error}</ErrorText> : null}
      {partialHint ? (
        <p className="text-xs text-amber-400">{partialHint}</p>
      ) : null}

      <div className="fixed inset-x-0 bottom-0 z-10 border-t border-[var(--color-border)] bg-[var(--color-bg)]/90 backdrop-blur">
        <div className="mx-auto flex max-w-3xl items-center justify-between gap-3 px-6 py-3">
          <span className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
            {running ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {PHASE_LABEL[phase as Exclude<Phase, 'idle'>]}
              </>
            ) : hasVolume ? (
              'Creates the service, volume, attaches it, and starts a deploy.'
            ) : (
              'Creates the service and starts a deploy.'
            )}
          </span>
          <div className="flex gap-2">
            <Link
              to="/w/$workspaceSlug/projects/$projectSlug/templates"
              params={{ workspaceSlug, projectSlug }}
            >
              <Button type="button" variant="secondary" disabled={running}>
                Cancel
              </Button>
            </Link>
            <Button type="submit" disabled={running}>
              {running ? 'Deploying…' : 'Deploy'}
            </Button>
          </div>
        </div>
      </div>
    </form>
  );
}

/* ---------- small pieces ---------- */

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
      {children}
    </Card>
  );
}

function EnvInput({
  env,
  value,
  onChange,
  disabled,
}: {
  env: TemplateEnv;
  value: string;
  onChange: (v: string) => void;
  disabled: boolean;
}) {
  const reveal = useMemo(() => env.generate !== 'password', [env.generate]);
  const [shown, setShown] = useState(reveal);
  return (
    <Field
      label={
        <span className="flex items-center gap-2">
          <span className="font-mono text-xs">{env.key}</span>
          {env.required ? (
            <span className="text-[10px] font-medium uppercase tracking-wider text-amber-400">
              required
            </span>
          ) : null}
          {env.readonly ? (
            <span className="text-[10px] font-medium uppercase tracking-wider text-[var(--color-subtle)]">
              locked
            </span>
          ) : null}
        </span>
      }
      htmlFor={`env-${env.key}`}
      hint={env.description}
    >
      <div className="flex items-center gap-2">
        <Input
          id={`env-${env.key}`}
          type={shown ? 'text' : 'password'}
          value={value}
          readOnly={env.readonly}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          className="font-mono text-xs"
        />
        {env.generate === 'password' ? (
          <>
            <Button
              type="button"
              variant="secondary"
              onClick={() => {
                onChange(generateRandomPassword(24));
                setShown(true);
              }}
              disabled={disabled}
              title="Generate a strong random password"
            >
              <Dices className="h-4 w-4" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={() => setShown((v) => !v)}
              disabled={disabled}
            >
              {shown ? 'Hide' : 'Show'}
            </Button>
          </>
        ) : null}
      </div>
    </Field>
  );
}

/* ---------- helpers ---------- */

function initialEnvValues(template: Template): Record<string, string> {
  const out: Record<string, string> = {};
  for (const env of template.envVars) {
    if (env.defaultValue !== undefined) out[env.key] = env.defaultValue;
    else if (env.generate === 'password') out[env.key] = generateRandomPassword(24);
    else out[env.key] = '';
  }
  return out;
}

function randomSuffix(): string {
  return Math.random().toString(36).slice(2, 6);
}
