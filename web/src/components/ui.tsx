import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

const buttonStyles: Record<ButtonVariant, string> = {
  primary:
    'bg-[var(--color-accent)] text-black hover:brightness-110 disabled:opacity-50',
  secondary:
    'border border-[var(--color-border)] text-[var(--color-fg)] hover:bg-black/5 dark:hover:bg-white/5',
  ghost:
    'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
  danger:
    'border border-red-500/40 text-red-400 hover:bg-red-500/10',
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
        'inline-flex h-9 items-center justify-center rounded-md px-3 text-sm font-medium transition',
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

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement> & { children: ReactNode }) {
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
      className="text-xs font-medium uppercase tracking-wider text-[var(--color-muted)]"
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
  label: string;
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

export function Card({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={[
        'rounded-lg border border-[var(--color-border)] bg-black/0',
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
