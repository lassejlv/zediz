import { useRef, type PointerEvent } from 'react';
import { ServiceNode, type ServiceNodeState } from './service-node';
import type { BoardPosition } from '@/lib/board-positions';

interface Props {
  nodes: ServiceNodeState[];
  selectedSlug: string | null;
  onSelect: (slug: string | null) => void;
  empty?: React.ReactNode;
  positions: Record<string, BoardPosition>;
  onPositionChange: (id: string, pos: BoardPosition) => void;
  hasCustomLayout: boolean;
}

const NODE_W = 280;
const NODE_H = 120;
const GAP = 16;
const DRAG_THRESHOLD = 4;

function defaultPosition(index: number, columns: number): BoardPosition {
  const col = index % columns;
  const row = Math.floor(index / columns);
  return { x: col * (NODE_W + GAP) + GAP, y: row * (NODE_H + GAP) + GAP };
}

export function BoardCanvas({
  nodes,
  selectedSlug,
  onSelect,
  empty,
  positions,
  onPositionChange,
  hasCustomLayout,
}: Props) {
  const surfaceRef = useRef<HTMLDivElement | null>(null);

  if (nodes.length === 0) {
    return (
      <div className="flex h-full min-h-[400px] items-center justify-center">{empty}</div>
    );
  }

  const columns = 3;
  const resolved = nodes.map((n, i) => ({
    node: n,
    pos: positions[n.service.id] ?? defaultPosition(i, columns),
  }));
  const maxBottom = Math.max(
    ...resolved.map((r) => r.pos.y + NODE_H + GAP),
    NODE_H + GAP * 2,
  );
  const maxRight = Math.max(
    ...resolved.map((r) => r.pos.x + NODE_W + GAP),
    columns * (NODE_W + GAP) + GAP,
  );

  return (
    <div
      ref={surfaceRef}
      onPointerDown={(e) => {
        if (e.target === surfaceRef.current) onSelect(null);
      }}
      className={[
        'relative min-h-full select-none',
        hasCustomLayout ? 'zediz-board-grid' : '',
      ].join(' ')}
      style={{
        minHeight: `${maxBottom}px`,
        minWidth: `${maxRight}px`,
        backgroundImage: hasCustomLayout
          ? 'radial-gradient(circle, color-mix(in oklch, currentColor 10%, transparent) 1px, transparent 1px)'
          : undefined,
        backgroundSize: hasCustomLayout ? `${GAP}px ${GAP}px` : undefined,
      }}
    >
      {resolved.map(({ node, pos }) => (
        <DraggableNode
          key={node.service.id}
          state={node}
          selected={selectedSlug === node.service.slug}
          pos={pos}
          onSelect={() => onSelect(node.service.slug)}
          onPositionChange={(p) => onPositionChange(node.service.id, p)}
          surfaceRef={surfaceRef}
        />
      ))}
    </div>
  );
}

function DraggableNode({
  state,
  selected,
  pos,
  onSelect,
  onPositionChange,
  surfaceRef,
}: {
  state: ServiceNodeState;
  selected: boolean;
  pos: BoardPosition;
  onSelect: () => void;
  onPositionChange: (pos: BoardPosition) => void;
  surfaceRef: React.MutableRefObject<HTMLDivElement | null>;
}) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const dragState = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    origX: number;
    origY: number;
    moved: boolean;
  } | null>(null);
  const livePos = useRef<BoardPosition>(pos);

  function setTransform(p: BoardPosition) {
    const el = wrapperRef.current;
    if (!el) return;
    el.style.left = `${p.x}px`;
    el.style.top = `${p.y}px`;
  }

  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    (e.target as Element).setPointerCapture?.(e.pointerId);
    dragState.current = {
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      origX: pos.x,
      origY: pos.y,
      moved: false,
    };
    livePos.current = pos;
  }

  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    const d = dragState.current;
    if (!d || d.pointerId !== e.pointerId) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    if (!d.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    d.moved = true;
    const surface = surfaceRef.current;
    const bounds = surface?.getBoundingClientRect();
    const maxX = bounds ? Math.max(0, bounds.width - NODE_W) : Infinity;
    const next = {
      x: Math.max(0, Math.min(maxX, d.origX + dx)),
      y: Math.max(0, d.origY + dy),
    };
    livePos.current = next;
    setTransform(next);
  }

  function onPointerUp(e: PointerEvent<HTMLDivElement>) {
    const d = dragState.current;
    if (!d || d.pointerId !== e.pointerId) return;
    dragState.current = null;
    if (d.moved) {
      onPositionChange(livePos.current);
    } else {
      onSelect();
    }
  }

  return (
    <div
      ref={wrapperRef}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      className="absolute touch-none"
      style={{ left: pos.x, top: pos.y, width: NODE_W, height: NODE_H }}
    >
      <ServiceNode state={state} selected={selected} />
    </div>
  );
}
