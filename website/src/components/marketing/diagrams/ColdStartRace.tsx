'use client';

import { animate, motion, useInView, useMotionValue, useTransform, useReducedMotion } from 'framer-motion';
import type { AnimationPlaybackControls, MotionValue } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { Box, Check } from 'lucide-react';
import { EASE, VIEWPORT, Reveal } from '../motion';
import { benchColdStart, benchWorkloads } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Cold-start comparison. Two hosts spin up agents, once, when scrolled into view.
//   Containers — each agent gets its own process: a separate box that boots on
//   its own (border red -> green + mini bar) and carries its own ~1 GB of memory.
//   Agent OS   — every agent is packed into ONE shared process: a single box
//   that boots once and shares memory, ~131 MB per agent.
// The contrast (four separate boots vs one grouped boot) is the point. Numbers
// come from bench.ts so they stay accurate.
// ---------------------------------------------------------------------------

const AGENTOS_MARK = '/images/agent-os/agentos-logo-ink.svg';
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

// ---- Sourced numbers -------------------------------------------------------
const cold = benchColdStart[0]; // p50
const AOS_MS = Math.round(cold.agentOS); // ~5 ms
const CONTAINER_MS = cold.sandbox; // ~440 ms
const SPEEDUP = Math.round(cold.sandbox / cold.agentOS); // ~92x
const mem = benchWorkloads.agent.memory; // { agentOS:"~131 MB", sandbox:"~1024 MB", multiplier:"8x smaller" }
const cost = benchWorkloads.agent.cost[0]; // AWS ARM { ratio, multiplier }
const MEM_X = mem.multiplier.split('x')[0]; // "8"
const CONTAINER_MEM = '~1 GB'; // ~1024 MB sandbox baseline, rounded for the chip
const AOS_MEM = mem.agentOS; // "~131 MB"

// ---- Animation timing ------------------------------------------------------
const BOOT_SEC = 2.7; // base animation seconds for the container boot (speed-adjustable)
const AOS_DONE = 0.04; // fraction of the timeline at which Agent OS is finished
// The speed slider maps to a playback-rate multiplier on the looping boot.
const speedFromSlider = (s: number) => 0.2 * Math.pow(30, s / 100); // 0.2x .. 6x
const slowdownLabel = (speed: number) => {
	const factor = (BOOT_SEC * 1000) / CONTAINER_MS / speed; // ~6 / speed
	return factor < 1.4 ? 'real time' : `slowed ~${Math.round(factor)}×`;
};

// A dashed grey box showing a memory figure.
const MemBox = ({ value, sub }: { value: string; sub: string }) => (
	<span className='flex flex-col items-center justify-center rounded-md border border-dashed border-ink/30 bg-ink/[0.05] px-1.5 py-0.5 leading-none'>
		<span className='font-mono text-[10px] font-semibold text-ink-soft'>{value}</span>
		<span className='mt-0.5 text-[7px] uppercase tracking-wide text-ink-faint'>{sub}</span>
	</span>
);

const MiniBar = ({ width, color }: { width: MotionValue<string>; color: MotionValue<string> }) => (
	<div className='h-1 w-full overflow-hidden rounded-full bg-ink/10'>
		<motion.div style={{ width, backgroundColor: color }} className='h-full rounded-full' />
	</div>
);

