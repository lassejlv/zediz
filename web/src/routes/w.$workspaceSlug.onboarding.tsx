import { createFileRoute, Link, useNavigate } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useState, type FormEvent } from 'react';
import {
  ArrowRight,
  Check,
  ExternalLink,
  Github,
  KeyRound,
  Package,
  Server,
  Sparkles,
} from 'lucide-react';
import {
  credentialsQuery,
  useCreateCredential,
} from '@/lib/credentials';
import { canAdmin, workspaceQuery } from '@/lib/workspaces';
import { usePublicSettings } from '@/lib/settings';
import { ApiError } from '@/lib/api';
import {
  Button,
  Card,
  ErrorText,
  Field,
  Input,
  PageHeader,
  Stack,
} from '@/components/ui';

export const Route = createFileRoute('/w/$workspaceSlug/onboarding')({
  component: OnboardingPage,
});

const HETZNER_TOKEN_URL = 'https://console.hetzner.com/';
const GITHUB_PAT_URL =
  'https://github.com/settings/tokens/new?scopes=repo,read:user&description=Driftbase';

type Step = 'hetzner' | 'github' | 'registry' | 'done';

function OnboardingPage() {
  const { workspaceSlug } = Route.useParams();
  const navigate = useNavigate();
  const workspace = useQuery(workspaceQuery(workspaceSlug));
  const creds = useQuery(credentialsQuery(workspaceSlug));

  const canManage = canAdmin(workspace.data);

  const hasHetzner = useMemo(
    () => !!creds.data?.some((c) => c.kind === 'hetzner_api_token'),
    [creds.data],
  );
  const hasGithub = useMemo(
    () => !!creds.data?.some((c) => c.kind === 'github_pat'),
    [creds.data],
  );
  const hasRegistry = useMemo(
    () => !!creds.data?.some((c) => c.kind === 'registry'),
    [creds.data],
  );

  const initialStep: Step = !hasHetzner
    ? 'hetzner'
    : !hasGithub
      ? 'github'
      : !hasRegistry
        ? 'registry'
        : 'done';
  const [step, setStep] = useState<Step>(initialStep);
  const [skippedGithub, setSkippedGithub] = useState(false);
  const [skippedRegistry, setSkippedRegistry] = useState(false);

  // If creds load after first render and the user is already past a step,
  // bump them forward.
  if (step === 'hetzner' && hasHetzner) setStep('github');
  if (step === 'github' && hasGithub) setStep('registry');
  if (step === 'registry' && hasRegistry) setStep('done');

  if (!canManage) {
    return (
      <Stack gap={6}>
        <PageHeader title="Get started" />
        <Card className="p-5">
          <p className="text-sm text-[var(--color-muted)]">
            Only owners or admins can finish workspace setup.
          </p>
        </Card>
      </Stack>
    );
  }

  return (
    <Stack gap={6}>
      <PageHeader
        title={`Set up ${workspace.data?.name ?? workspaceSlug}`}
        subtitle="Three quick credentials and you're ready to deploy."
      />

      <Steps
        current={step}
        hasHetzner={hasHetzner}
        hasGithub={hasGithub}
        hasRegistry={hasRegistry}
      />

      {step === 'hetzner' ? (
        <HetznerStep
          workspaceSlug={workspaceSlug}
          onDone={() => setStep('github')}
        />
      ) : null}

      {step === 'github' ? (
        <GithubStep
          workspaceSlug={workspaceSlug}
          alreadyHave={hasGithub}
          onDone={() => setStep('registry')}
          onSkip={() => {
            setSkippedGithub(true);
            setStep('registry');
          }}
        />
      ) : null}

      {step === 'registry' ? (
        <RegistryStep
          workspaceSlug={workspaceSlug}
          alreadyHave={hasRegistry}
          onDone={() => setStep('done')}
          onSkip={() => {
            setSkippedRegistry(true);
            setStep('done');
          }}
        />
      ) : null}

      {step === 'done' ? (
        <DoneStep
          workspaceSlug={workspaceSlug}
          skippedGithub={skippedGithub && !hasGithub}
          skippedRegistry={skippedRegistry && !hasRegistry}
          onContinue={() =>
            navigate({
              to: '/w/$workspaceSlug',
              params: { workspaceSlug },
            })
          }
        />
      ) : null}
    </Stack>
  );
}

/* ---------------- step indicator ---------------- */

