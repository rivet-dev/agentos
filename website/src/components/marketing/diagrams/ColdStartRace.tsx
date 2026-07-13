'use client';

import { animate, motion, useInView, useMotionValue, useTransform, useReducedMotion } from 'framer-motion';
import type { MotionValue } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { Box, Check } from 'lucide-react';
import { Reveal } from '../motion';
import { BenchToggle, BenchInfoTooltip } from './benchUI';
import { benchColdStart, SANDBOX_COLDSTART_PROVIDER, BENCHMARK_DATE } from '../../../data/bench';

// ---------------------------------------------------------------------------
// Cold-start comparison. Two hosts spin up agents, once, when scrolled into view.
//   Containers — each agent gets its own process: a separate box that boots on
//   its own (border red -> green + mini bar).
//   agentOS   — every agent is packed into ONE shared process: a single box
//   that boots once.
// The contrast (four separate boots vs one grouped boot) is the point. The
// container boot is shown slowed ~6x so the wait is visible. Numbers come from
// bench.ts so they stay accurate.
// ---------------------------------------------------------------------------

const AGENTOS_MARK = '/images/agent-os/agentos-logo-ink.svg';
const AGENT_LOGOS = [
	'/images/agent-logos/pi.svg',
	'/images/agent-logos/claude-code.svg',
	'/images/agent-logos/codex.svg',
	'/images/agent-logos/opencode.svg',
];
const agentAt = (i: number) => AGENT_LOGOS[(i * 7) % AGENT_LOGOS.length];

const RED = '#d6453a';
const GREEN = '#3f9a59';
const BORDER_RED = 'rgba(214,69,58,0.6)';
const BORDER_GREEN = 'rgba(63,154,89,0.65)';

// ---- Animation timing ------------------------------------------------------
// The boot is scaled to each percentile's REAL container cold start, always
// slowed 6x — so p99 crawls ~6x longer than p50, in true proportion.
const SLOWDOWN = 6;
// agentOS boots near-instantly, so its box snaps done in a fixed, snappy time —
// independent of the container's 6x slowdown. It must never appear to "wait".
const AOS_BOOT_SEC = 0.5;
const bootDuration = (containerMs: number) => (containerMs / 1000) * SLOWDOWN;
const slowdownFactor = (containerMs: number) => (bootDuration(containerMs) * 1000) / containerMs;
const slowdownLabel = (containerMs: number) => {
	const factor = slowdownFactor(containerMs);
	return factor < 1.4 ? 'in real time' : `slowed ~${Math.round(factor)}×`;
};
// Pill copy for the containers row. Phrased as a playback note so it reads as a
// display speed, not a perf claim that would clash with agentOS's "Nx faster".
const slowdownPill = (containerMs: number) => {
	const factor = slowdownFactor(containerMs);
	return factor < 1.4 ? 'shown in real time' : `shown ~${Math.round(factor)}× slower`;
};

const MiniBar = ({ width, color }: { width: MotionValue<string>; color: MotionValue<string> }) => (
	<div className='h-1 w-full overflow-hidden rounded-full bg-ink/10'>
		<motion.div style={{ width, backgroundColor: color }} className='h-full rounded-full' />
	</div>
);

// One container: its own agent, booting on its own.
const ContainerBox = ({ progress, lo, hi, logo }: { progress: MotionValue<number>; lo: number; hi: number; logo: string }) => {
	const border = useTransform(progress, [lo, hi], [BORDER_RED, BORDER_GREEN]);
	const barWidth = useTransform(progress, [lo, hi], ['14%', '100%']);
	const barColor = useTransform(progress, [lo, hi], [RED, GREEN]);
	return (
		<div className='flex w-16 flex-col items-center gap-1'>
			<motion.div
				style={{ borderColor: border }}
				className='flex h-12 w-full items-center justify-center rounded-lg border-2 bg-white/85 shadow-[0_1px_2px_rgba(27,25,22,0.05)]'
			>
				<img src={logo} alt='' aria-hidden='true' className='h-6 w-6 object-contain' />
			</motion.div>
			<MiniBar width={barWidth} color={barColor} />
		</div>
	);
};

