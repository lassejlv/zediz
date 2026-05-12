import { useMemo, useState } from 'react';
import { createFileRoute, Link } from '@tanstack/react-router';
import { Check } from 'lucide-react';
import { Button, Card, Field, Input } from '@/components/ui';

export const Route = createFileRoute('/pricing')({
  component: PricingPage,
});

const MEMORY_PER_GB_SEC = 0.0000015;
const VCPU_PER_SEC = 0.00000472;
const VOLUME_PER_GB_MONTH = 0.1;
const EGRESS_PER_GB = 0.1;

const SECONDS_PER_MONTH = 24 * 30 * 60 * 60;

type PlanId = 'hobby' | 'pro';

interface Plan {
  id: PlanId;
  name: string;
  fee: number;
  maxMemoryGb: number;
  maxVcpu: number;
  maxVolumeGb: number;
}

const PLANS: Plan[] = [
  { id: 'hobby', name: 'Hobby', fee: 5, maxMemoryGb: 3, maxVcpu: 3, maxVolumeGb: 250 },
  { id: 'pro', name: 'Pro', fee: 20, maxMemoryGb: 32, maxVcpu: 16, maxVolumeGb: 1024 },
];

function formatStorage(gb: number): string {
  if (gb >= 1024 && gb % 1024 === 0) return `${gb / 1024} TB`;
  return `${gb} GB`;
}

function currency(n: number): string {
  return n.toLocaleString('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}

function PricingPage() {
  const [planId, setPlanId] = useState<PlanId>('hobby');
  const plan = PLANS.find((p) => p.id === planId) ?? PLANS[0];

  const [memoryGb, setMemoryGb] = useState(1);
  const [vcpu, setVcpu] = useState(1);
  const [volumeGb, setVolumeGb] = useState(10);
  const [egressGb, setEgressGb] = useState(50);

  const clampedMemory = Math.min(memoryGb, plan.maxMemoryGb);
  const clampedVcpu = Math.min(vcpu, plan.maxVcpu);
  const clampedVolume = Math.min(volumeGb, plan.maxVolumeGb);

  const breakdown = useMemo(() => {
    const memory = clampedMemory * MEMORY_PER_GB_SEC * SECONDS_PER_MONTH;
    const cpu = clampedVcpu * VCPU_PER_SEC * SECONDS_PER_MONTH;
    const volume = clampedVolume * VOLUME_PER_GB_MONTH;
    const egress = egressGb * EGRESS_PER_GB;
    const usage = memory + cpu + volume + egress;
    const total = plan.fee + usage;
    return { memory, cpu, volume, egress, usage, total };
  }, [clampedMemory, clampedVcpu, clampedVolume, egressGb, plan.fee]);

  return (
    <section className="mx-auto max-w-4xl pb-24 pt-10">
      <header className="mb-10 text-center">
        <h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">Pricing</h1>
        <p className="mx-auto mt-3 max-w-md text-sm text-[var(--color-muted)]">
          Pay a small monthly fee plus per-second usage. No hidden costs.
        </p>
      </header>

      <div className="grid gap-6 md:grid-cols-2">
        {PLANS.map((p) => (
          <PlanCard key={p.id} plan={p} highlighted={p.id === 'pro'} />
        ))}
      </div>

      <div className="mt-12">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold tracking-tight">Resource pricing</h2>
          <span className="font-mono text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            same on every plan
          </span>
        </div>
        <p className="mt-1 text-xs text-[var(--color-muted)]">
          Pay only for what you use, billed per second.
        </p>

        <div className="mt-6 grid gap-3 sm:grid-cols-2">
          <RateRow label="Memory" value="$0.0000015 / GB / second" />
          <RateRow label="vCPU" value="$0.00000472 / vCPU / second" />
          <RateRow label="Volume Storage" value="$0.10 / GB / month" />
          <RateRow label="Egress" value="$0.10 / GB" />
        </div>
      </div>

      <div className="mt-12">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold tracking-tight">Estimate cost</h2>
          <span className="font-mono text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            24/7 / month
          </span>
        </div>
        <p className="mt-1 text-xs text-[var(--color-muted)]">
          Based on 720 hours of continuous hosting.
        </p>

        <div className="mt-5 inline-flex rounded-md border border-[var(--color-border)] p-0.5">
          {PLANS.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => setPlanId(p.id)}
              className={[
                'rounded px-3 py-1 text-xs font-medium transition-colors',
                planId === p.id
                  ? 'bg-[var(--color-accent)] text-black'
                  : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
              ].join(' ')}
            >
              {p.name}
            </button>
          ))}
        </div>

        <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Field label="Memory (GB)" htmlFor="mem" hint={`Max ${plan.maxMemoryGb} GB`}>
            <Input
              id="mem"
              type="number"
              min={0}
              max={plan.maxMemoryGb}
              step={0.5}
              value={memoryGb}
              onChange={(e) =>
                setMemoryGb(
                  Math.min(plan.maxMemoryGb, Math.max(0, Number(e.target.value) || 0)),
                )
              }
            />
          </Field>
          <Field label="vCPU" htmlFor="cpu" hint={`Max ${plan.maxVcpu}`}>
            <Input
              id="cpu"
              type="number"
              min={0}
              max={plan.maxVcpu}
              step={0.5}
              value={vcpu}
              onChange={(e) =>
                setVcpu(Math.min(plan.maxVcpu, Math.max(0, Number(e.target.value) || 0)))
              }
            />
          </Field>
          <Field label="Volume (GB)" htmlFor="vol" hint={`Max ${formatStorage(plan.maxVolumeGb)}`}>
            <Input
              id="vol"
              type="number"
              min={0}
              max={plan.maxVolumeGb}
              step={1}
              value={volumeGb}
              onChange={(e) =>
                setVolumeGb(
                  Math.min(plan.maxVolumeGb, Math.max(0, Number(e.target.value) || 0)),
                )
              }
            />
          </Field>
          <Field label="Egress (GB)" htmlFor="egress">
            <Input
              id="egress"
              type="number"
              min={0}
              step={1}
              value={egressGb}
              onChange={(e) => setEgressGb(Math.max(0, Number(e.target.value) || 0))}
            />
          </Field>
        </div>

        <div className="mt-6 space-y-2 border-t border-[var(--color-border)] pt-4 text-sm">
          <Row label={`${plan.name} plan fee`} value={currency(plan.fee)} />
          <Row label={`Memory (${clampedMemory} GB)`} value={currency(breakdown.memory)} />
          <Row label={`vCPU (${clampedVcpu})`} value={currency(breakdown.cpu)} />
          <Row label={`Volume (${clampedVolume} GB)`} value={currency(breakdown.volume)} />
          <Row label={`Egress (${egressGb} GB)`} value={currency(breakdown.egress)} />
        </div>

        <div className="mt-4 flex items-baseline justify-between border-t border-[var(--color-border)] pt-4">
          <span className="text-xs uppercase tracking-wider text-[var(--color-muted)]">
            Estimated total
          </span>
          <span className="font-mono text-2xl font-semibold tracking-tight">
            {currency(breakdown.total)}
            <span className="ml-1 text-xs font-normal text-[var(--color-muted)]">/ mo</span>
          </span>
        </div>
      </div>

      <p className="mt-10 text-center text-xs text-[var(--color-muted)]">
        Usage billed per second; figures shown assume 24/7 uptime over 30 days.
      </p>
    </section>
  );
}

