import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { Copy } from 'lucide-react';
import {
  useAddDomain,
  useDeleteDomain,
  useUpdateDomain,
  domainsQuery,
} from '@/lib/domains';
import { nodesQuery } from '@/lib/nodes';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input } from '@/components/ui';
import type { DomainSummary, TlsStatus } from '@/lib/types';

const STATUS_COLOR: Record<TlsStatus, string> = {
  pending: 'text-yellow-400',
  active: 'text-green-400',
  failed: 'text-red-400',
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
  const nodes = useQuery(nodesQuery(workspaceSlug));

  const [hostname, setHostname] = useState('');
  const [port, setPort] = useState('');
  const [error, setError] = useState<string | null>(null);

  const readyNode = nodes.data?.find(
    (n) => n.provider === 'hetzner' && n.status === 'ready' && n.public_ipv4,
  );
  const nodeIp = readyNode?.public_ipv4 ?? null;

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    try {
      const body: { hostname: string; container_port?: number } = {
        hostname: hostname.trim().toLowerCase(),
      };
      if (port.trim()) {
        const n = Number(port);
        if (!Number.isFinite(n)) throw new Error('invalid port');
        body.container_port = n;
      }
      await add.mutateAsync(body);
      setHostname('');
      setPort('');
    } catch (err) {
      setError(
        err instanceof ApiError ? err.message : err instanceof Error ? err.message : 'Failed',
      );
    }
  }

  return (
    <div>
      <h2 className="mb-3 text-sm font-medium">Domains</h2>

      {canManage ? (
        <Card className="mb-4 p-5">
          <form
            onSubmit={onSubmit}
            className="grid grid-cols-[1fr_160px_auto] items-end gap-3"
          >
            <Field
              label="Hostname"
              htmlFor="dom-host"
              hint="The public domain you want the service to answer on."
            >
              <Input
                id="dom-host"
                required
                placeholder="api.example.com"
                value={hostname}
                onChange={(e) => setHostname(e.target.value)}
              />
            </Field>
            <Field
              label="Container port"
              htmlFor="dom-port"
              hint={
                defaultPort
                  ? `Port inside the container (service default: ${defaultPort}).`
                  : 'Port inside the container (e.g. 80 for nginx, 3000 for Node).'
              }
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
            <Button type="submit" disabled={add.isPending}>
              {add.isPending ? 'Adding…' : 'Add'}
            </Button>
          </form>
          {error ? (
            <div className="mt-3">
              <ErrorText>{error}</ErrorText>
            </div>
          ) : null}

          <DnsGuide hostname={hostname.trim().toLowerCase()} nodeIp={nodeIp} />
        </Card>
      ) : null}

      <Card className="overflow-hidden">
        <table className="w-full text-sm">
          <thead className="text-left text-xs uppercase tracking-wider text-[var(--color-muted)]">
            <tr>
              <th className="px-4 py-2 font-medium">Hostname</th>
              <th className="px-4 py-2 font-medium">Port</th>
              <th className="px-4 py-2 font-medium">TLS</th>
              <th className="px-4 py-2 font-medium">Added</th>
              <th className="px-4 py-2" />
            </tr>
          </thead>
          <tbody>
            {domains.data?.length ? (
              domains.data.map((d) => (
                <tr key={d.id} className="border-t border-[var(--color-border)]">
                  <td className="px-4 py-2 font-mono text-xs">
                    <a
                      href={`https://${d.hostname}`}
                      target="_blank"
                      rel="noreferrer"
                      className="hover:underline"
                    >
                      {d.hostname}
                    </a>
                  </td>
                  <td className="px-4 py-2">
                    {canManage ? (
                      <PortCell
                        domain={d}
                        onSave={(port) =>
                          updateDomain.mutateAsync({ id: d.id, container_port: port })
                        }
                      />
                    ) : (
                      <span className="font-mono text-xs">{d.container_port}</span>
                    )}
                  </td>
                  <td className="px-4 py-2">
                    <span className={`font-mono text-xs ${STATUS_COLOR[d.tls_status]}`}>
                      {d.tls_status}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-[var(--color-muted)]">
                    {new Date(d.created_at).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-2 text-right">
                    {canManage ? (
                      <Button
                        variant="danger"
                        onClick={() => {
                          if (confirm(`Delete ${d.hostname}?`)) del.mutate(d.id);
                        }}
                      >
                        Delete
                      </Button>
                    ) : null}
                  </td>
                </tr>
              ))
            ) : (
              <tr>
                <td
                  colSpan={5}
                  className="px-4 py-6 text-center text-sm text-[var(--color-muted)]"
                >
                  No domains yet.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function DnsGuide({ hostname, nodeIp }: { hostname: string; nodeIp: string | null }) {
  const recordName = dnsRecordName(hostname);
  const displayIp = nodeIp ?? '<your node IP>';

  return (
    <div className="mt-5 border-t border-[var(--color-border)] pt-5">
      <h3 className="mb-2 text-xs font-medium uppercase tracking-wider text-[var(--color-muted)]">
        DNS setup
      </h3>
      <ol className="space-y-3 text-sm">
        <li>
          <div>
            At your DNS provider, add this record:
          </div>
          <div className="mt-2 grid grid-cols-[90px_1fr_auto] items-center gap-x-3 gap-y-1 rounded-md border border-[var(--color-border)] px-3 py-2 font-mono text-xs">
            <span className="text-[var(--color-muted)]">Type</span>
            <span>A</span>
            <span />

            <span className="text-[var(--color-muted)]">Name</span>
            <span>{recordName || '<subdomain>'}</span>
            <CopyBtn value={recordName} />

            <span className="text-[var(--color-muted)]">Value</span>
            <span>{displayIp}</span>
            <CopyBtn value={nodeIp ?? ''} />

            <span className="text-[var(--color-muted)]">TTL</span>
            <span>automatic (or 300)</span>
            <span />
          </div>
          <p className="mt-1 text-xs text-[var(--color-muted)]">
            Use <span className="font-mono">@</span> for the apex (
            <span className="font-mono">example.com</span>). Anything else is the subdomain
            label only — not the full hostname.
          </p>
        </li>
        <li>
          Wait ~1 min for DNS to propagate. Verify with{' '}
          <span className="font-mono">
            dig +short {hostname || '<hostname>'}
          </span>{' '}
          — it should return <span className="font-mono">{displayIp}</span>.
        </li>
        <li>
          Visit{' '}
          <span className="font-mono">
            https://{hostname || '<hostname>'}
          </span>
          . Caddy on the node issues a Let&apos;s Encrypt cert on the first HTTPS hit; TLS
          status flips from <span className="font-mono">pending</span> to{' '}
          <span className="font-mono">active</span> within a few seconds.
        </li>
      </ol>
      {!nodeIp ? (
        <p className="mt-3 text-xs text-yellow-400">
          No ready Hetzner node found in this workspace yet — provision one before DNS
          propagates.
        </p>
      ) : null}
    </div>
  );
}

function CopyBtn({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  if (!value) return <span />;
  return (
    <button
      type="button"
      onClick={async () => {
        await navigator.clipboard.writeText(value);
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      }}
      className="inline-flex h-6 w-6 items-center justify-center rounded text-[var(--color-muted)] hover:text-[var(--color-fg)]"
      aria-label="Copy"
    >
      {copied ? (
        <span className="text-[10px] text-green-400">OK</span>
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </button>
  );
}

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
          'h-7 w-20 rounded border bg-transparent px-2 font-mono text-xs focus:outline-none',
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
      className="rounded px-1 font-mono text-xs hover:bg-black/5 dark:hover:bg-white/5"
    >
      {domain.container_port}
    </button>
  );
}

/**
 * Split a hostname into the DNS record name. `api.example.com` → `api`.
 * `example.com` (apex) → `@`.
 */
function dnsRecordName(hostname: string): string {
  if (!hostname) return '';
  const parts = hostname.split('.');
  if (parts.length <= 2) return '@';
  return parts.slice(0, parts.length - 2).join('.');
}