function Steps({
  current,
  hasHetzner,
  hasGithub,
  hasRegistry,
}: {
  current: Step;
  hasHetzner: boolean;
  hasGithub: boolean;
  hasRegistry: boolean;
}) {
  return (
    <ol className="flex items-center gap-3 text-xs">
      <StepBadge
        index={1}
        label="Hetzner"
        state={
          hasHetzner ? 'done' : current === 'hetzner' ? 'active' : 'todo'
        }
      />
      <Connector />
      <StepBadge
        index={2}
        label="GitHub"
        state={hasGithub ? 'done' : current === 'github' ? 'active' : 'todo'}
      />
      <Connector />
      <StepBadge
        index={3}
        label="Registry"
        state={
          hasRegistry ? 'done' : current === 'registry' ? 'active' : 'todo'
        }
      />
      <Connector />
      <StepBadge
        index={4}
        label="Done"
        state={current === 'done' ? 'active' : 'todo'}
      />
    </ol>
  );
}

function Connector() {
  return <span className="h-px w-8 bg-[var(--color-border)]" />;
}

function StepBadge({
  index,
  label,
  state,
}: {
  index: number;
  label: string;
  state: 'todo' | 'active' | 'done';
}) {
  const dotClass =
    state === 'done'
      ? 'border-[var(--color-accent)] bg-[var(--color-accent)] text-black'
      : state === 'active'
        ? 'border-[var(--color-accent)] text-[var(--color-fg)]'
        : 'border-[var(--color-border)] text-[var(--color-muted)]';
  return (
    <li className="flex items-center gap-2">
      <span
        className={[
          'inline-flex h-5 w-5 items-center justify-center rounded-full border font-mono text-[10px]',
          dotClass,
        ].join(' ')}
      >
        {state === 'done' ? <Check className="h-3 w-3" /> : index}
      </span>
      <span
        className={
          state === 'todo'
            ? 'text-[var(--color-muted)]'
            : 'text-[var(--color-fg)]'
        }
      >
        {label}
      </span>
    </li>
  );
}

/* ---------------- step 1: hetzner ---------------- */

function HetznerStep({
  workspaceSlug,
  onDone,
}: {
  workspaceSlug: string;
  onDone: () => void;
}) {
  const create = useCreateCredential(workspaceSlug);
  const [name, setName] = useState('production');
  const [secret, setSecret] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await create.mutateAsync({
        kind: 'hetzner_api_token',
        name: name.trim() || 'production',
        secret: secret.trim(),
      });
      onDone();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Card className="p-5">
      <div className="mb-4 flex items-start gap-3">
        <span className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--color-border)]">
          <Server className="h-4 w-4" />
        </span>
        <div>
          <h2 className="text-base font-medium">Add a Hetzner API token</h2>
          <p className="mt-1 text-sm text-[var(--color-muted)]">
            Driftbase provisions and tears down nodes in your Hetzner Cloud
            project. Create a token with{' '}
            <span className="font-mono text-[var(--color-fg)]">
              Read &amp; Write
            </span>{' '}
            permissions.
          </p>
          <a
            href={HETZNER_TOKEN_URL}
            target="_blank"
            rel="noreferrer"
            className="mt-2 inline-flex items-center gap-1.5 text-xs text-[var(--color-accent)] hover:underline"
          >
            Open Hetzner Cloud Console
            <ExternalLink className="h-3 w-3" />
          </a>
        </div>
      </div>

      <form onSubmit={onSubmit} className="space-y-4">
        <Field
          label="Name"
          htmlFor="hetzner-name"
          hint="A label for this token. You can rotate it later."
        >
          <Input
            id="hetzner-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field label="API token" htmlFor="hetzner-secret">
          <Input
            id="hetzner-secret"
            type="password"
            required
            autoComplete="off"
            placeholder="64-character token from Hetzner"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
          />
        </Field>
        {error ? <ErrorText>{error}</ErrorText> : null}
        <div className="flex items-center justify-between pt-2">
          <Link
            to="/w/$workspaceSlug"
            params={{ workspaceSlug }}
            className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            I'll do this later
          </Link>
          <Button type="submit" disabled={create.isPending || !secret.trim()}>
            {create.isPending ? 'Saving…' : 'Save and continue'}
            <ArrowRight className="ml-1.5 h-3.5 w-3.5" />
          </Button>
        </div>
      </form>
    </Card>
  );
}

