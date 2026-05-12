import { useQuery } from '@tanstack/react-query';
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from 'react';
import {
  Check,
  Copy,
  ExternalLink,
  Info,
  Plus,
  RefreshCw,
  Trash2,
  X,
} from 'lucide-react';
import {
  useAddDomain,
  useDeleteDomain,
  useRetryDomain,
  useUpdateDomain,
  domainsQuery,
} from '@/lib/domains';
import { ApiError } from '@/lib/api';
import {
  Button,
  Card,
  EmptyState,
  ErrorText,
  Field,
  Input,
  RelativeTime,
  Stack,
  StatusPill,
  type SemanticStatus,
} from '@/components/ui';
import type { DomainSummary, TlsStatus } from '@/lib/types';

const TLS_META: Record<
  TlsStatus,
  { status: SemanticStatus; label: string; pulse: boolean }
> = {
  pending: { status: 'warn', label: 'pending', pulse: true },
  active: { status: 'ok', label: 'active', pulse: false },
  failed: { status: 'error', label: 'failed', pulse: false },
};

interface Props {
  workspaceSlug: string;
  projectSlug: string;
  serviceSlug: string;
  canManage: boolean;
  defaultPort?: number | null;
}

export function DomainsSection({
  workspaceSlug,
  projectSlug,
  serviceSlug,
  canManage,
  defaultPort,
}: Props) {
  const domains = useQuery(domainsQuery(workspaceSlug, projectSlug, serviceSlug));
  const add = useAddDomain(workspaceSlug, projectSlug, serviceSlug);
  const del = useDeleteDomain(workspaceSlug, projectSlug, serviceSlug);
  const updateDomain = useUpdateDomain(workspaceSlug, projectSlug, serviceSlug);
  const retry = useRetryDomain(workspaceSlug, projectSlug, serviceSlug);

  const list = domains.data ?? [];
  const edgeIp =
    list.flatMap((domain) => domain.edge_ips).find((ip) => ip.length > 0) ?? null;
  const [addOpen, setAddOpen] = useState(false);
  const showAddCard = canManage && (addOpen || list.length === 0);

  return (
    <Stack gap={4}>
      <div className="flex items-end justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium">Domains</h2>
          <p className="mt-0.5 text-xs text-[var(--color-muted)]">
            {list.length === 0
              ? 'No hostnames routed to this service yet.'
              : `${list.length} hostname${list.length === 1 ? '' : 's'} routed to this service.`}
          </p>
        </div>
        {canManage && list.length > 0 ? (
          <Button
            variant={addOpen ? 'ghost' : 'secondary'}
            onClick={() => setAddOpen((v) => !v)}
          >
            {addOpen ? (
              <>
                <X className="mr-1.5 h-4 w-4" /> Cancel
              </>
            ) : (
              <>
                <Plus className="mr-1.5 h-4 w-4" /> Add domain
              </>
            )}
          </Button>
        ) : null}
      </div>

      {showAddCard ? (
        <AddDomainCard
          defaultPort={defaultPort}
          edgeIp={edgeIp}
          onSubmit={async (body) => {
            await add.mutateAsync(body);
            setAddOpen(false);
          }}
          pending={add.isPending}
        />
      ) : null}

      {list.length === 0 ? (
        !showAddCard ? (
          <EmptyState
            title="No domains yet"
            body="Route a custom hostname to this service and Caddy will issue TLS on first HTTPS request."
          />
        ) : null
      ) : (
        <Stack gap={2}>
          {list.map((d) => (
            <DomainRow
              key={d.id}
              domain={d}
              canManage={canManage}
              edgeIp={d.edge_ips[0] ?? edgeIp}
              onUpdatePort={(port) =>
                updateDomain.mutateAsync({ id: d.id, container_port: port })
              }
              onRetry={() => retry.mutate(d.id)}
              retrying={retry.isPending && retry.variables === d.id}
              onDelete={() => del.mutate(d.id)}
              deleting={del.isPending && del.variables === d.id}
            />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

/* ---------- add form ---------- */

function AddDomainCard({
  defaultPort,
  edgeIp,
  pending,
  onSubmit,
}: {
  defaultPort?: number | null;
  edgeIp: string | null;
  pending: boolean;
  onSubmit: (body: { hostname: string; container_port?: number }) => Promise<void>;
}) {
  const [hostname, setHostname] = useState('');
  const [port, setPort] = useState('');
  const [error, setError] = useState<string | null>(null);

  const normalized = hostname.trim().toLowerCase();

  async function handle(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const body: { hostname: string; container_port?: number } = {
        hostname: normalized,
      };
      if (port.trim()) {
        const n = Number(port);
        if (!Number.isFinite(n)) throw new Error('invalid port');
        body.container_port = n;
      }
      await onSubmit(body);
      setHostname('');
      setPort('');
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : 'Failed',
      );
    }
  }

  return (
    <Card className="p-5">
      <form
        onSubmit={handle}
        className="grid grid-cols-[1fr_140px_auto] items-end gap-3"
      >
        <Field label="Hostname" htmlFor="dom-host">
          <Input
            id="dom-host"
            required
            autoFocus
            placeholder="api.example.com"
            value={hostname}
            onChange={(e) => setHostname(e.target.value)}
          />
        </Field>
        <Field
          label="Port"
          htmlFor="dom-port"
          hint={defaultPort ? `default: ${defaultPort}` : '80, 3000, …'}
        >
          <Input
            id="dom-port"
            type="number"
            min={1}
            max={65535}
            placeholder={String(defaultPort ?? 80)}
            value={port}
            onChange={(e) => setPort(e.target.value)}
          />
        </Field>
        <Button type="submit" disabled={pending}>
          {pending ? 'Adding…' : 'Add domain'}
        </Button>
      </form>

      {error ? (
        <div className="mt-3">
          <ErrorText>{error}</ErrorText>
        </div>
      ) : null}

      <DnsRecordPreview hostname={normalized} edgeIp={edgeIp} />
    </Card>
  );
}

function DnsRecordPreview({
  hostname,
  edgeIp,
}: {
  hostname: string;
  edgeIp: string | null;
}) {
  const recordName = dnsRecordName(hostname);
  const ip = edgeIp ?? '—';
  const cloudflare = useCloudflareDetection(hostname);

  return (
    <div className="mt-5 rounded-md border border-[var(--color-border)] bg-black/[0.02] p-3 dark:bg-white/[0.02]">
      <div className="mb-2 flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
        <Info className="h-3 w-3" />
        DNS record
      </div>
      <div className="grid grid-cols-[auto_auto_auto_auto_1fr_auto] items-center gap-x-3 gap-y-1 font-mono text-xs">
        <DnsCell label="type" value="A" />
        <DnsCell label="name" value={recordName || '—'} copyable={recordName} />
        <DnsCell label="value" value={ip} copyable={edgeIp ?? ''} />
        <DnsCell label="ttl" value="auto" />
      </div>
      <p className="mt-2 text-xs text-[var(--color-muted)]">
        {edgeIp
          ? 'Caddy issues TLS at the edge on the first HTTPS request. Propagation usually takes a minute.'
          : 'Deploy the service first — an edge IP is needed before DNS can point anywhere.'}
      </p>
      {cloudflare.detected ? (
        <CloudflareConnect
          apex={cloudflare.apex}
          recordName={recordName}
          nodeIp={edgeIp}
        />
      ) : null}
    </div>
  );
}

function DnsCell({
  label,
  value,
  copyable,
}: {
  label: string;
  value: string;
  copyable?: string;
}) {
  return (
    <>
      <span className="text-[var(--color-muted)]">{label}</span>
      <span className="col-span-4 flex items-center gap-2">
        <span>{value}</span>
        {copyable ? <CopyBtn value={copyable} /> : null}
      </span>
    </>
  );
}

/* ---------- domain row ---------- */

function DomainRow({
  domain,
  canManage,
  edgeIp,
  onUpdatePort,
  onRetry,
  retrying,
  onDelete,
  deleting,
}: {
  domain: DomainSummary;
  canManage: boolean;
  edgeIp: string | null;
  onUpdatePort: (port: number) => Promise<DomainSummary>;
  onRetry: () => void;
  retrying: boolean;
  onDelete: () => void;
  deleting: boolean;
}) {
  const meta = TLS_META[domain.tls_status];
  const needsSetup = domain.tls_status !== 'active';
  const canRetry = canManage && domain.tls_status !== 'active';

  return (
    <Card className="group p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <a
            href={`https://${domain.hostname}`}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 truncate font-mono text-sm font-medium hover:text-[var(--color-accent)]"
          >
            <span className="truncate">{domain.hostname}</span>
            <ExternalLink className="h-3.5 w-3.5 opacity-0 transition-opacity group-hover:opacity-60" />
          </a>
          <div className="mt-1 flex items-center gap-3 text-xs text-[var(--color-muted)]">
            <span className="inline-flex items-center gap-1">
              <span>port</span>
              {canManage ? (
                <PortCell domain={domain} onSave={onUpdatePort} />
              ) : (
                <span className="font-mono text-[var(--color-fg)]">
                  {domain.container_port}
                </span>
              )}
            </span>
            <span className="text-[var(--color-subtle)]">·</span>
            <span>
              added <RelativeTime date={domain.created_at} className="!text-[var(--color-muted)]" />
            </span>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <StatusPill status={meta.status} label={meta.label} pulse={meta.pulse} />
          {canRetry ? (
            <RetryButton onClick={onRetry} pending={retrying} />
          ) : null}
          {canManage ? (
            <ConfirmDelete onConfirm={onDelete} pending={deleting} />
          ) : null}
        </div>
      </div>

      {needsSetup || domain.last_error ? (
        <div className="mt-3 border-t border-[var(--color-border)] pt-3">
          {domain.tls_status === 'pending' ? (
            <PendingHelp hostname={domain.hostname} edgeIp={edgeIp} />
          ) : null}
          {domain.tls_status === 'failed' && domain.last_error ? (
            <Stack gap={2}>
              <p className="text-xs text-red-400">{domain.last_error}</p>
              <PendingHelp hostname={domain.hostname} edgeIp={edgeIp} />
            </Stack>
          ) : null}
        </div>
      ) : null}
    </Card>
  );
}

function RetryButton({
  onClick,
  pending,
}: {
  onClick: () => void;
  pending: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={pending}
      title="Re-check DNS and re-issue the certificate"
      className="inline-flex h-7 items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)] disabled:opacity-60"
    >
      <RefreshCw
        className={['h-3 w-3', pending ? 'animate-spin' : ''].join(' ')}
      />
      {pending ? 'Retrying…' : 'Retry'}
    </button>
  );
}

function PendingHelp({
  hostname,
  edgeIp,
}: {
  hostname: string;
  edgeIp: string | null;
}) {
  const name = dnsRecordName(hostname);
  const ip = edgeIp ?? '<your edge IP>';
  const cloudflare = useCloudflareDetection(hostname);
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--color-muted)]">
        <span>Point DNS at the edge, then HTTPS issues the cert:</span>
        <span className="inline-flex items-center gap-1.5 rounded border border-[var(--color-border)] bg-black/[0.03] px-1.5 py-0.5 font-mono text-[var(--color-fg)] dark:bg-white/[0.03]">
          A {name} → {ip}
        </span>
        {edgeIp ? <CopyBtn value={edgeIp} /> : null}
      </div>
      {cloudflare.detected ? (
        <CloudflareConnect
          apex={cloudflare.apex}
          recordName={name}
          nodeIp={edgeIp}
        />
      ) : null}
    </div>
  );
}

function ConfirmDelete({
  onConfirm,
  pending,
}: {
  onConfirm: () => void;
  pending: boolean;
}) {
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (!armed) return;
    const id = setTimeout(() => setArmed(false), 3000);
    return () => clearTimeout(id);
  }, [armed]);

  if (pending) {
    return (
      <span className="text-xs text-[var(--color-muted)]">Deleting…</span>
    );
  }

  if (armed) {
    return (
      <button
        type="button"
        onClick={() => {
          onConfirm();
          setArmed(false);
        }}
        className="inline-flex h-7 items-center rounded-md border border-red-500/60 px-2 text-xs font-medium text-red-400 hover:bg-red-500/10"
      >
        Confirm delete
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={() => setArmed(true)}
      aria-label="Delete domain"
      title="Delete domain"
      className="inline-flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-muted)] hover:bg-red-500/10 hover:text-red-400"
    >
      <Trash2 className="h-3.5 w-3.5" />
    </button>
  );
}

/* ---------- port cell ---------- */

function PortCell({
  domain,
  onSave,
}: {
  domain: DomainSummary;
  onSave: (port: number) => Promise<DomainSummary>;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(String(domain.container_port));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!editing) setValue(String(domain.container_port));
  }, [domain.container_port, editing]);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);

  async function commit() {
    const n = Number(value);
    if (!Number.isFinite(n) || n < 1 || n > 65535) {
      setError(true);
      return;
    }
    if (n === domain.container_port) {
      setEditing(false);
      setError(false);
      return;
    }
    setSaving(true);
    setError(false);
    try {
      await onSave(n);
      setEditing(false);
    } catch {
      setError(true);
    } finally {
      setSaving(false);
    }
  }

  function cancel() {
    setValue(String(domain.container_port));
    setEditing(false);
    setError(false);
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void commit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  }

  if (editing) {
    return (
      <input
        ref={inputRef}
        type="number"
        min={1}
        max={65535}
        value={value}
        disabled={saving}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={onKeyDown}
        className={[
          'h-6 w-16 rounded border bg-transparent px-1.5 font-mono text-xs focus:outline-none',
          error
            ? 'border-red-500/60 focus:border-red-500'
            : 'border-[var(--color-border)] focus:border-[var(--color-accent)]',
        ].join(' ')}
      />
    );
  }

  return (
    <button
      type="button"
      onClick={() => setEditing(true)}
      title="Click to edit"
      className="rounded px-1 font-mono text-[var(--color-fg)] hover:bg-black/5 dark:hover:bg-white/5"
    >
      {domain.container_port}
    </button>
  );
}

