'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { Server, Container, SquareTerminal } from 'lucide-react';
import { EASE, VIEWPORT, Reveal } from '../motion';
import { BenchToggle, CountUpStat, BenchInfoTooltip } from './benchUI';
import { benchWorkloads, SANDBOX_COST_PROVIDER, BENCHMARK_DATE, type WorkloadKey } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Cost-per-execution-second, told as a density story. One server packs N
// concurrent executions (N = executions that fit per server at 70% utilization,
// from bench.ts) while a sandbox holds exactly one. The packed count is the
// denominator of the cost, so the packing animation resolves into the price:
//   one $X/hr server / N executions = $Y per execution-second.
// Laid out as a full-width horizontal card. Driven by the shared workload toggle
// plus a local hardware-tier toggle.
// ---------------------------------------------------------------------------

const WORKLOAD_KEYS = Object.keys(benchWorkloads) as WorkloadKey[];
const AGENT_LOGOS = [
	'/images/agent-logos/pi.svg',
	'/images/agent-logos/claude-code.svg',
	'/images/agent-logos/codex.svg',
	'/images/agent-logos/opencode.svg',
	'/images/agent-logos/amp.svg',
];
const CHIP_CAP = 48; // cap rendered chips; the rest fold into a "+N more" pill

// Smoothly animates its own height to match its content. A ResizeObserver tracks
// the inner element's natural height (unaffected by child transforms), so when
// the packed-chip count changes the card grows/shrinks instead of jumping. The
// inner element keeps its real size — nothing is scaled — so no text distortion.
function AutoHeight({ children }: { children: ReactNode }) {
	const reduced = useReducedMotion();
	const innerRef = useRef<HTMLDivElement>(null);
	const [height, setHeight] = useState<number | 'auto'>('auto');

	useEffect(() => {
		const el = innerRef.current;
		if (!el) return;
		const update = () => setHeight(el.offsetHeight);
		update();
		const ro = new ResizeObserver(update);
		ro.observe(el);
		return () => ro.disconnect();
	}, []);

	return (
		<motion.div
			initial={false}
			animate={{ height }}
			transition={reduced ? { duration: 0 } : { duration: 0.4, ease: [...EASE] }}
			style={{ overflow: 'hidden' }}
		>
			<div ref={innerRef}>{children}</div>
		</motion.div>
	);
}

