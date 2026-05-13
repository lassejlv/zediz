import { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useLocation } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { AnimatePresence, motion, useReducedMotion } from 'motion/react';
import {
  FolderKanban,
  LayoutDashboard,
  LogOut,
  Moon,
  Search,
  Sun,
  Boxes,
} from 'lucide-react';
import { useCommandPalette } from '@/components/app-shell';
import { useWorkspaces } from '@/lib/workspaces';
import { projectsQuery } from '@/lib/projects';
import { useMe, useLogout } from '@/lib/auth';

interface PaletteItem {
  id: string;
  group: string;
  label: string;
  hint?: string;
  icon: typeof FolderKanban;
  onSelect: () => void;
}

export function CommandPalette() {
  const { open, setOpen } = useCommandPalette();
  const me = useMe();
  const logout = useLogout();
  const navigate = useNavigate();
  const location = useLocation();
  const reduce = useReducedMotion();

  const workspaceSlug = useMemo(() => {
    const m = location.pathname.match(/^\/w\/([^/]+)/);
    return m ? decodeURIComponent(m[1]) : null;
  }, [location.pathname]);

  const workspaces = useWorkspaces();
  const projects = useQuery({
    ...projectsQuery(workspaceSlug ?? ''),
    enabled: !!workspaceSlug && open,
  });

  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (open) {
      setQuery('');
      setCursor(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const items = useMemo<PaletteItem[]>(() => {
    const out: PaletteItem[] = [];

    if (workspaceSlug && projects.data) {
      for (const p of projects.data) {
        out.push({
          id: `proj:${p.id}`,
          group: 'Projects',
          label: p.name,
          hint: p.slug,
          icon: FolderKanban,
          onSelect: () => {
            navigate({
              to: '/w/$workspaceSlug/projects/$projectSlug',
              params: { workspaceSlug, projectSlug: p.slug },
            });
          },
        });
      }
    }

    if (workspaces.data) {
      for (const w of workspaces.data) {
        if (w.slug === workspaceSlug) continue;
        out.push({
          id: `ws:${w.id}`,
          group: 'Workspaces',
          label: w.name,
          hint: w.slug,
          icon: Boxes,
          onSelect: () => {
            window.location.href = `/w/${w.slug}`;
          },
        });
      }
    }

    if (workspaceSlug) {
      out.push({
        id: 'nav:home',
        group: 'Navigate',
        label: 'Workspace home',
        icon: LayoutDashboard,
        onSelect: () => navigate({ to: '/w/$workspaceSlug', params: { workspaceSlug } }),
      });
      out.push({
        id: 'nav:projects',
        group: 'Navigate',
        label: 'All projects',
        icon: FolderKanban,
        onSelect: () =>
          navigate({ to: '/w/$workspaceSlug/projects', params: { workspaceSlug } }),
      });
    }

    out.push({
      id: 'act:theme',
      group: 'Actions',
      label: 'Toggle theme',
      icon: isDark() ? Sun : Moon,
      onSelect: () => toggleTheme(),
    });

    if (me.data) {
      out.push({
        id: 'act:signout',
        group: 'Actions',
        label: 'Sign out',
        icon: LogOut,
        onSelect: () => logout.mutate(),
      });
    }

    return out;
  }, [workspaces.data, projects.data, workspaceSlug, navigate, logout, me.data]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (it) =>
        it.label.toLowerCase().includes(q) ||
        (it.hint?.toLowerCase().includes(q) ?? false) ||
        it.group.toLowerCase().includes(q),
    );
  }, [items, query]);

  useEffect(() => {
    setCursor(0);
  }, [query]);

  const grouped = useMemo(() => {
    const map = new Map<string, PaletteItem[]>();
    for (const it of filtered) {
      const list = map.get(it.group) ?? [];
      list.push(it);
      map.set(it.group, list);
    }
    return Array.from(map.entries());
  }, [filtered]);

  function handleSelect(item: PaletteItem) {
    setOpen(false);
    item.onSelect();
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setCursor((c) => Math.min(filtered.length - 1, c + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setCursor((c) => Math.max(0, c - 1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const item = filtered[cursor];
      if (item) handleSelect(item);
    }
  }

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          className="fixed inset-0 z-50 flex items-start justify-center p-4 pt-[12vh]"
        >
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-sm"
            onClick={() => setOpen(false)}
          />
          <motion.div
            initial={reduce ? { opacity: 0 } : { opacity: 0, y: -12, scale: 0.98 }}
            animate={reduce ? { opacity: 1 } : { opacity: 1, y: 0, scale: 1 }}
            exit={reduce ? { opacity: 0 } : { opacity: 0, y: -8, scale: 0.98 }}
            transition={{ type: 'spring', stiffness: 360, damping: 30 }}
            className="relative z-10 w-full max-w-xl overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-elevated)] shadow-2xl"
            onKeyDown={onKey}
          >
            <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3 py-2.5">
              <Search className="h-3.5 w-3.5 text-[var(--color-muted)]" />
              <input
                ref={inputRef}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Jump to anything…"
                className="w-full bg-transparent text-sm focus:outline-none"
              />
              <span className="font-mono text-[10px] text-[var(--color-muted)]">esc</span>
            </div>
            <div className="max-h-[60vh] overflow-y-auto py-2">
              {filtered.length === 0 ? (
                <div className="px-3 py-6 text-center text-xs text-[var(--color-muted)]">
                  Nothing matches.
                </div>
              ) : (
                grouped.map(([group, items]) => (
                  <div key={group} className="mb-2 last:mb-0">
                    <div className="px-3 pb-1 pt-2 text-[10px] font-medium uppercase tracking-wider text-[var(--color-muted)]">
                      {group}
                    </div>
                    {items.map((it) => {
                      const idx = filtered.indexOf(it);
                      const active = idx === cursor;
                      return (
                        <button
                          key={it.id}
                          type="button"
                          onMouseEnter={() => setCursor(idx)}
                          onClick={() => handleSelect(it)}
                          className={[
                            'flex w-full items-center gap-2.5 px-3 py-1.5 text-left text-sm',
                            active
                              ? 'bg-black/[0.06] text-[var(--color-fg)] dark:bg-white/[0.06]'
                              : 'text-[var(--color-fg)] hover:bg-black/[0.04] dark:hover:bg-white/[0.04]',
                          ].join(' ')}
                        >
                          <it.icon className="h-3.5 w-3.5 shrink-0 text-[var(--color-muted)]" />
                          <span className="flex-1 truncate">{it.label}</span>
                          {it.hint ? (
                            <span className="shrink-0 font-mono text-[11px] text-[var(--color-muted)]">
                              {it.hint}
                            </span>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>
                ))
              )}
            </div>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}

function isDark(): boolean {
  if (typeof document === 'undefined') return false;
  return document.documentElement.classList.contains('dark');
}

function toggleTheme() {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const next = !root.classList.contains('dark');
  root.classList.toggle('dark', next);
  try {
    window.localStorage.setItem('theme', next ? 'dark' : 'light');
  } catch {
    // ignore
  }
}
