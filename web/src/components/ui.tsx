import {
  useEffect,
  useState,
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from 'react';
import { Check, Copy } from 'lucide-react';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

const buttonStyles: Record<ButtonVariant, string> = {
  primary:
    'bg-[var(--color-accent)] text-black hover:brightness-110 disabled:opacity-50',
  secondary:
    'border border-[var(--color-border)] text-[var(--color-fg)] hover:border-[var(--color-border-strong)] hover:bg-black/5 dark:hover:bg-white/5',
  ghost: 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
  danger: 'border border-red-500/40 text-red-400 hover:bg-red-500/10',
};

export function Button({
  variant = 'primary',
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  return (
    <button
      {...props}
      className={[
        'inline-flex h-9 items-center justify-center rounded-md px-3 text-sm font-medium transition-colors duration-150',
        buttonStyles[variant],
        className ?? '',
      ].join(' ')}
    />
  );
}

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={[
        'h-9 w-full rounded-md border border-[var(--color-border)] bg-transparent px-3 text-sm',
        'placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:outline-none',
        className ?? '',
      ].join(' ')}
    />
  );
}

export function Select({
  className,
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement> & { children: ReactNode }) {
  return (
    <select
      {...props}
      className={[
        'h-9 w-full rounded-md border border-[var(--color-border)] bg-transparent px-2 text-sm',
        'focus:border-[var(--color-accent)] focus:outline-none',
        className ?? '',
      ].join(' ')}
    >
      {children}
    </select>
  );
}

export function Label({ children, htmlFor }: { children: ReactNode; htmlFor?: string }) {
  return (
    <label
      htmlFor={htmlFor}
      className="text-[11px] font-medium uppercase tracking-wider text-[var(--color-muted)]"
    >
      {children}
    </label>
  );
}

export function Field({
  label,
  htmlFor,
  children,
  hint,
}: {
  label: ReactNode;
  htmlFor?: string;
  children: ReactNode;
  hint?: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
      {hint ? <p className="text-xs text-[var(--color-muted)]">{hint}</p> : null}
    </div>
  );
}

export function Card({
  children,
  className,
  ...rest
}: HTMLAttributes<HTMLDivElement> & { children: ReactNode }) {
  return (
    <div
      {...rest}
      className={[
        'rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]',
        'transition-colors duration-150',
        className ?? '',
      ].join(' ')}
    >
      {children}
    </div>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  return <p className="text-xs text-red-400">{children}</p>;
}

/* ---------- layout primitives ---------- */

type GapToken = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 8 | 10 | 12;
const gapClass: Record<GapToken, string> = {
  0: 'gap-0',
  1: 'gap-1',
  2: 'gap-2',
  3: 'gap-3',
  4: 'gap-4',
  5: 'gap-5',
  6: 'gap-6',
  8: 'gap-8',
  10: 'gap-10',
  12: 'gap-12',
};

export function Stack({
  gap = 4,
  className,
  children,
}: {
  gap?: GapToken;
  className?: string;
  children: ReactNode;
}) {
  return <div className={['flex flex-col', gapClass[gap], className ?? ''].join(' ')}>{children}</div>;
}

export function Inline({
  gap = 3,
  align = 'center',
  justify = 'start',
  wrap = false,
  className,
  children,
}: {
  gap?: GapToken;
  align?: 'start' | 'center' | 'end' | 'stretch' | 'baseline';
  justify?: 'start' | 'center' | 'end' | 'between';
  wrap?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const alignCls = {
    start: 'items-start',
    center: 'items-center',
    end: 'items-end',
    stretch: 'items-stretch',
    baseline: 'items-baseline',
  }[align];
  const justifyCls = {
    start: 'justify-start',
    center: 'justify-center',
    end: 'justify-end',
    between: 'justify-between',
  }[justify];
  return (
    <div
      className={[
        'flex',
        alignCls,
        justifyCls,
        wrap ? 'flex-wrap' : '',
        gapClass[gap],
        className ?? '',
      ].join(' ')}
    >
      {children}
    </div>
  );
}

/* ---------- status ---------- */

export type SemanticStatus = 'ok' | 'warn' | 'error' | 'info' | 'muted' | 'accent';

const statusDotColor: Record<SemanticStatus, string> = {
  ok: 'bg-emerald-500',
  warn: 'bg-amber-500',
  error: 'bg-red-500',
  info: 'bg-indigo-500',
  muted: 'bg-neutral-500',
  accent: 'bg-emerald-500',
};

export function StatusDot({
  status,
  pulse = false,
  className,
}: {
  status: SemanticStatus;
  pulse?: boolean;
  className?: string;
}) {
  return (
    <span
      className={[
        'inline-block h-1.5 w-1.5 rounded-full',
        statusDotColor[status],
        pulse ? 'animate-status-pulse' : '',
        className ?? '',
      ].join(' ')}
    />
  );
}

export function StatusPill({
  status,
  label,
  pulse = false,
  className,
}: {
  status: SemanticStatus;
  label: ReactNode;
  pulse?: boolean;
  className?: string;
}) {
  return (
    <span
      className={[
        'inline-flex items-center gap-1.5 text-xs text-[var(--color-fg)]',
        className ?? '',
      ].join(' ')}
    >
      <StatusDot status={status} pulse={pulse} />
      <span>{label}</span>
    </span>
  );
}

/* ---------- stat card ---------- */

export function StatCard({
  label,
  value,
  hint,
  mono = false,
  className,
}: {
  label: ReactNode;
  value: ReactNode;
  hint?: ReactNode;
  mono?: boolean;
  className?: string;
}) {
  return (
    <div
      className={[
        'rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3',
        className ?? '',
      ].join(' ')}
    >
      <div className="text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
        {label}
      </div>
      <div
        className={[
          'mt-1 text-[15px] font-medium leading-tight',
          mono ? 'font-mono' : '',
        ].join(' ')}
      >
        {value}
      </div>
      {hint ? (
        <div className="mt-1 text-xs text-[var(--color-muted)]">{hint}</div>
      ) : null}
    </div>
  );
}

/* ---------- empty state ---------- */

export function EmptyState({
  title,
  body,
  cta,
  className,
}: {
  title: ReactNode;
  body?: ReactNode;
  cta?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={[
        'rounded-lg border border-dashed border-[var(--color-border)] px-6 py-10 text-center',
        className ?? '',
      ].join(' ')}
    >
      <div className="text-sm font-medium text-[var(--color-fg)]">{title}</div>
      {body ? (
        <div className="mx-auto mt-1.5 max-w-sm text-xs text-[var(--color-muted)]">
          {body}
        </div>
      ) : null}
      {cta ? <div className="mt-4 flex justify-center">{cta}</div> : null}
    </div>
  );
}

/* ---------- copyable id ---------- */

export function CopyableId({
  value,
  display,
  className,
  title,
}: {
  value: string;
  display?: ReactNode;
  className?: string;
  title?: string;
}) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const id = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(id);
  }, [copied]);

  return (
    <button
      type="button"
      title={title ?? value}
      onClick={async (e) => {
        e.stopPropagation();
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
        } catch {
          // ignore clipboard failures
        }
      }}
      className={[
        'group inline-flex items-center gap-1.5 rounded px-1 font-mono text-xs',
        'hover:text-[var(--color-fg)]',
        className ?? '',
      ].join(' ')}
    >
      <span>{display ?? value}</span>
      {copied ? (
        <Check className="h-3 w-3 text-emerald-400" />
      ) : (
        <Copy className="h-3 w-3 opacity-0 transition-opacity group-hover:opacity-60" />
      )}
    </button>
  );
}