export const ExecutionDensity = ({ workload, onWorkloadChange }: { workload: WorkloadKey; onWorkloadChange: (w: WorkloadKey) => void }) => {
	const reduced = useReducedMotion();
	const [tierIdx, setTierIdx] = useState(0); // AWS ARM default
	const [inView, setInView] = useState(false);

	const wl = benchWorkloads[workload];
	const tier = wl.cost[tierIdx];
	const execs = tier.execs;
	const [mult, verb] = tier.multiplier.split(' '); // ['171x', 'cheaper']
	const shown = Math.min(execs, CHIP_CAP);
	const overflow = execs - shown;
	const activeIdx = WORKLOAD_KEYS.indexOf(workload);

	if (overflow > 0 && import.meta.env.DEV) {
		console.info(`[ExecutionDensity] ${execs} execs (${workload}/${tier.label}); rendering ${shown} chips +${overflow} more (cap=${CHIP_CAP}).`);
	}

	const chipGlyph = (i: number) =>
		workload === 'agent' ? (
			<img src={AGENT_LOGOS[(i * 7) % AGENT_LOGOS.length]} alt='' aria-hidden='true' className='h-4 w-4 object-contain' />
		) : (
			<SquareTerminal className='h-4 w-4 text-ink-soft' aria-hidden='true' />
		);

	return (
		<Reveal>
			<motion.div
				className='flex flex-col rounded-2xl border border-ink/10 bg-white/55 p-6 md:p-7'
				onViewportEnter={() => setInView(true)}
				viewport={VIEWPORT}
			>
				{/* Header: eyebrow + workload toggle */}
				<div className='flex flex-wrap items-start justify-between gap-3'>
					<span className='font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-ink-faint'>Cost Per Execution-Second</span>
					<div className='w-64 max-sm:w-full'>
						<BenchToggle
							options={WORKLOAD_KEYS.map((k) => benchWorkloads[k].label)}
							active={activeIdx}
							onChange={(i) => onWorkloadChange(WORKLOAD_KEYS[i])}
						/>
					</div>
				</div>

				{/* Body: copy + ledger on the left, the packing visual on the right */}
				<div className='mt-6 grid items-center gap-8 lg:grid-cols-2 lg:gap-12'>
					<div>
						{/* Headline multiplier */}
						<div className='flex items-baseline gap-2'>
							<span className='font-sans text-[2.75rem] font-medium leading-[1.0] tracking-[-0.02em] tabular-nums text-ink md:text-5xl'>
								<CountUpStat text={mult} active={inView} />
							</span>
							<span className='font-sans text-lg font-medium text-ink-faint md:text-xl'>{verb}</span>
						</div>

						{/* Hardware-tier toggle */}
						<div className='mt-5'>
							<BenchToggle options={wl.cost.map((t) => t.label)} active={tierIdx} onChange={setTierIdx} />
						</div>

						{/* Equation strip — the resolve to cost */}
						<p className='mt-4 font-mono text-[11px] leading-relaxed text-ink-faint'>
							${tier.costPerHour.toFixed(4)}/hr server &divide; <span className='text-accent-deep'>{execs} executions</span> = {tier.value} per execution-second
						</p>

						{/* Comparison ledger */}
						<div className='mt-4 divide-y divide-ink/10 border-y border-ink/10'>
							<div className='flex items-baseline justify-between gap-4 py-2.5'>
								<span className='inline-flex min-w-0 items-baseline font-mono text-[13px] font-medium text-ink'>
									agentOS
									<BenchInfoTooltip>
										<strong>What&apos;s measured:</strong>{' '}
										<code className='rounded bg-ink/10 px-1 py-0.5 text-[10px]'>server price per second / concurrent executions per server</code>
										<br /><br />
										<strong>Why it&apos;s cheaper:</strong> Each execution uses {wl.memory.agentOS} instead of a {wl.memory.sandbox} sandbox minimum. And you run on your own hardware, which is significantly cheaper than per-second sandbox billing.
										<br /><br />
										<strong>Sandbox baseline:</strong> {SANDBOX_COST_PROVIDER}, the cheapest mainstream sandbox provider as of {BENCHMARK_DATE}. Default sandbox: 1 vCPU + 1 GiB RAM at $0.0504/vCPU-h + $0.0162/GiB-h.
										<br /><br />
										<strong>agentOS:</strong> {wl.memory.agentOS} baseline per execution, assuming 70% utilization (industry-standard HPA scaling threshold). Select a hardware tier above to compare.
									</BenchInfoTooltip>
								</span>
								<span className='whitespace-nowrap font-mono text-[15px] font-medium tabular-nums text-accent-deep'>
									<CountUpStat text={tier.value} active={inView} />
								</span>
							</div>
							<div className='flex items-baseline justify-between gap-4 py-2.5'>
								<span className='font-mono text-[13px] text-ink-faint'>Cheapest sandbox</span>
								<span className='whitespace-nowrap font-mono text-[15px] tabular-nums text-ink-faint'>{wl.sandboxCost}</span>
							</div>
						</div>

						<p className='mt-4 font-mono text-[10px] leading-relaxed text-ink-faint'>Assumes one agent per sandbox, needed for isolation.</p>
					</div>

					{/* Visual: one server packs N executions vs one per sandbox */}
					<div className='flex flex-col gap-3'>
						<div className='rounded-xl border border-accent/25 bg-accent/[0.04] p-4'>
							<div className='mb-3 flex items-center justify-between gap-3'>
								<span className='inline-flex items-center gap-2 text-sm font-medium text-ink'>
									<Server className='h-4 w-4 text-accent-deep' aria-hidden='true' /> One server &mdash; {tier.label}
								</span>
								<span className='font-mono text-[11px] tabular-nums text-accent-deep'>{execs} executions</span>
							</div>
							<AutoHeight>
								<motion.div
									key={`${workload}-${tier.label}`}
									className='flex flex-wrap gap-1.5'
									initial='hidden'
									animate={inView ? 'visible' : 'hidden'}
									variants={{ hidden: {}, visible: { transition: { staggerChildren: reduced ? 0 : 0.018 } } }}
								>
									{Array.from({ length: shown }).map((_, i) => (
										<motion.span
											key={i}
											variants={{
												hidden: reduced ? { opacity: 0 } : { opacity: 0, scale: 0.6 },
												visible: { opacity: 1, scale: 1, transition: { duration: reduced ? 0 : 0.22, ease: [...EASE] } },
											}}
											className='flex h-8 w-8 items-center justify-center rounded-md bg-white/80 ring-1 ring-ink/10'
										>
											{chipGlyph(i)}
										</motion.span>
									))}
									{overflow > 0 && (
										<span className='flex h-8 items-center rounded-md bg-accent/10 px-2 font-mono text-[10px] text-accent-deep'>+{overflow} more</span>
									)}
								</motion.div>
							</AutoHeight>
						</div>

						{/* One sandbox: a full 1 GB reservation that's mostly idle. The bar's
						    full width is the 1 GB always reserved; the solid sliver is what the
						    one execution actually uses (memory ratio), the rest is reserved-
						    and-billed dead weight — the fixed spend agentOS avoids by sharing
						    the host and only paying for what each execution needs. */}
						<div className='rounded-xl border border-dashed border-ink/15 bg-ink/[0.03] p-4'>
							<div className='mb-3 flex items-center justify-between gap-3'>
								<span className='inline-flex items-center gap-2 text-sm text-ink-soft'>
									<Container className='h-4 w-4 text-ink-faint' aria-hidden='true' /> One per sandbox
								</span>
								<span className='font-mono text-[11px] tabular-nums text-ink-faint'>1 execution</span>
							</div>
							{/* reservation bar — full width = 1 GB always reserved */}
							<div className='relative h-8 w-full overflow-hidden rounded-md border border-dashed border-ink/20 bg-ink/[0.02]'>
								<motion.div
									className='absolute inset-y-0 left-0 min-w-[3px] border-r border-ink/30 bg-ink/25'
									initial={{ width: reduced ? `${wl.memory.agentOSBar}%` : '0%' }}
									animate={{ width: inView ? `${wl.memory.agentOSBar}%` : '0%' }}
									transition={{ duration: reduced ? 0 : 0.6, ease: [...EASE] }}
								/>
								<span className='absolute inset-y-0 right-2.5 flex items-center font-mono text-[10px] uppercase tracking-wide text-ink-faint'>idle &middot; reserved</span>
							</div>
							<p className='mt-2 font-mono text-[10px] leading-relaxed text-ink-faint'>
								{wl.memory.agentOS} used of <span className='text-ink-soft'>~1 GB reserved</span> &mdash; the rest is billed while idle.
							</p>
						</div>
					</div>
				</div>
			</motion.div>
		</Reveal>
	);
};
