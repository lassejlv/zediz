import { useState, type FormEvent, type ReactNode } from 'react';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from '@/components/sheet';
import { Button, ErrorText, Field, Input } from '@/components/ui';
import { useCreateVolume } from '@/lib/volumes';
import { ApiError } from '@/lib/api';

interface Props {
  workspaceSlug: string;
  children: ReactNode;
  onCreated?: (volumeId: string) => void;
}

// Hetzner block volumes support 10–10240 GB. The slider keeps the
// common range tight; the paired number input unlocks the full range.
const SLIDER_MIN = 10;
const SLIDER_MAX = 500;
const HARD_MAX = 10240;
const MONTHLY_PRICE_PER_GB_EUR = 0.044; // Hetzner list price as of 2026.

export function CreateVolumeSheet({ workspaceSlug, children, onCreated }: Props) {
  const create = useCreateVolume(workspaceSlug);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [sizeGb, setSizeGb] = useState(20);
  const [error, setError] = useState<string | null>(null);

  function reset() {
    setName('');
    setSizeGb(20);
    setError(null);
  }

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Name is required');
      return;
    }
    if (!Number.isFinite(sizeGb) || sizeGb < 10 || sizeGb > HARD_MAX) {
      setError(`Size must be between 10 and ${HARD_MAX} GB`);
      return;
    }
    try {
      const created = await create.mutateAsync({ name: trimmed, size_gb: sizeGb });
      reset();
      setOpen(false);
      onCreated?.(created.id);
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : err instanceof Error
            ? err.message
            : 'Failed to create volume',
      );
    }
  }

  const monthlyEur = (sizeGb * MONTHLY_PRICE_PER_GB_EUR).toFixed(2);
  const sliderValue = Math.min(Math.max(sizeGb, SLIDER_MIN), SLIDER_MAX);

  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) reset();
      }}
    >
      <SheetTrigger asChild>{children}</SheetTrigger>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>Create volume</SheetTitle>
          <SheetDescription>
            A Hetzner block volume in this workspace's region. Once created, attach it to a
            service from the service's Volume tab.
          </SheetDescription>
        </SheetHeader>

        <form onSubmit={onSubmit} className="flex flex-1 flex-col gap-5">
          <Field
            label="Name"
            htmlFor="vol-name"
            hint="How it's shown in the UI. No spaces; lowercase letters, digits, and dashes."
          >
            <Input
              id="vol-name"
              autoFocus
              placeholder="data"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>

          <Field
            label="Size"
            htmlFor="vol-size"
            hint={`${SLIDER_MIN}–${HARD_MAX} GB · ~€${monthlyEur}/mo at Hetzner list price.`}
          >
            <div className="space-y-2">
              <input
                id="vol-size"
                type="range"
                min={SLIDER_MIN}
                max={SLIDER_MAX}
                step={10}
                value={sliderValue}
                onChange={(e) => setSizeGb(Number(e.target.value))}
                className="w-full accent-[var(--color-accent)]"
              />
              <div className="flex items-center gap-2">
                <Input
                  type="number"
                  min={10}
                  max={HARD_MAX}
                  step={1}
                  value={sizeGb}
                  onChange={(e) => setSizeGb(Number(e.target.value) || 0)}
                  className="w-28"
                />
                <span className="text-sm text-[var(--color-muted)]">GB</span>
              </div>
            </div>
          </Field>

          {error ? <ErrorText>{error}</ErrorText> : null}

          <div className="mt-auto flex justify-end gap-2 pt-4">
            <Button type="button" variant="secondary" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? 'Creating…' : 'Create volume'}
            </Button>
          </div>
        </form>
      </SheetContent>
    </Sheet>
  );
}