// agentOS: all agents packed inside ONE process box that boots once.
const SharedProcessBox = ({ progress, count, doneAt }: { progress: MotionValue<number>; count: number; doneAt: number }) => {
	const border = useTransform(progress, [0, doneAt], [BORDER_RED, BORDER_GREEN]);
	const barWidth = useTransform(progress, [0, doneAt], ['14%', '100%']);
	const barColor = useTransform(progress, [0, doneAt], [RED, GREEN]);
	return (
		<div className='flex flex-col gap-1'>
			<motion.div style={{ borderColor: border }} className='rounded-xl border-2 bg-white/60 p-2'>
				<div className='flex flex-wrap items-center gap-1.5'>
					{Array.from({ length: count }).map((_, i) => (
						<span key={i} className='flex h-8 w-8 items-center justify-center rounded-md bg-white/85 ring-1 ring-ink/10'>
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
	grouped: boolean; // agentOS packs all agents into one process box
	accent: boolean;
	badge?: ReactNode;
};

const Host = ({ cfg, progress }: { cfg: HostCfg; progress: MotionValue<number> }) => {
	const counter = useTransform(progress, (p) => `~${Math.round(Math.min(1, p / cfg.doneAt) * cfg.finalMs)} ms`);
	const checkOpacity = useTransform(progress, [0, cfg.doneAt * 0.95, cfg.doneAt], [0, 0, 1]);
	return (
		<div className={`rounded-xl border p-4 ${cfg.accent ? 'border-accent/30 bg-accent/[0.05]' : 'border-ink/10 bg-paper/40'}`}>
			<div className='mb-3 flex items-center justify-between gap-3'>
				<span className='flex items-center gap-2 text-sm font-bold text-ink'>{cfg.name}</span>
				<div className='flex items-center gap-3'>
					{cfg.badge}
					<motion.span style={{ opacity: checkOpacity }} aria-hidden='true'>
						<Check className='h-4 w-4' style={{ color: GREEN }} />
					</motion.span>
					<motion.span className='w-[4.5rem] text-right font-mono text-sm tabular-nums text-ink'>{counter}</motion.span>
				</div>
			</div>
			{cfg.grouped ? (
				<SharedProcessBox progress={progress} count={cfg.units} doneAt={cfg.doneAt} />
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
	const progress = useMotionValue(0); // containers — slowed boot
	const aosProgress = useMotionValue(0); // agentOS — fixed snappy boot
	const [pct, setPct] = useState(0); // p50 by default

	const cold = benchColdStart[pct];
	const aosMs = Math.round(cold.agentOS);
	const containerMs = cold.sandbox;
	const speedup = Math.round(cold.sandbox / cold.agentOS);
	const durationSec = bootDuration(containerMs);

	// Play the boot once it scrolls into view; replay when the percentile changes.
	// The containers crawl over the slowed duration; agentOS snaps done in a fixed
	// fast time so it never appears to scale with the container slowdown.
	useEffect(() => {
		if (reduced) {
			progress.set(1);
			aosProgress.set(1);
			return;
		}
		if (!inView) return;
		progress.set(0);
		aosProgress.set(0);
		const c1 = animate(progress, [0, 1], { duration: durationSec, ease: 'easeInOut' });
		const c2 = animate(aosProgress, [0, 1], { duration: AOS_BOOT_SEC, ease: 'easeOut' });
		return () => {
			c1.stop();
			c2.stop();
		};
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [inView, reduced, pct]);

	return (
		<Reveal>
			<div ref={ref} className='rounded-2xl border border-ink/10 bg-white/55 p-5 md:p-7'>
				<div className='mb-5 flex flex-wrap items-center justify-between gap-3'>
					<div className='flex items-center gap-2.5'>
						<span className='font-mono text-[11px] font-bold uppercase tracking-[0.16em] text-ink'>Cold start</span>
						<span className='rounded-full bg-ink/[0.06] px-2 py-0.5 text-[10px] font-semibold text-ink-soft'>{slowdownPill(containerMs)}</span>
					</div>
					<div className='w-44 max-sm:w-full'>
						<BenchToggle options={benchColdStart.map((g) => g.label)} active={pct} onChange={setPct} />
					</div>
				</div>

				<div key={pct} className='flex flex-col gap-4'>
					<Host
						progress={progress}
						cfg={{
							name: (
								<>
									<Box className='h-4 w-4 text-ink-soft' aria-hidden='true' /> Containers &mdash; one process each
								</>
							),
							finalMs: containerMs,
							doneAt: 1,
							units: 4,
							grouped: false,
							accent: false,
						}}
					/>
					<Host
						progress={aosProgress}
						cfg={{
							name: (
								<>
									<img src={AGENTOS_MARK} alt='' aria-hidden='true' className='h-4 w-4' /> agentOS &mdash; one shared process
									<BenchInfoTooltip>
										<strong>What&apos;s measured:</strong> Time from requesting an execution to first code running.
										<br /><br />
										<strong>Why the gap:</strong> agentOS runs agents in-process &mdash; WASM inside your host. No VM to boot, no network hop, no disk image. Sandboxes must boot an entire environment, allocate memory, and establish a network connection before code can run.
										<br /><br />
										<strong>Sandbox baseline:</strong> {SANDBOX_COLDSTART_PROVIDER}, the fastest mainstream sandbox provider as of {BENCHMARK_DATE}.
										<br /><br />
										<strong>agentOS:</strong> Median of 10,000 runs (100 iterations x 100 samples) on Intel i7-12700KF.
									</BenchInfoTooltip>
								</>
							),
							finalMs: aosMs,
							doneAt: 1,
							units: 12,
							grouped: true,
							accent: true,
							badge: (
								<span className='rounded-full bg-accent/10 px-2 py-0.5 text-[10px] font-semibold text-accent-deep'>{speedup}&times; faster</span>
							),
						}}
					/>
				</div>

				<p className='mt-4 font-mono text-[11px] leading-relaxed text-ink-faint'>
					Each container is its own process that boots on its own; agentOS packs every agent into one shared process that boots once. Real {cold.label} cold start: ~{aosMs} ms vs ~{containerMs.toLocaleString()} ms ({speedup}&times; faster) &mdash; container boot shown {slowdownLabel(containerMs)}.
				</p>
			</div>
		</Reveal>
	);
};