// One container: its own agent + its own memory, booting on its own.
const ContainerBox = ({ progress, lo, hi, logo }: { progress: MotionValue<number>; lo: number; hi: number; logo: string }) => {
	const border = useTransform(progress, [lo, hi], [BORDER_RED, BORDER_GREEN]);
	const barWidth = useTransform(progress, [lo, hi], ['14%', '100%']);
	const barColor = useTransform(progress, [lo, hi], [RED, GREEN]);
	return (
		<div className='flex w-[6.5rem] flex-col items-center gap-1'>
			<motion.div
				style={{ borderColor: border }}
				className='flex h-12 w-full items-center justify-center gap-1.5 rounded-lg border-2 bg-white/85 px-2 shadow-[0_1px_2px_rgba(27,25,22,0.05)]'
			>
				<img src={logo} alt='' aria-hidden='true' className='h-6 w-6 object-contain' />
				<MemBox value={CONTAINER_MEM} sub='/ agent' />
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
			<motion.div style={{ borderColor: border }} className='rounded-xl border-2 bg-white/60 p-2'>
				<div className='flex flex-wrap items-center gap-1.5'>
					{Array.from({ length: count }).map((_, i) => (
						<span key={i} className='flex h-8 w-8 items-center justify-center rounded-md bg-white/85 ring-1 ring-ink/10'>
							<img src={agentAt(i)} alt='' aria-hidden='true' className='h-5 w-5 object-contain' />
						</span>
					))}
					<MemBox value={AOS_MEM} sub='/ agent' />
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
	const counter = useTransform(progress, (p) => `~${Math.round(Math.min(1, p / cfg.doneAt) * cfg.finalMs)} ms`);
	const checkOpacity = useTransform(progress, [0, cfg.doneAt * 0.95, cfg.doneAt], [0, 0, 1]);
	return (
		<div className={`rounded-xl border p-4 ${cfg.accent ? 'border-accent/30 bg-accent/[0.05]' : 'border-ink/10 bg-paper/40'}`}>
			<div className='mb-3 flex items-center justify-between gap-3'>
				<span className='flex items-center gap-2 text-sm font-medium text-ink'>{cfg.name}</span>
				<div className='flex items-center gap-3'>
					{cfg.badge}
					<motion.span style={{ opacity: checkOpacity }} aria-hidden='true'>
						<Check className='h-4 w-4' style={{ color: GREEN }} />
					</motion.span>
					<motion.span className='w-[4.5rem] text-right font-mono text-sm tabular-nums text-ink'>{counter}</motion.span>
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

const Stat = ({ value, label, sub }: { value: string; label: string; sub: string }) => (
	<div className='rounded-xl border border-ink/10 bg-white/55 p-4 text-center'>
		<div className='text-2xl font-medium text-accent-deep md:text-3xl'>{value}</div>
		<div className='mt-0.5 text-sm font-medium text-ink'>{label}</div>
		<div className='mt-0.5 font-mono text-[11px] text-ink-faint'>{sub}</div>
	</div>
);

export const ColdStartRace = () => {
	const reduced = useReducedMotion();
	const ref = useRef<HTMLDivElement>(null);
	const inView = useInView(ref, { once: true, margin: '-15% 0px' });
	const progress = useMotionValue(0);
	const [slider, setSlider] = useState(50);
	const speed = speedFromSlider(slider);
	const speedRef = useRef(speed);
	speedRef.current = speed;
	const controlsRef = useRef<AnimationPlaybackControls | null>(null);

	// Create the looping boot once it scrolls into view; speed is adjusted live.
	useEffect(() => {
		if (reduced) {
			progress.set(1);
			return;
		}
		if (!inView) return;
		const controls = animate(progress, [0, 1], { duration: BOOT_SEC, ease: 'easeInOut', repeat: Infinity, repeatDelay: 0.9 });
		controls.speed = speedRef.current;
		controlsRef.current = controls;
		return () => controls.stop();
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [inView, reduced]);

	useEffect(() => {
		if (controlsRef.current) controlsRef.current.speed = speed;
	}, [speed]);

	return (
		<div>
			<Reveal>
				<div ref={ref} className='rounded-2xl border border-ink/10 bg-white/55 p-5 md:p-7'>
					<div className='mb-5 flex flex-wrap items-center justify-between gap-3'>
						<span className='font-mono text-[11px] uppercase tracking-[0.16em] text-ink-faint'>Cold start &middot; spinning up agents</span>
						<div className='flex items-center gap-2'>
							<span className='font-mono text-[10px] uppercase tracking-wide text-ink-faint'>Slow</span>
							<input
								type='range'
								min={0}
								max={100}
								value={slider}
								onChange={(e) => setSlider(Number(e.target.value))}
								aria-label='Animation playback speed'
								className='h-1.5 w-28 cursor-pointer accent-accent'
							/>
							<span className='font-mono text-[10px] uppercase tracking-wide text-ink-faint'>Fast</span>
							<span className='w-[5.5rem] text-right font-mono text-[10px] text-ink-soft'>{slowdownLabel(speed)}</span>
						</div>
					</div>

					<div className='flex flex-col gap-4'>
						<Host
							progress={progress}
							cfg={{
								name: (
									<>
										<Box className='h-4 w-4 text-ink-soft' aria-hidden='true' /> Containers &mdash; one process each
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
								badge: (
									<span className='rounded-full bg-accent/10 px-2 py-0.5 text-[10px] font-semibold text-accent-deep'>{SPEEDUP}&times; faster</span>
								),
							}}
						/>
					</div>

					<p className='mt-4 font-mono text-[11px] leading-relaxed text-ink-faint'>
						Same host, same memory. Each container is its own process with its own ~1 GB; Agent OS packs every agent into one shared process at {AOS_MEM} each ({MEM_X}&times; less memory). Real cold start: ~{AOS_MS} ms vs ~{CONTAINER_MS} ms ({SPEEDUP}&times; faster) &mdash; drag the slider to set playback speed.
					</p>
				</div>
			</Reveal>

			<motion.div
				className='mt-5 grid grid-cols-1 gap-3 sm:grid-cols-3'
				initial={reduced ? { opacity: 0 } : { opacity: 0, y: 12 }}
				whileInView={{ opacity: 1, y: 0 }}
				viewport={VIEWPORT}
				transition={{ duration: 0.5, ease: [...EASE], delay: 0.1 }}
			>
				<Stat value={`${SPEEDUP}×`} label='faster cold start' sub={`~${AOS_MS} ms vs ~${CONTAINER_MS} ms`} />
				<Stat value={`${MEM_X}×`} label='less memory' sub={`${mem.agentOS} vs ${mem.sandbox}`} />
				<Stat value={`${cost.ratio}×`} label='cheaper to run' sub='vs. sandboxes' />
			</motion.div>
		</div>
	);
};