/* ---------------- step 2: github ---------------- */

function GithubStep({
  workspaceSlug,
  alreadyHave,
  onDone,
  onSkip,
}: {
  workspaceSlug: string;
  alreadyHave: boolean;
  onDone: () => void;
  onSkip: () => void;
}) {
  const create = useCreateCredential(workspaceSlug);
  const [name, setName] = useState('github');
  const [secret, setSecret] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      await create.mutateAsync({
        kind: 'github_pat',
        name: name.trim() || 'github',
        secret: secret.trim(),
      });
      onDone();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Card className="p-5">
      <div className="mb-4 flex items-start gap-3">
        <span className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--color-border)]">
          <Github className="h-4 w-4" />
        </span>
        <div>
          <h2 className="text-base font-medium">Add a GitHub PAT</h2>
          <p className="mt-1 text-sm text-[var(--color-muted)]">
            Used to clone repositories during builds — including private ones.
            We need the{' '}
            <span className="font-mono text-[var(--color-fg)]">repo</span> and{' '}
            <span className="font-mono text-[var(--color-fg)]">read:user</span>{' '}
            scopes.
          </p>
          <a
            href={GITHUB_PAT_URL}
            target="_blank"
            rel="noreferrer"
            className="mt-2 inline-flex items-center gap-1.5 text-xs text-[var(--color-accent)] hover:underline"
          >
            Create token on GitHub (scopes pre-selected)
            <ExternalLink className="h-3 w-3" />
          </a>
          {alreadyHave ? (
            <p className="mt-2 text-xs text-[var(--color-muted)]">
              You already have a GitHub PAT on this workspace. You can add
              another or skip.
            </p>
          ) : null}
        </div>
      </div>

      <form onSubmit={onSubmit} className="space-y-4">
        <Field label="Name" htmlFor="github-name">
          <Input
            id="github-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field label="Personal access token" htmlFor="github-secret">
          <Input
            id="github-secret"
            type="password"
            required
            autoComplete="off"
            placeholder="ghp_… or github_pat_…"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
          />
        </Field>
        {error ? <ErrorText>{error}</ErrorText> : null}
        <div className="flex items-center justify-between pt-2">
          <button
            type="button"
            onClick={onSkip}
            className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            Skip for now
          </button>
          <Button type="submit" disabled={create.isPending || !secret.trim()}>
            {create.isPending ? 'Saving…' : 'Save and continue'}
            <ArrowRight className="ml-1.5 h-3.5 w-3.5" />
          </Button>
        </div>
      </form>
    </Card>
  );
}

/* ---------------- step 3: registry ---------------- */

