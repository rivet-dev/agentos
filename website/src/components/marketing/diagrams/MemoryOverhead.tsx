'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { useState } from 'react';
import { ArrowDown } from 'lucide-react';
import { EASE, VIEWPORT, Reveal } from '../motion';
import { BenchToggle, CountUpStat, BenchInfoTooltip } from './benchUI';
import { benchWorkloads, SANDBOX_COST_PROVIDER, BENCHMARK_DATE, type WorkloadKey } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Memory-per-instance comparison. A full-height sandbox column (1 GiB reserved)
// sits next to an Agent OS column whose fill is a sliver of that reservation.
// Toggling the workload (coding agent ~131 MB <-> execution ~22 MB) GROWS and
// SHRINKS the Agent OS fill — the floating MB caption rides its top edge and the
// headline multiplier (8x <-> 47x) re-counts. Numbers come from bench.ts.
// ---------------------------------------------------------------------------

const WORKLOAD_KEYS = Object.keys(benchWorkloads) as WorkloadKey[];

const Row = ({ label, value, highlight }: { label: React.ReactNode; value: string; highlight?: boolean }) => (
	<div className='flex items-baseline justify-between gap-4 py-2.5'>
		<span className={`inline-flex min-w-0 items-baseline font-mono text-[13px] ${highlight ? 'font-medium text-ink' : 'font-normal text-ink-faint'}`}>
			{label}
		</span>
		<span className={`whitespace-nowrap font-mono text-[15px] tabular-nums ${highlight ? 'font-medium text-accent-deep' : 'font-normal text-ink-faint'}`}>
			{value}
		</span>
	</div>
);

