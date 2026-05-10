import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type InputHTMLAttributes,
  type KeyboardEvent,
} from 'react';
import type { EnvVars, VariableReferencesResponse } from '@/lib/types';

const GENERATED_PRIVATE_HOSTNAME = 'DRIFTBASE_PRIVATE_HOSTNAME';

type EnvReferenceInputProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'onChange' | 'value'
> & {
  value: string;
  onChange: (value: string) => void;
  references?: VariableReferencesResponse;
  currentEnv?: EnvVars;
  includeCurrentServiceShorthand?: boolean;
};

interface ReferenceSuggestion {
  id: string;
  label: string;
  expression: string;
  detail: string;
  search: string;
}

interface ReferenceTrigger {
  start: number;
  cursor: number;
  fragment: string;
}

export function EnvReferenceInput({
  value,
  onChange,
  references,
  currentEnv,
  includeCurrentServiceShorthand = false,
  className,
  disabled,
  onBlur,
  onFocus,
  onKeyDown,
  onClick,
  onKeyUp,
  ...inputProps
}: EnvReferenceInputProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const trigger = cursor === null ? null : findReferenceTrigger(value, cursor);
  const allSuggestions = useMemo(
    () => buildSuggestions(references, currentEnv, includeCurrentServiceShorthand),
    [references, currentEnv, includeCurrentServiceShorthand],
  );
  const suggestions = useMemo(() => {
    if (!trigger) return [];
    return allSuggestions
      .filter((suggestion) => matchesFragment(suggestion, trigger.fragment))
      .slice(0, 8);
  }, [allSuggestions, trigger]);
  const isOpen = !disabled && !!trigger && suggestions.length > 0;

  useEffect(() => {
    setActiveIndex(0);
  }, [trigger?.fragment, suggestions.length]);

  function syncCursor() {
    const next = inputRef.current?.selectionStart ?? value.length;
    setCursor(next);
  }

  function insertSuggestion(suggestion: ReferenceSuggestion) {
    const input = inputRef.current;
    const currentCursor = input?.selectionStart ?? trigger?.cursor ?? value.length;
    const activeTrigger = findReferenceTrigger(value, currentCursor);
    if (!activeTrigger) return;

    const close = value.indexOf('}}', currentCursor);
    const end = close === -1 ? currentCursor : close + 2;
    const next =
      value.slice(0, activeTrigger.start) +
      suggestion.expression +
      value.slice(end);
    const nextCursor = activeTrigger.start + suggestion.expression.length;
    onChange(next);
    setCursor(nextCursor);
    requestAnimationFrame(() => {
      input?.focus();
      input?.setSelectionRange(nextCursor, nextCursor);
    });
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (isOpen) {
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setActiveIndex((idx) => (idx + 1) % suggestions.length);
        return;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setActiveIndex((idx) => (idx - 1 + suggestions.length) % suggestions.length);
        return;
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault();
        insertSuggestion(suggestions[activeIndex]);
        return;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        setCursor(null);
        return;
      }
    }
    onKeyDown?.(event);
  }

  return (
    <div className="relative min-w-0">
      <input
        {...inputProps}
        ref={inputRef}
        value={value}
        disabled={disabled}
        onChange={(event) => {
          onChange(event.target.value);
          setCursor(event.target.selectionStart ?? event.target.value.length);
        }}
        onFocus={(event) => {
          setCursor(event.target.selectionStart ?? event.target.value.length);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          window.setTimeout(() => setCursor(null), 100);
          onBlur?.(event);
        }}
        onClick={(event) => {
          syncCursor();
          onClick?.(event);
        }}
        onKeyUp={(event) => {
          syncCursor();
          onKeyUp?.(event);
        }}
        onKeyDown={handleKeyDown}
        className={[
          'h-9 w-full rounded-md border border-[var(--color-border)] bg-transparent px-3 text-sm',
          'placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:outline-none',
          className ?? '',
        ].join(' ')}
      />
      {isOpen ? (
        <div className="absolute left-0 right-0 top-full z-50 mt-1 max-h-56 overflow-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-1 shadow-xl">
          {suggestions.map((suggestion, index) => (
            <button
              key={suggestion.id}
              type="button"
              onMouseDown={(event) => {
                event.preventDefault();
                insertSuggestion(suggestion);
              }}
              className={[
                'flex w-full items-center justify-between gap-3 rounded px-2 py-1.5 text-left',
                index === activeIndex
                  ? 'bg-[var(--color-accent)]/10 text-[var(--color-fg)]'
                  : 'text-[var(--color-muted)] hover:bg-black/5 hover:text-[var(--color-fg)] dark:hover:bg-white/5',
              ].join(' ')}
            >
              <span className="truncate font-mono text-xs">{suggestion.label}</span>
              <span className="shrink-0 text-[10px] uppercase tracking-wider">
                {suggestion.detail}
              </span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function buildSuggestions(
  references?: VariableReferencesResponse,
  currentEnv?: EnvVars,
  includeCurrentServiceShorthand = false,
): ReferenceSuggestion[] {
  const suggestions = new Map<string, ReferenceSuggestion>();

  if (includeCurrentServiceShorthand) {
    const keys = Object.keys(currentEnv ?? {})
      .filter(isValidEnvKey)
      .sort();
    for (const key of keys) {
      addSuggestion(suggestions, {
        id: `self:${key}`,
        label: key,
        expression: selfExpression(key),
        detail: 'same service',
        search: key,
      });
    }
    if (!currentEnv || !(GENERATED_PRIVATE_HOSTNAME in currentEnv)) {
      addSuggestion(suggestions, {
        id: `self:${GENERATED_PRIVATE_HOSTNAME}`,
        label: GENERATED_PRIVATE_HOSTNAME,
        expression: selfExpression(GENERATED_PRIVATE_HOSTNAME),
        detail: 'generated',
        search: GENERATED_PRIVATE_HOSTNAME,
      });
    }
  }

  for (const service of references?.services ?? []) {
    for (const variable of service.variables) {
      const label = `${service.slug}.${variable.key}`;
      addSuggestion(suggestions, {
        id: `${service.slug}:${variable.key}:${variable.kind}`,
        label,
        expression: variable.expression,
        detail: variable.kind === 'generated' ? 'generated' : service.name,
        search: `${label} ${service.name} ${variable.key}`,
      });
    }
  }

  return [...suggestions.values()];
}

function addSuggestion(
  suggestions: Map<string, ReferenceSuggestion>,
  suggestion: ReferenceSuggestion,
) {
  if (!suggestions.has(suggestion.expression)) {
    suggestions.set(suggestion.expression, suggestion);
  }
}

function matchesFragment(suggestion: ReferenceSuggestion, fragment: string): boolean {
  const needle = fragment.trim().toLowerCase();
  if (!needle) return true;
  return suggestion.search.toLowerCase().includes(needle);
}

function findReferenceTrigger(value: string, cursor: number): ReferenceTrigger | null {
  const start = value.lastIndexOf('${{', cursor);
  if (start === -1) return null;
  const closeBeforeCursor = value.lastIndexOf('}}', cursor);
  if (closeBeforeCursor > start) return null;
  const fragment = value.slice(start + 3, cursor);
  if (fragment.includes('\n') || fragment.includes('\r')) return null;
  return { start, cursor, fragment };
}

function selfExpression(key: string): string {
  return `\${{${key}}}`;
}

function isValidEnvKey(key: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(key);
}
