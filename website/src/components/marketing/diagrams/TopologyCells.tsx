'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { EASE, VIEWPORT } from '../motion';

// ---------------------------------------------------------------------------
// Compact topology cells that live inside the versus-sandbox comparison
// ledger, one under each column header, so the table's first "row" is the
// picture its data rows annotate. Built from real card surfaces (rings, soft
// layered shadows, hairline rules, mono microtype) rather than stroked SVG
// boxes. agentOS: the cell itself is your backend, filled with a dense grid
// of small accent-washed VM chips. Sandbox: a small backend chip separated
// from a stack of heavy 1 GiB boxes by a dashed network gap a request dot has
// to keep crossing.
// ---------------------------------------------------------------------------

const PANEL =
	'rounded-lg bg-white ring-1 ring-ink/[0.09] shadow-[0_1px_2px_rgba(20,20,22,0.06),0_4px_10px_-6px_rgba(20,20,22,0.14)]';

export const AgentOsTopologyCell = () => {
	const reduced = useReducedMotion();
	const tiles = Array.from({ length: 8 }, (_, i) => i);
	return (
		<div className='relative h-40 overflow-hidden rounded-xl bg-[#faf8f3] p-4 ring-1 ring-ink/[0.09] shadow-[inset_0_1px_0_rgba(255,255,255,0.8)]'>
			<span className='text-[11px] font-medium text-ink-soft'>Your backend</span>
			<div className='mt-3 grid w-fit grid-cols-4 gap-2'>
				{tiles.map((i) => (
					<motion.span
						key={i}
						initial={reduced ? undefined : { opacity: 0, scale: 0.6 }}
						whileInView={reduced ? undefined : { opacity: 1, scale: 1 }}
						viewport={VIEWPORT}
						transition={{ duration: 0.3, delay: 0.25 + i * 0.05, ease: [...EASE] }}
						className='flex h-9 w-9 items-center justify-center rounded-lg bg-white text-[10px] font-bold text-pine ring-1 ring-pine/30 shadow-[0_1px_2px_rgba(46,64,52,0.08),0_3px_8px_-4px_rgba(46,64,52,0.20)]'
					>
						OS
					</motion.span>
				))}
			</div>
			<span className='absolute bottom-3 right-4 font-mono text-[10px] text-ink-soft'>
				22–131 MB per agent
			</span>
		</div>
	);
};

export const SandboxTopologyCell = () => {
	const reduced = useReducedMotion();
	const boxes = [0, 1, 2];
	return (
		<div className='relative flex h-40 items-center rounded-xl bg-ink/[0.02] p-4 ring-1 ring-ink/[0.05]'>
			{/* Your backend, small: the agents are not in it */}
			<div className={`${PANEL} shrink-0 px-2.5 py-2`}>
				<span className='text-[11px] font-medium leading-none text-ink'>Your backend</span>
			</div>

			{/* The network gap a request has to keep crossing */}
			<div className='relative mx-2 h-px min-w-8 flex-1'>
				<span aria-hidden='true' className='absolute inset-x-0 top-1/2 border-t border-dashed border-ink/25' />
				<span className='absolute -top-4 left-1/2 -translate-x-1/2 font-mono text-[10px] text-ink-soft'>
					network
				</span>
				{!reduced && (
					<motion.span
						aria-hidden='true'
						initial={{ left: '0%', opacity: 0 }}
						animate={{ left: ['0%', '96%'], opacity: [0, 1, 1, 0] }}
						transition={{ duration: 1.8, repeat: Infinity, ease: 'easeInOut', repeatDelay: 0.5 }}
						className='absolute top-1/2 h-1 w-1 -translate-y-1/2 rounded-full bg-ink-faint'
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
						className={`${PANEL} flex items-baseline gap-2 px-3 py-2`}
					>
						<span className='text-[11px] font-medium leading-none text-ink'>Sandbox</span>
						<span className='font-mono text-[10px] leading-none text-ink-soft'>1 GiB</span>
					</motion.div>
				))}
			</div>
		</div>
	);
};
