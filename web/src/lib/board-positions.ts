import { useCallback, useEffect, useState } from 'react';

export interface BoardPosition {
  x: number;
  y: number;
}

interface BoardPositionsBlob {
  version: 1;
  positions: Record<string, BoardPosition>;
}

const SNAP = 16;

function storageKey(workspaceSlug: string, projectSlug: string): string {
  return `zediz:board:${workspaceSlug}:${projectSlug}:positions`;
}

function readPositions(key: string): Record<string, BoardPosition> {
  if (typeof window === 'undefined') return {};
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as BoardPositionsBlob;
    if (parsed.version !== 1 || !parsed.positions) return {};
    return parsed.positions;
  } catch {
    return {};
  }
}

function writePositions(key: string, positions: Record<string, BoardPosition>) {
  try {
    const blob: BoardPositionsBlob = { version: 1, positions };
    localStorage.setItem(key, JSON.stringify(blob));
  } catch {
    // ignore quota/serialization errors
  }
}

export function useBoardPositions(workspaceSlug: string, projectSlug: string) {
  const key = storageKey(workspaceSlug, projectSlug);
  const [positions, setPositions] = useState<Record<string, BoardPosition>>(() =>
    readPositions(key),
  );

  useEffect(() => {
    setPositions(readPositions(key));
  }, [key]);

  const setPosition = useCallback(
    (id: string, pos: BoardPosition) => {
      setPositions((prev) => {
        const snapped = {
          x: Math.max(0, Math.round(pos.x / SNAP) * SNAP),
          y: Math.max(0, Math.round(pos.y / SNAP) * SNAP),
        };
        const next = { ...prev, [id]: snapped };
        writePositions(key, next);
        return next;
      });
    },
    [key],
  );

  const clear = useCallback(() => {
    setPositions({});
    try {
      localStorage.removeItem(key);
    } catch {
      // ignore
    }
  }, [key]);

  return { positions, setPosition, clear, hasCustomLayout: Object.keys(positions).length > 0 };
}
