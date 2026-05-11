import { useEffect, useRef, useState } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { Button, Card, StatusPill, type SemanticStatus } from '@/components/ui';

type ConnState = 'connecting' | 'open' | 'closed' | 'error';

export function ServiceConsole({ deploymentId }: { deploymentId: string }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [connState, setConnState] = useState<ConnState>('connecting');
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [reconnectKey, setReconnectKey] = useState(0);

  useEffect(() => {
    const host = containerRef.current;
    if (!host) return;

    setErrorMsg(null);
    setConnState('connecting');

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
      theme: {
        background: '#000000',
        foreground: '#e5e5e5',
        cursor: '#e5e5e5',
      },
      convertEol: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    try {
      fit.fit();
    } catch {
      // proposeDimensions can throw before layout; ignore
    }

    const dims = fit.proposeDimensions();
    const initialCols = dims?.cols ?? 80;
    const initialRows = dims?.rows ?? 24;

    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${proto}//${window.location.host}/api/v1/deployments/${encodeURIComponent(
      deploymentId,
    )}/console/ws?cols=${initialCols}&rows=${initialRows}`;
    const ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';

    term.writeln('\x1b[2mConnecting to container…\x1b[0m');

    const encoder = new TextEncoder();
    const onData = (data: string) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(encoder.encode(data));
      }
    };
    const dataSub = term.onData(onData);

    ws.onopen = () => {
      setConnState('open');
      term.focus();
    };
    ws.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(e.data));
      } else if (typeof e.data === 'string') {
        try {
          const v = JSON.parse(e.data);
          if (v && v.type === 'error' && typeof v.message === 'string') {
            setErrorMsg(v.message);
          }
        } catch {
          // ignore non-JSON text frames
        }
      }
    };
    ws.onerror = () => setConnState('error');
    ws.onclose = (ev) => {
      setConnState('closed');
      if (!errorMsg && ev.reason) setErrorMsg(ev.reason);
    };

    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    const sendResize = () => {
      try {
        fit.fit();
      } catch {
        return;
      }
      const d = fit.proposeDimensions();
      if (!d) return;
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'resize', cols: d.cols, rows: d.rows }));
      }
    };
    const ro = new ResizeObserver(() => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(sendResize, 100);
    });
    ro.observe(host);

    return () => {
      ro.disconnect();
      if (resizeTimer) clearTimeout(resizeTimer);
      dataSub.dispose();
      try {
        ws.close();
      } catch {
        // ignore
      }
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deploymentId, reconnectKey]);

  const tone: SemanticStatus =
    connState === 'open' ? 'ok' : connState === 'error' ? 'error' : 'muted';

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-xs">
        <span className="text-[var(--color-muted)]">
          Interactive shell · admin-only · sessions are not recorded
        </span>
        <div className="flex items-center gap-3">
          {connState === 'closed' || connState === 'error' ? (
            <Button
              variant="secondary"
              onClick={() => setReconnectKey((k) => k + 1)}
            >
              Reconnect
            </Button>
          ) : null}
          <StatusPill
            status={tone}
            label={connState}
            pulse={connState === 'connecting'}
          />
        </div>
      </div>
      {errorMsg ? (
        <div className="border-b border-[var(--color-border)] bg-red-950/30 px-4 py-2 text-xs text-red-300">
          {errorMsg}
        </div>
      ) : null}
      <div
        ref={containerRef}
        className="h-[32rem] bg-black p-2"
      />
    </Card>
  );
}