/* ---------- copy button ---------- */

function CopyBtn({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  if (!value) return null;
  return (
    <button
      type="button"
      onClick={async () => {
        await navigator.clipboard.writeText(value);
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      }}
      className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-muted)] hover:text-[var(--color-fg)]"
      aria-label="Copy"
    >
      {copied ? (
        <Check className="h-3 w-3 text-emerald-400" />
      ) : (
        <Copy className="h-3 w-3" />
      )}
    </button>
  );
}

/* ---------- helpers ---------- */

/** `api.example.com` → `api`, `example.com` → `@`. */
function dnsRecordName(hostname: string): string {
  if (!hostname) return '';
  const parts = hostname.split('.');
  if (parts.length <= 2) return '@';
  return parts.slice(0, parts.length - 2).join('.');
}

/* ---------- cloudflare detection ---------- */

/**
 * Probes Cloudflare's DNS-over-HTTPS for the apex domain's NS records.
 * Cloudflare-managed zones return nameservers under `*.ns.cloudflare.com`.
 */
function useCloudflareDetection(hostname: string): {
  detected: boolean;
  apex: string;
} {
  const apex = apexDomain(hostname);
  const query = useQuery({
    queryKey: ['cloudflare-ns', apex] as const,
    enabled: apex.length > 0,
    staleTime: 5 * 60_000,
    retry: false,
    queryFn: async ({ signal }) => {
      const res = await fetch(
        `https://cloudflare-dns.com/dns-query?name=${encodeURIComponent(apex)}&type=NS`,
        { headers: { Accept: 'application/dns-json' }, signal },
      );
      if (!res.ok) return false;
      const data = (await res.json()) as { Answer?: Array<{ data?: string }> };
      return (data.Answer ?? []).some(
        (a) =>
          typeof a.data === 'string' &&
          a.data.toLowerCase().includes('.ns.cloudflare.com'),
      );
    },
  });
  return { detected: query.data === true, apex };
}

