'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { useState } from 'react';
import { VIEWPORT, Reveal } from '../motion';
import { BenchToggle, CountUpStat, BenchInfoTooltip } from './benchUI';
import { benchWorkloads, SANDBOX_COST_PROVIDER, BENCHMARK_DATE, type WorkloadKey } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Memory-per-instance comparison, drawn as two side-by-side squares. A full
// reservation square (1 GiB sandbox) sits next to a smaller agentOS square whose
// AREA is proportional to the memory ratio — so the visible size gap equals the
// real reduction (131 MB -> ~13% area / 8x; 22 MB -> ~2% area / 47x). Toggling
// the workload (coding agent <-> execution) GROWS and SHRINKS the agentOS
// square; the floating MB caption rides its top edge and the headline multiplier
// re-counts. Laid out as a full-width horizontal card. Numbers from bench.ts.
// ---------------------------------------------------------------------------

const WORKLOAD_KEYS = Object.keys(benchWorkloads) as WorkloadKey[];

// Ease-in-out (easeInOutCubic) so the agentOS square grows/shrinks on an S curve:
// slow start, quick middle, gentle settle.
const S_CURVE = [0.65, 0, 0.35, 1] as const;

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
	// agentOSBar is the memory ratio as a percentage of the reservation, i.e. the
	// target AREA of the inner square. Side = sqrt(area) keeps area proportional
	// to memory: '35.8%' side (agent, ~13% area) | '14.5%' side (execution, ~2%).
	const targetSide = `${(Math.sqrt(mem.agentOSBar / 100) * 100).toFixed(1)}%`;
	const activeIdx = WORKLOAD_KEYS.indexOf(workload);

	return (
		<Reveal>
			<motion.div
				className='flex flex-col rounded-2xl border border-ink/10 bg-white/55 p-6 md:p-7'
				onViewportEnter={() => setInView(true)}
				viewport={VIEWPORT}
			>
				{/* Header: eyebrow + workload toggle */}
				<div className='flex flex-wrap items-start justify-between gap-3'>
					<span className='font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-ink-faint'>Memory Per Instance</span>
					<div className='w-64 max-sm:w-full'>
						<BenchToggle
							options={WORKLOAD_KEYS.map((k) => benchWorkloads[k].label)}
							active={activeIdx}
							onChange={(i) => onWorkloadChange(WORKLOAD_KEYS[i])}
						/>
					</div>
				</div>

				{/* Body: copy + ledger on the left, the squares visual on the right */}
				<div className='mt-6 grid items-center gap-8 lg:grid-cols-2 lg:gap-12'>
					<div>
						{/* Headline multiplier */}
						<div className='flex items-baseline gap-2'>
							<span className='font-sans text-[2.75rem] font-medium leading-[1.0] tracking-[-0.02em] tabular-nums text-ink md:text-5xl'>
								<CountUpStat text={mult} active={inView} />
							</span>
							<span className='font-sans text-lg font-medium text-ink-faint md:text-xl'>{verb}</span>
						</div>

						{/* Comparison ledger */}
						<div className='mt-6 divide-y divide-ink/10 border-y border-ink/10'>
							<Row
								highlight
								label={
									<>
										agentOS
										<BenchInfoTooltip>
											<strong>What&apos;s measured:</strong> Memory footprint added per concurrent execution.
											<br /><br />
											<strong>Why the gap:</strong> In-process isolates share the host&apos;s memory. Each additional execution only adds its own heap and stack. Sandboxes allocate a dedicated environment with a minimum memory reservation, even if the code inside uses far less.
											<br /><br />
											<strong>Sandbox baseline:</strong> {SANDBOX_COST_PROVIDER}, the cheapest mainstream sandbox provider as of {BENCHMARK_DATE}. Default sandbox: 1 vCPU + 1 GiB RAM.
											<br /><br />
											<strong>agentOS:</strong> {workload === 'agent' ? `${benchWorkloads.agent.memory.agentOS} for a full Pi coding agent session with MCP servers and file system mounts.` : `${benchWorkloads.shell.memory.agentOS} for the minimal shell workload under sustained load.`}
										</BenchInfoTooltip>
									</>
								}
								value={mem.agentOS}
							/>
							<Row label='Cheapest sandbox' value={mem.sandbox} />
						</div>

						<p className='mt-4 font-mono text-[10px] leading-relaxed text-ink-faint'>Sandboxes reserve idle RAM per agent; agentOS isolates share the host.</p>
					</div>

					{/* Visual: two squares, bottom-aligned, area ∝ memory */}
					<div className='flex items-end justify-center gap-8 max-sm:gap-6'>
						{/* Sandbox — full reservation square, static */}
						<div className='flex flex-col items-center gap-2.5'>
							<div className='relative h-44 w-44 max-sm:h-36 max-sm:w-36 overflow-hidden rounded-xl border border-dashed border-ink/15 bg-ink/[0.04]'>
								<span className='absolute inset-0 flex items-center justify-center font-mono text-[13px] tabular-nums text-ink-soft'>{mem.sandbox}</span>
							</div>
							<span className='font-mono text-[11px] font-medium text-ink-soft'>Sandbox</span>
						</div>

						{/* agentOS — proportional square, animated */}
						<div className='flex flex-col items-center gap-2.5'>
							<div className='relative flex h-44 w-44 items-end justify-center max-sm:h-36 max-sm:w-36'>
								<motion.div
									className='absolute inset-x-0 z-10 -translate-y-1/2'
									initial={{ bottom: reduced ? targetSide : '0%' }}
									animate={{ bottom: inView ? targetSide : '0%' }}
									transition={{ duration: reduced ? 0 : 0.6, ease: [...S_CURVE] }}
								>
									<span className='mx-auto block w-fit whitespace-nowrap rounded bg-accent px-1.5 py-0.5 font-mono text-[10px] font-semibold tabular-nums text-cream'>
										<CountUpStat text={mem.agentOS} active={inView} />
									</span>
								</motion.div>
								<motion.div
									className='relative aspect-square min-h-[6px] overflow-hidden rounded-lg bg-accent'
									initial={{ height: reduced ? targetSide : '0%' }}
									animate={{ height: inView ? targetSide : '0%' }}
									transition={{ duration: reduced ? 0 : 0.6, ease: [...S_CURVE] }}
								>
									<div className='absolute inset-0 [background-image:repeating-linear-gradient(0deg,rgba(244,241,231,0.18)_0,rgba(244,241,231,0.18)_1px,transparent_1px,transparent_8px)]' />
									<div className='absolute inset-x-0 top-0 h-px bg-cream/50' />
								</motion.div>
							</div>
							<span className='font-mono text-[11px] font-medium text-accent-deep'>agentOS</span>
						</div>
					</div>
				</div>
			</motion.div>
		</Reveal>
	);
}
