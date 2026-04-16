import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { useAddDomain, useDeleteDomain, domainsQuery } from '@/lib/domains';
import { nodesQuery } from '@/lib/nodes';
import { ApiError } from '@/lib/api';
import { Button, Card, ErrorText, Field, Input } from '@/components/ui';
import type { TlsStatus } from '@/lib/types';

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
  const nodes = useQuery(nodesQuery(workspaceSlug));

  const [hostname, setHostname] = useState('');
  const [port, setPort] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Any ready hetzner node's public IPv4 — what users should A-record at.
  const readyNode = nodes.data?.find(
    (n) => n.provider === 'hetzner' && n.status === 'ready' && n.public_ipv4,
  );
  const dnsHint = readyNode?.public_ipv4
    ?? 'the public IP of the node hosting this service';

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
        <Card className="mb-4 p-4">
          <form
            onSubmit={onSubmit}
            className="grid grid-cols-[1fr_140px_auto] items-end gap-3"
          >
            <Field label="Hostname" htmlFor="dom-host" hint="e.g. api.example.com">
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
              hint={defaultPort ? `Default: ${defaultPort}` : 'Default: 80'}
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
          <p className="mt-3 text-xs text-[var(--color-muted)]">
            Point an <span className="font-mono">A</span> record for your hostname at{' '}
            <span className="font-mono">{dnsHint}</span>. Caddy on the node will issue a
            Let&apos;s Encrypt cert on the first HTTPS hit.
          </p>
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
                  <td className="px-4 py-2 font-mono text-xs">{d.hostname}</td>
                  <td className="px-4 py-2 font-mono text-xs">{d.container_port}</td>
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