function apexDomain(host: string): string {
  const trimmed = host.trim().toLowerCase().replace(/\.+$/, '');
  if (!trimmed) return '';
  const parts = trimmed.split('.').filter(Boolean);
  if (parts.length < 2) return '';
  return parts.slice(-2).join('.');
}

function CloudflareConnect({
  apex,
  recordName,
  nodeIp,
}: {
  apex: string;
  recordName: string;
  nodeIp: string | null;
}) {
  const url = `https://dash.cloudflare.com/?to=/:account/${encodeURIComponent(apex)}/dns/records`;
  // Cloudflare doesn't accept deeplink params for new DNS records, so the
  // best we can do is land the user on the right zone's DNS page and copy
  // the IP onto their clipboard so paste-into-content is one keystroke.
  function handleConnect(e: React.MouseEvent<HTMLAnchorElement>) {
    if (nodeIp) {
      void navigator.clipboard.writeText(nodeIp).catch(() => {
        // Clipboard API can fail in cross-origin iframes; the new tab still opens.
      });
    }
    // Let the default open-in-new-tab behavior run.
    void e;
  }

  return (
    <div className="mt-3 rounded-md border border-amber-500/30 bg-amber-500/5 p-3 text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <CloudflareMark className="h-4 w-auto shrink-0" />
          <span className="text-[var(--color-fg)]">
            Cloudflare detected on{' '}
            <span className="font-mono">{apex}</span>
          </span>
        </div>
        <a
          href={url}
          target="_blank"
          rel="noreferrer"
          onClick={handleConnect}
          className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-amber-500/40 bg-amber-500/10 px-2 font-medium text-amber-600 hover:bg-amber-500/20 dark:text-amber-400"
        >
          Connect
          <ExternalLink className="h-3 w-3" />
        </a>
      </div>
      <div className="mt-2.5 grid grid-cols-[auto_1fr_auto] items-center gap-x-3 gap-y-1 font-mono text-[11px]">
        <span className="text-[var(--color-muted)]">type</span>
        <span className="text-[var(--color-fg)]">A</span>
        <span />

        <span className="text-[var(--color-muted)]">name</span>
        <span className="text-[var(--color-fg)]">{recordName || '—'}</span>
        {recordName ? <CopyBtn value={recordName} /> : <span />}

        <span className="text-[var(--color-muted)]">content</span>
        <span className="text-[var(--color-fg)]">{nodeIp ?? '—'}</span>
        {nodeIp ? <CopyBtn value={nodeIp} /> : <span />}

        <span className="text-[var(--color-muted)]">proxy</span>
        <span className="text-[var(--color-fg)]">DNS only (gray cloud)</span>
        <span />
      </div>
      <p className="mt-2 text-[11px] leading-relaxed text-[var(--color-muted)]">
        Connect copies the IP to your clipboard. In Cloudflare, click{' '}
        <span className="font-mono text-[var(--color-fg)]">Add record</span>,
        pick <span className="font-mono text-[var(--color-fg)]">A</span>, paste
        the values above, and turn the orange cloud{' '}
        <span className="font-mono text-[var(--color-fg)]">off</span> so Caddy
        can issue its own cert.
      </p>
    </div>
  );
}

function CloudflareMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 256 116"
      xmlns="http://www.w3.org/2000/svg"
      preserveAspectRatio="xMidYMid"
      aria-label="Cloudflare"
      className={className}
    >
      <path
        fill="#FFF"
        d="m202.357 49.394-5.311-2.124C172.085 103.434 72.786 69.289 66.81 85.997c-.996 11.286 54.227 2.146 93.706 4.059 12.039.583 18.076 9.671 12.964 24.484l10.069.031c11.615-36.209 48.683-17.73 50.232-29.68-2.545-7.857-42.601 0-31.425-35.497Z"
      />
      <path
        fill="#F4811F"
        d="M176.332 108.348c1.593-5.31 1.062-10.622-1.593-13.809-2.656-3.187-6.374-5.31-11.154-5.842L71.17 87.634c-.531 0-1.062-.53-1.593-.53-.531-.532-.531-1.063 0-1.594.531-1.062 1.062-1.594 2.124-1.594l92.946-1.062c11.154-.53 22.838-9.56 27.087-20.182l5.312-13.809c0-.532.531-1.063 0-1.594C191.203 20.182 166.772 0 138.091 0 111.535 0 88.697 16.995 80.73 40.896c-5.311-3.718-11.684-5.843-19.12-5.31-12.747 1.061-22.838 11.683-24.432 24.43-.531 3.187 0 6.374.532 9.56C16.996 70.107 0 87.103 0 108.348c0 2.124 0 3.718.531 5.842 0 1.063 1.062 1.594 1.594 1.594h170.489c1.062 0 2.125-.53 2.125-1.594l1.593-5.842Z"
      />
      <path
        fill="#FAAD3F"
        d="M205.544 48.863h-2.656c-.531 0-1.062.53-1.593 1.062l-3.718 12.747c-1.593 5.31-1.062 10.623 1.594 13.809 2.655 3.187 6.373 5.31 11.153 5.843l19.652 1.062c.53 0 1.062.53 1.593.53.53.532.53 1.063 0 1.594-.531 1.063-1.062 1.594-2.125 1.594l-20.182 1.062c-11.154.53-22.838 9.56-27.087 20.182l-1.063 4.78c-.531.532 0 1.594 1.063 1.594h70.108c1.062 0 1.593-.531 1.593-1.593 1.062-4.25 2.124-9.03 2.124-13.81 0-27.618-22.838-50.456-50.456-50.456"
      />
    </svg>
  );
}