function RegistryStep({
  workspaceSlug,
  alreadyHave,
  onDone,
  onSkip,
}: {
  workspaceSlug: string;
  alreadyHave: boolean;
  onDone: () => void;
  onSkip: () => void;
}) {
  const create = useCreateCredential(workspaceSlug);
  const settings = usePublicSettings();
  const bundledHost = settings.data?.registry_site ?? null;

  const [name, setName] = useState(bundledHost ? 'bundled' : 'ghcr');
  const [registryUrl, setRegistryUrl] = useState('');
  const [registryUsername, setRegistryUsername] = useState('');
  const [secret, setSecret] = useState('');
  const [error, setError] = useState<string | null>(null);

  const effectiveUrl = registryUrl.trim() || bundledHost || '';
  const urlIsBundled =
    bundledHost !== null &&
    hostFromUrl(effectiveUrl) === bundledHost.toLowerCase();

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      if (!effectiveUrl) {
        setError('Registry URL is required');
        return;
      }
      if (!urlIsBundled && !registryUsername.trim()) {
        setError('Username is required for external registries');
        return;
      }
      await create.mutateAsync({
        kind: 'registry',
        name: name.trim() || 'registry',
        secret: secret.trim(),
        metadata: {
          url: effectiveUrl,
          ...(urlIsBundled ? {} : { username: registryUsername.trim() }),
        },
      });
      onDone();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Something went wrong');
    }
  }

  return (
    <Card className="p-5">
      <div className="mb-4 flex items-start gap-3">
        <span className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--color-border)]">
          <Package className="h-4 w-4" />
        </span>
        <div>
          <h2 className="text-base font-medium">Connect a container registry</h2>
          <p className="mt-1 text-sm text-[var(--color-muted)]">
            {bundledHost
              ? `Defaults to the bundled registry (${bundledHost}). Change the URL to use GHCR, Docker Hub, or self-hosted.`
              : 'Where built images are pushed and runtime nodes pull from. Use GHCR, Docker Hub, or any OCI registry.'}
          </p>
          {alreadyHave ? (
            <p className="mt-2 text-xs text-[var(--color-muted)]">
              You already have a registry credential. You can add another or
              skip.
            </p>
          ) : null}
        </div>
      </div>

      <form onSubmit={onSubmit} className="space-y-4">
        <Field label="Name" htmlFor="registry-name">
          <Input
            id="registry-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field
          label="Registry URL"
          htmlFor="registry-url"
          hint={
            bundledHost
              ? `Leave blank to use ${bundledHost}.`
              : 'e.g. ghcr.io or docker.io'
          }
        >
          <Input
            id="registry-url"
            placeholder={bundledHost ?? 'ghcr.io'}
            value={registryUrl}
            onChange={(e) => setRegistryUrl(e.target.value)}
          />
        </Field>
        {urlIsBundled ? (
          <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-3 py-2 text-[11px] text-[var(--color-muted)]">
            Username will be auto-generated (the credential's ULID) so the
            built-in auth proxy can scope access to this workspace.
          </div>
        ) : (
          <Field
            label="Username"
            htmlFor="registry-username"
            hint="Registry login (e.g. your GitHub username for GHCR)."
          >
            <Input
              id="registry-username"
              required
              value={registryUsername}
              onChange={(e) => setRegistryUsername(e.target.value)}
            />
          </Field>
        )}
        <Field label="Password" htmlFor="registry-secret">
          <Input
            id="registry-secret"
            type="password"
            required
            autoComplete="off"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
          />
        </Field>
        {error ? <ErrorText>{error}</ErrorText> : null}
        <div className="flex items-center justify-between pt-2">
          <button
            type="button"
            onClick={onSkip}
            className="text-xs text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            Skip for now
          </button>
          <Button type="submit" disabled={create.isPending || !secret.trim()}>
            {create.isPending ? 'Saving…' : 'Save and finish'}
            <ArrowRight className="ml-1.5 h-3.5 w-3.5" />
          </Button>
        </div>
      </form>
    </Card>
  );
}

function hostFromUrl(url: string): string {
  return url
    .trim()
    .replace(/^https?:\/\//, '')
    .split('/')[0]
    .toLowerCase();
}

/* ---------------- step 4: done ---------------- */

function DoneStep({
  workspaceSlug,
  skippedGithub,
  skippedRegistry,
  onContinue,
}: {
  workspaceSlug: string;
  skippedGithub: boolean;
  skippedRegistry: boolean;
  onContinue: () => void;
}) {
  const skipped: string[] = [];
  if (skippedGithub) skipped.push('GitHub PAT');
  if (skippedRegistry) skipped.push('registry credential');
  return (
    <Card className="p-6">
      <div className="flex items-start gap-3">
        <span className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 text-[var(--color-accent)]">
          <Sparkles className="h-4 w-4" />
        </span>
        <div className="flex-1">
          <h2 className="text-base font-medium">You're all set</h2>
          <p className="mt-1 text-sm text-[var(--color-muted)]">
            Driftbase can now provision Hetzner nodes
            {skippedGithub ? '' : ', clone your GitHub repos'}
            {skippedRegistry ? '' : ', and pull from your registry'}. Create
            your first project to deploy a service.
          </p>
          {skipped.length > 0 ? (
            <p className="mt-3 text-xs text-[var(--color-muted)]">
              You can add a {skipped.join(' and ')} any time from{' '}
              <Link
                to="/w/$workspaceSlug/credentials"
                params={{ workspaceSlug }}
                className="text-[var(--color-fg)] underline-offset-2 hover:underline"
              >
                Credentials
              </Link>
              .
            </p>
          ) : null}
          <div className="mt-5 flex items-center gap-2">
            <Button onClick={onContinue}>
              <KeyRound className="mr-1.5 h-3.5 w-3.5" />
              Go to workspace
            </Button>
            <Link
              to="/w/$workspaceSlug/credentials"
              params={{ workspaceSlug }}
            >
              <Button variant="secondary">Manage credentials</Button>
            </Link>
          </div>
        </div>
      </div>
    </Card>
  );
}
