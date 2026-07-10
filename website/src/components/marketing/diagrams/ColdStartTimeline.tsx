'use client';

import { animate, motion, useMotionValue, useMotionValueEvent, useReducedMotion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { COLDSTART_P99_MS, SANDBOX_COLDSTART_MS } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Cold-start anatomy, shown in the modal behind the comparison table's Startup
// row. Two stacked cards: a sandbox cold start as a sequential waterfall (API
// call, scheduling, boot, re-auth), and agentOS as a single tick so thin it
// needs a magnified inset to be visible at all. A shared clock counts the
// timeline in ms while the bars fill. Totals come from bench.ts; the sandbox
// step split is illustrative anatomy that sums to the measured p99 total.
// ---------------------------------------------------------------------------

const AXIS_MAX_MS = 3500;
const INSET_MAX_MS = 8;
// Wall-clock seconds per timeline millisecond: the full 3.5 s timeline plays
// in ~2.6 s.
const K = 0.00075;

const SANDBOX_TOTAL = SANDBOX_COLDSTART_MS.p99;
const STEP_OVERHEAD = 200;
const SANDBOX_STEPS = [
	{ label: 'API call to provider', start: 0, dur: STEP_OVERHEAD },
	{ label: 'Orchestration / scheduling', start: STEP_OVERHEAD, dur: STEP_OVERHEAD },
	{ label: 'Boot microVM & pull image', start: STEP_OVERHEAD * 2, dur: SANDBOX_TOTAL - STEP_OVERHEAD * 3 },
	{ label: 'Re-authenticate agent against API', start: SANDBOX_TOTAL - STEP_OVERHEAD, dur: STEP_OVERHEAD },
];
const MAGNIFICATION = Math.round(AXIS_MAX_MS / INSET_MAX_MS);
const TICKS = [0, 500, 1000, 1500, 2000, 2500, 3000, 3500];
// The magnified fill is a near-instant sweep: on the shared clock the whole
// agentOS boot is ~6 timeline-ms, so it must read as already finished while
// the sandbox is still on its first step, matching the tick on the main axis.
const INSET_FILL_SEC = 0.22;

const pct = (ms: number) => `${(ms / AXIS_MAX_MS) * 100}%`;

const fmt = (ms: number) => `${ms.toLocaleString('en-US')} ms`;

// Vertical gridlines shared by both timeline bodies.
const Gridlines = () => (
	<div aria-hidden='true' className='pointer-events-none absolute inset-0'>
		{TICKS.slice(1).map((t) => (
			<span key={t} className='absolute inset-y-0 w-px bg-ink/[0.06]' style={{ left: pct(t) }} />
		))}
	</div>
);

const CARD_CLASS =
	'rounded-2xl bg-gradient-to-b from-white to-[#f9f9fa] ring-1 ring-ink/[0.08] ' +
	'shadow-[inset_0_1px_0_rgba(255,255,255,0.9),0_1px_2px_-1px_rgba(20,20,22,0.10),0_8px_24px_-14px_rgba(20,20,22,0.20)]';

export const ColdStartTimeline = () => {
	const reduced = useReducedMotion();
	const [run, setRun] = useState(false);
	const clock = useMotionValue(0);
	// The clock writes straight to the DOM node: a per-frame setState here
	// would re-render the whole timeline ~60 times a second and drop frames.
	const clockRef = useRef<HTMLSpanElement>(null);

	useMotionValueEvent(clock, 'change', (v) => {
		if (clockRef.current) clockRef.current.textContent = Math.round(v).toLocaleString('en-US');
	});

	// The component mounts when the modal opens, so the sequence starts on
	// mount rather than on scroll.
	useEffect(() => {
		setRun(true);
		if (reduced) {
			clock.set(AXIS_MAX_MS);
			return;
		}
		const controls = animate(clock, AXIS_MAX_MS, { duration: AXIS_MAX_MS * K, ease: 'linear', delay: 0.2 });
		return () => controls.stop();
	}, [reduced, clock]);

	// Fills animate scaleX (compositor-only) instead of width, with the final
	// width set statically; with reduced motion bars render settled.
	const bar = (startMs: number, durMs: number) =>
		reduced
			? { initial: { scaleX: 1 }, animate: { scaleX: 1 } }
			: {
					initial: { scaleX: 0 },
					animate: run ? { scaleX: 1 } : { scaleX: 0 },
					transition: { duration: durMs * K, delay: 0.2 + startMs * K, ease: 'linear' as const },
				};

	const appear = (atMs: number, extraDelay = 0) =>
		reduced
			? { initial: { opacity: 1 }, animate: { opacity: 1 } }
			: {
					initial: { opacity: 0 },
					animate: run ? { opacity: 1 } : { opacity: 0 },
					transition: { duration: 0.25, delay: 0.2 + atMs * K + extraDelay },
				};

	return (
		<div>
			{/* Header: title + running clock */}
			<div className='mb-4 flex items-baseline justify-between'>
				<span className='text-sm font-medium text-ink'>Cold-start timeline</span>
				<span className='font-mono text-xs tabular-nums text-ink-soft'>
					t = <span ref={clockRef}>0</span> ms
				</span>
			</div>

			{/* Sandbox waterfall */}
			<div className={`p-5 md:p-6 ${CARD_CLASS}`}>
				<div className='mb-5 flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1'>
					<p className='text-sm text-ink-soft'>
						<span className='font-semibold text-ink'>microVM sandboxes</span> · separate infrastructure (p99, fastest
						provider)
					</p>
					<motion.p className='text-sm text-ink' {...appear(SANDBOX_TOTAL)}>
						Total: <span className='font-semibold'>{fmt(SANDBOX_TOTAL)}</span>
					</motion.p>
				</div>

				<div className='flex flex-col gap-2.5'>
					{SANDBOX_STEPS.map((step, i) => (
						<div key={step.label} className='flex items-center gap-3'>
							<div className='flex w-36 shrink-0 items-baseline justify-end gap-1.5 text-right md:w-52'>
								<span className='font-mono text-[10px] text-ink-faint'>{i + 1}</span>
								<span className='truncate text-xs leading-tight text-ink-soft'>{step.label}</span>
							</div>
							<div className='relative h-6 min-w-0 flex-1'>
								<Gridlines />
								{/* Dashed marker at the measured total */}
								<span
									aria-hidden='true'
									className='absolute inset-y-0 w-px border-l border-dashed border-ink/25'
									style={{ left: pct(SANDBOX_TOTAL) }}
								/>
								<motion.span
									className='absolute inset-y-0.5 rounded-[3px] bg-ink/75'
									style={{ left: pct(step.start), width: pct(step.dur), transformOrigin: 'left', willChange: 'transform' }}
									{...bar(step.start, step.dur)}
								/>
								<motion.span
									className='absolute top-1/2 -translate-y-1/2 whitespace-nowrap font-mono text-[10px] text-ink-faint'
									style={{ left: `calc(${pct(step.start + step.dur)} + 8px)` }}
									{...appear(step.start + step.dur)}
								>
									{fmt(step.dur)}
								</motion.span>
							</div>
						</div>
					))}
				</div>
			</div>

			{/* agentOS: a tick at zero, with a magnified inset */}
			<div className={`mt-4 p-5 md:p-6 ${CARD_CLASS}`}>
				<div className='mb-5 flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1'>
					<p className='text-sm text-ink-soft'>
						<span className='font-semibold text-ink'>agentOS</span> · WebAssembly, in-process (p99)
					</p>
					<motion.p className='text-sm text-accent-deep' {...appear(0, 0.5)}>
						Total: <span className='font-semibold'>{COLDSTART_P99_MS} ms</span>
					</motion.p>
				</div>

				<div className='flex items-center gap-3'>
					<div className='flex w-36 shrink-0 items-center justify-end md:w-52'>
						<span className='truncate text-xs leading-tight text-ink-soft'>Boot agentOS VM</span>
					</div>
					<div className='relative h-6 min-w-0 flex-1'>
						<Gridlines />
						<motion.span className='absolute inset-y-0.5 w-[2px] rounded-full bg-accent' {...appear(0, 0.2)} />
					</div>
				</div>

				{/* Time axis */}
				<div className='ml-[156px] md:ml-[220px]'>
					<div className='relative h-4'>
						{TICKS.map((t) => (
							<span
								key={t}
								className='absolute top-1 -translate-x-1/2 font-mono text-[9px] text-ink-faint'
								style={{ left: pct(t) }}
							>
								{t.toLocaleString('en-US')}
							</span>
						))}
					</div>

					{/* Dashed connectors from the origin down to the inset */}
					<svg aria-hidden='true' viewBox='0 0 100 10' preserveAspectRatio='none' className='block h-8 w-full'>
						<line x1={0.4} y1={0} x2={1} y2={10} stroke='#8A8478' strokeWidth={1} strokeDasharray='3 3' vectorEffect='non-scaling-stroke' opacity={0.6} />
						<line x1={0.4} y1={0} x2={44} y2={10} stroke='#8A8478' strokeWidth={1} strokeDasharray='3 3' vectorEffect='non-scaling-stroke' opacity={0.6} />
					</svg>

					{/* Magnified inset */}
					<motion.div className={`w-full max-w-md p-4 ${CARD_CLASS}`} {...appear(0, 0.4)}>
						<div className='mb-3 flex justify-end'>
							<span className='rounded-full border border-ink/10 bg-ink/[0.04] px-2.5 py-0.5 font-mono text-[10px] text-ink-soft'>
								{MAGNIFICATION}× magnified
							</span>
						</div>
						<div className='relative h-7'>
							<div aria-hidden='true' className='pointer-events-none absolute inset-0'>
								{[2, 4, 6, 8].map((t) => (
									<span key={t} className='absolute inset-y-0 w-px bg-ink/[0.06]' style={{ left: `${(t / INSET_MAX_MS) * 100}%` }} />
								))}
							</div>
							{/* At this magnification 6.1 ms takes as long as the whole sandbox
							    timeline above, so the fill runs the full clock duration. */}
							<motion.span
								className='absolute inset-y-1 rounded-[3px] bg-accent ring-1 ring-accent-deep/50'
								style={{ width: `${(COLDSTART_P99_MS / INSET_MAX_MS) * 100}%`, transformOrigin: 'left', willChange: 'transform' }}
								initial={reduced ? { scaleX: 1 } : { scaleX: 0 }}
								animate={reduced || !run ? undefined : { scaleX: 1 }}
								transition={{ duration: INSET_FILL_SEC, delay: reduced ? 0 : 0.35, ease: 'linear' }}
							/>
							<motion.span
								className='absolute top-1/2 -translate-y-1/2 whitespace-nowrap font-mono text-[10px] text-accent-deep'
								style={{ left: `calc(${(COLDSTART_P99_MS / INSET_MAX_MS) * 100}% + 8px)` }}
								{...appear(0, INSET_FILL_SEC + 0.4)}
							>
								{COLDSTART_P99_MS} ms
							</motion.span>
						</div>
						<div className='relative mt-1 h-4'>
							{[0, 2, 4, 6, 8].map((t) => (
								<span
									key={t}
									className='absolute top-0 -translate-x-1/2 font-mono text-[9px] text-ink-faint'
									style={{ left: `${(t / INSET_MAX_MS) * 100}%` }}
								>
									{t}
								</span>
							))}
						</div>
					</motion.div>
				</div>
			</div>
		</div>
	);
};