/* ---------- relative time ---------- */

export function RelativeTime({
  date,
  className,
}: {
  date: string | Date;
  className?: string;
}) {
  const iso = typeof date === 'string' ? date : date.toISOString();
  const [, setTick] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 30_000);
    return () => clearInterval(id);
  }, []);

  return (
    <time
      dateTime={iso}
      title={new Date(iso).toLocaleString()}
      className={['text-[var(--color-muted)]', className ?? ''].join(' ')}
    >
      {formatRelative(new Date(iso))}
    </time>
  );
}

function formatRelative(d: Date): string {
  const ms = Date.now() - d.getTime();
  const s = Math.round(ms / 1000);
  if (s < 5) return 'just now';
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const days = Math.round(h / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.round(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.round(months / 12)}y ago`;
}

/* ---------- page header ---------- */

export interface Breadcrumb {
  label: ReactNode;
  to?: string;
}

export function PageHeader({
  title,
  subtitle,
  breadcrumbs,
  status,
  actions,
  className,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  breadcrumbs?: Breadcrumb[];
  status?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div className={['flex flex-col gap-4 pb-6', className ?? ''].join(' ')}>
      {breadcrumbs && breadcrumbs.length > 0 ? (
        <nav className="flex items-center gap-1.5 text-xs text-[var(--color-muted)]">
          {breadcrumbs.map((b, i) => (
            <span key={i} className="flex items-center gap-1.5">
              {i > 0 ? <span className="text-[var(--color-subtle)]">/</span> : null}
              {b.to ? (
                <a
                  href={b.to}
                  className="hover:text-[var(--color-fg)]"
                >
                  {b.label}
                </a>
              ) : (
                <span>{b.label}</span>
              )}
            </span>
          ))}
        </nav>
      ) : null}
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="truncate text-2xl font-semibold tracking-tight">{title}</h1>
            {status}
          </div>
          {subtitle ? (
            <div className="mt-1 text-sm text-[var(--color-muted)]">{subtitle}</div>
          ) : null}
        </div>
        {actions ? <div className="flex shrink-0 items-center gap-2">{actions}</div> : null}
      </div>
    </div>
  );
}
