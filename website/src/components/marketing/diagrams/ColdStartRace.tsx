'use client';

import { animate, motion, useInView, useMotionValue, useTransform, useReducedMotion } from 'framer-motion';
import type { MotionValue } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { ArrowDown, Box, Check } from 'lucide-react';
import { Reveal } from '../motion';
import { InkPanel } from '../editorial/InkPanel';
import { BenchToggle, CountUpStat } from './benchUI';
import { benchColdStart } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Cold-start comparison. Two hosts spin up agents, once, when scrolled into view.
//   Containers — each agent gets its own process: a separate box that boots on
//   its own (border red -> green + mini bar).
//   Agent OS   — every agent is packed into ONE shared process: a single box
//   that boots once.
// The contrast (four separate boots vs one grouped boot) is the point. A
// p50/p95/p99 toggle re-points the numbers to bench.ts so they stay accurate,
// and the race replays when the percentile changes.
// ---------------------------------------------------------------------------

const AGENTOS_MARK = '/images/agent-os/agentos-logo.svg'; // light variant for the dark panel
const AGENT_LOGOS = [
	'/images/agent-logos/pi.svg',
	'/images/agent-logos/claude-code.svg',
	'/images/agent-logos/codex.svg',
	'/images/agent-logos/opencode.svg',
	'/images/agent-logos/amp.svg',
];
const agentAt = (i: number) => AGENT_LOGOS[(i * 7) % AGENT_LOGOS.length];

const RED = '#d6453a';
const GREEN = '#3f9a59';
const BORDER_RED = 'rgba(214,69,58,0.6)';
const BORDER_GREEN = 'rgba(63,154,89,0.65)';

// ---- Animation timing ------------------------------------------------------
const BOOT_SEC = 2.7; // base animation seconds for the container boot
const AOS_DONE = 0.04; // fraction of the timeline at which Agent OS is finished

const MiniBar = ({ width, color }: { width: MotionValue<string>; color: MotionValue<string> }) => (
	<div className='h-1 w-full overflow-hidden rounded-full bg-cream/10'>
		<motion.div style={{ width, backgroundColor: color }} className='h-full rounded-full' />
	</div>
);

// One container: its own agent, booting on its own.
const ContainerBox = ({ progress, lo, hi, logo }: { progress: MotionValue<number>; lo: number; hi: number; logo: string }) => {
	const border = useTransform(progress, [lo, hi], [BORDER_RED, BORDER_GREEN]);
	const barWidth = useTransform(progress, [lo, hi], ['14%', '100%']);
	const barColor = useTransform(progress, [lo, hi], [RED, GREEN]);
	return (
		<div className='flex w-24 flex-col items-center gap-1'>
			<motion.div
				style={{ borderColor: border }}
				className='flex h-12 w-full items-center justify-center rounded-lg border-2 bg-cream/[0.06] px-2'
			>
				<img src={logo} alt='' aria-hidden='true' className='h-6 w-6 object-contain' />
			</motion.div>
			<MiniBar width={barWidth} color={barColor} />
		</div>
	);
};

// Agent OS: all agents packed inside ONE process box that boots once.
const SharedProcessBox = ({ progress, count }: { progress: MotionValue<number>; count: number }) => {
	const border = useTransform(progress, [0, AOS_DONE], [BORDER_RED, BORDER_GREEN]);
	const barWidth = useTransform(progress, [0, AOS_DONE], ['14%', '100%']);
	const barColor = useTransform(progress, [0, AOS_DONE], [RED, GREEN]);
	return (
		<div className='flex flex-col gap-1'>
			<motion.div style={{ borderColor: border }} className='rounded-xl border-2 bg-cream/[0.04] p-2'>
				<div className='flex flex-wrap items-center gap-1.5'>
					{Array.from({ length: count }).map((_, i) => (
						<span key={i} className='flex h-8 w-8 items-center justify-center rounded-md bg-cream/10 ring-1 ring-cream/15'>
							<img src={agentAt(i)} alt='' aria-hidden='true' className='h-5 w-5 object-contain' />
						</span>
					))}
				</div>
			</motion.div>
			<MiniBar width={barWidth} color={barColor} />
		</div>
	);
};

type HostCfg = {
	name: ReactNode;
	finalMs: number;
	doneAt: number;
	units: number;
	grouped: boolean; // Agent OS packs all agents into one process box
	accent: boolean;
	badge?: ReactNode;
};