function PlanCard({ plan, highlighted }: { plan: Plan; highlighted?: boolean }) {
  return (
    <Card
      className={[
        'p-6',
        highlighted ? 'border-[var(--color-accent)]/60 ring-1 ring-[var(--color-accent)]/30' : '',
      ].join(' ')}
    >
      <h2 className="text-lg font-semibold tracking-tight">{plan.name}</h2>

      <div className="mt-3 flex items-baseline gap-1">
        <span className="text-4xl font-semibold tracking-tight">${plan.fee}</span>
        <span className="text-sm text-[var(--color-muted)]">/ month</span>
      </div>
      <p className="mt-1 text-xs text-[var(--color-muted)]">+ usage below</p>

      <ul className="mt-6 space-y-3 text-sm">
        <Line label="Plan fee" value={`$${plan.fee} / month`} />
        <Line label="Max memory" value={`${plan.maxMemoryGb} GB`} />
        <Line label="Max vCPU" value={`${plan.maxVcpu}`} />
        <Line label="Max volume" value={formatStorage(plan.maxVolumeGb)} />
        <Line label="Usage rates" value="see below" />
      </ul>

      <div className="mt-8">
        <Link to="/signup">
          <Button className="w-full" variant={highlighted ? 'primary' : 'secondary'}>
            Get started
          </Button>
        </Link>
      </div>
    </Card>
  );
}

function Line({ label, value }: { label: string; value: string }) {
  return (
    <li className="flex items-start gap-2">
      <Check className="mt-0.5 h-4 w-4 shrink-0 text-[var(--color-accent)]" />
      <div className="flex w-full items-baseline justify-between gap-3">
        <span className="text-[var(--color-fg)]">{label}</span>
        <span className="font-mono text-xs text-[var(--color-muted)]">{value}</span>
      </div>
    </li>
  );
}

function RateRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between rounded-md border border-[var(--color-border)] px-3 py-2 text-sm">
      <span className="text-[var(--color-fg)]">{label}</span>
      <span className="font-mono text-xs text-[var(--color-muted)]">{value}</span>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between">
      <span className="text-[var(--color-muted)]">{label}</span>
      <span className="font-mono text-[var(--color-fg)]">{value}</span>
    </div>
  );
}
