import { useEffect, useState } from 'react';
import { AnimatePresence, motion, useReducedMotion } from 'motion/react';
import { Moon, Sun } from 'lucide-react';

type Theme = 'light' | 'dark';

function readTheme(): Theme {
  if (typeof document === 'undefined') return 'dark';
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light';
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(readTheme);
  const shouldReduce = useReducedMotion();

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
    try {
      localStorage.setItem('driftbase-theme', theme);
    } catch {
      /* ignore */
    }
  }, [theme]);

  const toggle = () => setTheme((t) => (t === 'dark' ? 'light' : 'dark'));

  return (
    <motion.button
      type="button"
      onClick={toggle}
      aria-label="Toggle theme"
      whileTap={shouldReduce ? undefined : { scale: 0.92 }}
      transition={{ type: 'spring', stiffness: 500, damping: 40 }}
      className="relative inline-flex h-8 w-8 items-center justify-center overflow-hidden rounded-md border border-[var(--color-border)] text-[var(--color-muted)] transition-colors hover:text-[var(--color-fg)]"
    >
      <AnimatePresence mode="wait" initial={false}>
        <motion.span
          key={theme}
          initial={shouldReduce ? { opacity: 0 } : { rotate: -90, opacity: 0, scale: 0.6 }}
          animate={shouldReduce ? { opacity: 1 } : { rotate: 0, opacity: 1, scale: 1 }}
          exit={shouldReduce ? { opacity: 0 } : { rotate: 90, opacity: 0, scale: 0.6 }}
          transition={shouldReduce ? { duration: 0 } : { duration: 0.22, ease: 'easeOut' }}
          className="inline-flex"
        >
          {theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
        </motion.span>
      </AnimatePresence>
    </motion.button>
  );
}