export function MemoryOverhead({ workload, onWorkloadChange }: { workload: WorkloadKey; onWorkloadChange: (w: WorkloadKey) => void }) {
	const reduced = useReducedMotion();
	const [inView, setInView] = useState(false);

	const mem = benchWorkloads[workload].memory;
	const [mult, verb] = mem.multiplier.split(' '); // ['8x', 'smaller']
	const targetH = `${mem.agentOSBar}%`; // '12.8%' (agent) | '2.1%' (execution)
	const activeIdx = WORKLOAD_KEYS.indexOf(workload);

	return (
		<Reveal>
			<motion.div
				className='flex h-full flex-col rounded-2xl border border-ink/10 bg-white/55 p-6 md:p-7'
				onViewportEnter={() => setInView(true)}
				viewport={VIEWPORT}
			>
				{/* Header: eyebrow + workload toggle */}
				<div className='flex flex-wrap items-start justify-between gap-3'>
					<div className='flex flex-col gap-1'>
						<span className='font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-ink-faint'>Memory Per Instance</span>
						<span className='inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-[0.12em] text-ink-faint'>
							<ArrowDown className='h-3 w-3' /> lower is better
						</span>
					</div>
					<div className='w-64 max-sm:w-full'>
						<BenchToggle
							options={WORKLOAD_KEYS.map((k) => benchWorkloads[k].label)}
							active={activeIdx}
							onChange={(i) => onWorkloadChange(WORKLOAD_KEYS[i])}
						/>
					</div>
				</div>

				{/* Headline multiplier */}
				<div className='mt-5 flex items-baseline gap-2'>
					<span className='font-sans text-[2.75rem] font-medium leading-[1.0] tracking-[-0.02em] tabular-nums text-ink md:text-5xl'>
						<CountUpStat text={mult} active={inView} />
					</span>
					<span className='font-sans text-lg font-medium text-ink-faint md:text-xl'>{verb}</span>
				</div>

				{/* Animated stage: two memory columns */}
				<div className='mt-7 flex h-56 items-end gap-6 max-sm:h-44'>
					{/* Sandbox — full reservation, static */}
					<div className='flex h-full flex-1 flex-col items-center'>
						<div className='relative h-full w-full overflow-hidden rounded-lg border border-dashed border-ink/15 bg-ink/[0.04]'>
							<span className='absolute inset-x-0 top-2 text-center font-mono text-[9px] uppercase tracking-wide text-ink-faint'>reserved</span>
						</div>
						<span className='mt-2 font-mono text-[13px] tabular-nums text-ink-soft'>{mem.sandbox}</span>
						<span className='font-mono text-[9px] uppercase tracking-wide text-ink-faint'>reserved / instance</span>
					</div>

					{/* Agent OS — animated fill */}
					<div className='flex h-full flex-1 flex-col items-center'>
						<div className='relative h-full w-full overflow-hidden rounded-lg border border-accent/30 bg-accent/[0.03]'>
							{/* floating caption riding the top of the fill */}
							<motion.div
								className='absolute inset-x-0 z-10 -translate-y-1/2'
								initial={{ bottom: reduced ? targetH : '0%' }}
								animate={{ bottom: inView ? targetH : '0%' }}
								transition={{ duration: reduced ? 0 : 0.6, ease: [...EASE] }}
							>
								<span className='mx-auto block w-fit rounded bg-accent px-1.5 py-0.5 font-mono text-[10px] font-semibold tabular-nums text-cream'>
									<CountUpStat text={mem.agentOS} active={inView} />
								</span>
							</motion.div>
							{/* the fill — height is the only animated property */}
							<motion.div
								className='absolute inset-x-0 bottom-0 min-h-[3px] overflow-hidden rounded-b-[7px] bg-accent'
								initial={{ height: reduced ? targetH : '0%' }}
								animate={{ height: inView ? targetH : '0%' }}
								transition={{ duration: reduced ? 0 : 0.6, ease: [...EASE] }}
							>
								{/* stacked-instance bands */}
								<div className='absolute inset-0 [background-image:repeating-linear-gradient(0deg,rgba(244,241,231,0.18)_0,rgba(244,241,231,0.18)_1px,transparent_1px,transparent_8px)]' />
								<div className='absolute inset-x-0 top-0 h-px bg-cream/50' />
							</motion.div>
						</div>
						<span className='mt-2 font-mono text-[13px] font-medium tabular-nums text-accent-deep'>
							<CountUpStat text={mem.agentOS} active={inView} />
						</span>
						<span className='font-mono text-[9px] uppercase tracking-wide text-accent/80'>used / instance</span>
					</div>
				</div>

				{/* Comparison ledger */}
				<div className='mb-6 mt-7 divide-y divide-ink/10 border-y border-ink/10'>
					<Row
						highlight
						label={
							<>
								Agent OS
								<BenchInfoTooltip>
									<strong>What&apos;s measured:</strong> Memory footprint added per concurrent execution.
									<br /><br />
									<strong>Why the gap:</strong> In-process isolates share the host&apos;s memory. Each additional execution only adds its own heap and stack. Sandboxes allocate a dedicated environment with a minimum memory reservation, even if the code inside uses far less.
									<br /><br />
									<strong>Sandbox baseline:</strong> {SANDBOX_COST_PROVIDER}, the cheapest mainstream sandbox provider as of {BENCHMARK_DATE}. Default sandbox: 1 vCPU + 1 GiB RAM.
									<br /><br />
									<strong>Agent OS:</strong> {workload === 'agent' ? `${benchWorkloads.agent.memory.agentOS} for a full Pi coding agent session with MCP servers and file system mounts.` : `${benchWorkloads.shell.memory.agentOS} for the minimal shell workload under sustained load.`}
								</BenchInfoTooltip>
							</>
						}
						value={mem.agentOS}
					/>
					<Row label='Cheapest sandbox' value={mem.sandbox} />
				</div>

				<p className='mt-auto font-mono text-[10px] leading-relaxed text-ink-faint'>Sandboxes reserve idle RAM per agent; Agent OS isolates share the host.</p>
			</motion.div>
		</Reveal>
	);
}