const Host = ({ cfg, progress }: { cfg: HostCfg; progress: MotionValue<number> }) => {
	const counter = useTransform(progress, (p) => `~${Math.round(Math.min(1, p / cfg.doneAt) * cfg.finalMs).toLocaleString()} ms`);
	const checkOpacity = useTransform(progress, [0, cfg.doneAt * 0.95, cfg.doneAt], [0, 0, 1]);
	return (
		<div className={`rounded-xl border p-4 ${cfg.accent ? 'border-accent/40 bg-accent/[0.10]' : 'border-cream/10 bg-cream/[0.03]'}`}>
			<div className='mb-3 flex items-center justify-between gap-3'>
				<span className='flex items-center gap-2 text-sm font-medium text-cream'>{cfg.name}</span>
				<div className='flex items-center gap-3'>
					{cfg.badge}
					<motion.span style={{ opacity: checkOpacity }} aria-hidden='true'>
						<Check className='h-4 w-4' style={{ color: GREEN }} />
					</motion.span>
					<motion.span className='w-[5rem] text-right font-mono text-sm tabular-nums text-cream'>{counter}</motion.span>
				</div>
			</div>
			{cfg.grouped ? (
				<SharedProcessBox progress={progress} count={cfg.units} />
			) : (
				<div className='flex flex-wrap items-start gap-3'>
					{Array.from({ length: cfg.units }).map((_, i) => {
						const lo = (i / cfg.units) * 0.08;
						return <ContainerBox key={i} progress={progress} lo={lo} hi={cfg.doneAt} logo={agentAt(i)} />;
					})}
				</div>
			)}
		</div>
	);
};

export const ColdStartRace = () => {
	const reduced = useReducedMotion();
	const ref = useRef<HTMLDivElement>(null);
	const inView = useInView(ref, { once: true, margin: '-15% 0px' });
	const progress = useMotionValue(0);
	const [pct, setPct] = useState(2); // default p99 — the most dramatic tail

	const cold = benchColdStart[pct];
	const AOS_MS = Math.round(cold.agentOS);
	const CONTAINER_MS = cold.sandbox;
	const SPEEDUP = Math.round(cold.sandbox / cold.agentOS);

	// Loop the boot once it scrolls into view; restart it when the percentile changes.
	useEffect(() => {
		if (reduced) {
			progress.set(1);
			return;
		}
		if (!inView) return;
		progress.set(0);
		const controls = animate(progress, [0, 1], { duration: BOOT_SEC, ease: 'easeInOut', repeat: Infinity, repeatDelay: 0.9 });
		return () => controls.stop();
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [inView, reduced, pct]);

	return (
		<Reveal>
			<InkPanel>
				<div ref={ref} className='p-6 md:p-7'>
					<div className='flex flex-wrap items-start justify-between gap-3'>
						<div className='flex flex-col gap-1'>
							<span className='font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-sage'>Cold Start</span>
							<span className='inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-[0.12em] text-cream/40'>
								<ArrowDown className='h-3 w-3' /> lower is better
							</span>
						</div>
						<div className='w-40 max-sm:w-full'>
							<BenchToggle options={benchColdStart.map((g) => g.label)} active={pct} onChange={setPct} />
						</div>
					</div>

					{/* Headline multiplier — recomputes per percentile (92x / 170x / 516x) */}
					<div className='mt-5 flex items-baseline gap-2'>
						<span className='font-sans text-[2.75rem] font-medium leading-[1.0] tracking-[-0.02em] tabular-nums text-cream md:text-5xl'>
							<CountUpStat text={`${SPEEDUP}x`} active={inView} />
						</span>
						<span className='font-sans text-lg font-medium text-cream/45 md:text-xl'>faster</span>
					</div>

					<div key={pct} className='mt-7 flex flex-col gap-4'>
						<Host
							progress={progress}
							cfg={{
								name: (
									<>
										<Box className='h-4 w-4 text-cream/60' aria-hidden='true' /> Containers &mdash; one process each
									</>
								),
								finalMs: CONTAINER_MS,
								doneAt: 1,
								units: 4,
								grouped: false,
								accent: false,
							}}
						/>
						<Host
							progress={progress}
							cfg={{
								name: (
									<>
										<img src={AGENTOS_MARK} alt='' aria-hidden='true' className='h-4 w-4' /> Agent OS &mdash; one shared process
									</>
								),
								finalMs: AOS_MS,
								doneAt: AOS_DONE,
								units: 12,
								grouped: true,
								accent: true,
							}}
						/>
					</div>

					<p className='mt-4 font-mono text-[11px] leading-relaxed text-cream/40'>
						Same host. Each container boots its own process before code can run; Agent OS runs every agent in one shared process &mdash; the first instruction executes in ~{AOS_MS} ms vs ~{CONTAINER_MS.toLocaleString()} ms ({SPEEDUP}&times; faster). Toggle the percentile to compare median vs tail latency.
					</p>
				</div>
			</InkPanel>
		</Reveal>
	);
};
