'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { EASE, VIEWPORT } from '../motion';

// ---------------------------------------------------------------------------
// Before/after topology panels for the "what it is" section, stacked as two
// wide cells. Built from real card surfaces (rings, soft layered shadows,
// hairline rules) rather than stroked SVG boxes. Sandbox: a small backend
// chip separated from a stack of heavy 1 GiB boxes by a dashed network gap a
// request dot has to keep crossing. agentOS: the panel itself is your
// backend, filled with a wrapping run of small VM chips.
// ---------------------------------------------------------------------------

const PANEL =
	'rounded-lg bg-white ring-1 ring-ink/[0.09] shadow-[0_1px_2px_rgba(20,20,22,0.06),0_4px_10px_-6px_rgba(20,20,22,0.14)]';

export const AgentOsTopologyCell = () => {
	const reduced = useReducedMotion();
	const tiles = Array.from({ length: 22 }, (_, i) => i);
	return (
		<div className='min-h-40 rounded-xl bg-[#faf8f3] p-4 ring-1 ring-ink/10 shadow-[inset_0_1px_0_rgba(255,255,255,0.8)]'>
			<div className='flex items-baseline justify-between gap-3'>
				<span className='text-[13px] font-medium text-ink'>Your backend</span>
				<span className='font-mono text-[11px] text-ink-soft'>22–131 MB per agent</span>
			</div>
			<div className='mt-3.5 flex flex-wrap gap-2'>
				{tiles.map((i) => (
					<motion.span
						key={i}
						initial={reduced ? undefined : { opacity: 0, scale: 0.6 }}
						whileInView={reduced ? undefined : { opacity: 1, scale: 1 }}
						viewport={VIEWPORT}
						transition={{ duration: 0.3, delay: 0.15 + i * 0.03, ease: [...EASE] }}
						className='flex h-9 w-9 items-center justify-center rounded-lg bg-white text-[11px] font-semibold text-pine ring-1 ring-pine/35 shadow-[0_1px_2px_rgba(46,64,52,0.08),0_3px_8px_-4px_rgba(46,64,52,0.20)]'
					>
						OS
					</motion.span>
				))}
			</div>
		</div>
	);
};

export const SandboxTopologyCell = () => {
	const reduced = useReducedMotion();
	const boxes = [0, 1, 2];
	return (
		<div className='relative flex min-h-40 items-center rounded-xl bg-ink/[0.03] p-4 ring-1 ring-ink/[0.08]'>
			{/* Your backend, small: the agents are not in it */}
			<div className={`${PANEL} shrink-0 px-3.5 py-2.5`}>
				<span className='text-[13px] font-medium leading-none text-ink'>Your backend</span>
			</div>

			{/* The network gap a request has to keep crossing */}
			<div className='relative mx-3 h-px min-w-8 flex-1'>
				<span aria-hidden='true' className='absolute inset-x-0 top-1/2 border-t border-dashed border-ink/30' />
				<span className='absolute -top-5 left-1/2 -translate-x-1/2 font-mono text-[11px] text-ink-soft'>
					network
				</span>
				{!reduced && (
					<motion.span
						aria-hidden='true'
						initial={{ left: '0%', opacity: 0 }}
						animate={{ left: ['0%', '96%'], opacity: [0, 1, 1, 0] }}
						transition={{ duration: 1.8, repeat: Infinity, ease: 'easeInOut', repeatDelay: 0.5 }}
						className='absolute top-1/2 h-1.5 w-1.5 -translate-y-1/2 rounded-full bg-ink/50'
					/>
				)}
			</div>

			{/* The fleet: one heavy box per agent */}
			<div className='flex shrink-0 flex-col gap-1.5'>
				{boxes.map((i) => (
					<motion.div
						key={i}
						initial={reduced ? undefined : { opacity: 0, x: 8 }}
						whileInView={reduced ? undefined : { opacity: 1, x: 0 }}
						viewport={VIEWPORT}
						transition={{ duration: 0.4, delay: 0.3 + i * 0.14, ease: [...EASE] }}
						className={`${PANEL} flex items-baseline gap-2.5 px-3.5 py-2.5`}
					>
						<span className='text-[13px] font-medium leading-none text-ink'>Sandbox</span>
						<span className='font-mono text-[11px] leading-none text-ink-soft'>1 GiB</span>
					</motion.div>
				))}
			</div>
		</div>
	);
};
