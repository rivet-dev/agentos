'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { RotateCcw, SquarePen } from 'lucide-react';
import { motion, useReducedMotion } from 'framer-motion';
import { InkPanel } from '../editorial/InkPanel';

// ---------------------------------------------------------------------------
// A recorded agent coding session, played back line by line inside an ink
// terminal. It replaces the runtime SDK snippets in the "Any execution layer"
// section: instead of showing the exec API, it shows an agent using it, with
// one task flowing through shell, Python, and Node in a single VM.
// The transcript is scripted; the pacing is what sells it, so the prompt and
// commands type at human-ish speed while output arrives in bursts.
// ---------------------------------------------------------------------------

type SessionLineKind = 'user' | 'agent' | 'cmd' | 'out' | 'tool';

interface SessionLine {
	kind: SessionLineKind;
	text: string;
}

// A regression hunt after a deploy: the most recognizable coding-agent task,
// and the one where reaching for all three runtimes is causal rather than
// staged. Shell and git to orient, Python because the evidence is an events
// CSV, Node because the app under repair is JavaScript. The numbers
// cross-check on a second read (419 failed checkouts, 412 with coupons; the
// suspect commit touches coupon.js; pass 8 / fail 0 closes it).
const SESSION: SessionLine[] = [
	{ kind: 'user', text: "checkout conversion dropped after Tuesday's deploy, figure out why" },
	{ kind: 'cmd', text: 'ls' },
	{ kind: 'out', text: 'data  shop' },
	{ kind: 'cmd', text: 'git -C shop log --oneline -3' },
	{ kind: 'out', text: '4c7d1e9 refactor coupon validation' },
	{ kind: 'out', text: 'b2a90f3 update stripe client' },
	{ kind: 'out', text: 'e11d84a fix mobile nav overlap' },
	{ kind: 'agent', text: 'Quantifying the drop first, then checking that coupon change.' },
	{ kind: 'cmd', text: 'pip install pandas' },
	{ kind: 'out', text: 'Successfully installed numpy-2.3.1 pandas-2.3.2' },
	{ kind: 'tool', text: 'Write funnel.py · 21 lines' },
	{ kind: 'cmd', text: 'python funnel.py data/events.csv' },
	{ kind: 'out', text: 'checkout conversion: 6.4% -> 3.1% since the deploy' },
	{ kind: 'out', text: 'of 419 failed checkouts since then, 412 had a coupon code' },
	{ kind: 'cmd', text: 'git -C shop show --stat 4c7d1e9' },
	{ kind: 'out', text: 'src/checkout/coupon.js | 31 ++++++++-------' },
	{ kind: 'tool', text: 'Read shop/src/checkout/coupon.js' },
	{ kind: 'agent', text: 'Expiry is unix seconds, Date.now() is ms: no coupon ever validates.' },
	{ kind: 'tool', text: 'Edit shop/src/checkout/coupon.js · 1 line' },
	{ kind: 'cmd', text: 'node --test shop/test' },
	{ kind: 'out', text: '# pass 8' },
	{ kind: 'out', text: '# fail 0' },
	{ kind: 'agent', text: 'Root cause was 4c7d1e9. One-line fix in coupon.js:42, tests pass.' },
];

const WINDOW_TITLE = 'agentos vm · /home/agentos';
const CAPTION = 'One agent, one VM. Shell, Python, and Node behind one exec API.';
const CAPTION_ASIDE = 'node v22 · python 3.13';

// Pacing in clock ms. `user` and `cmd` lines type per character; the rest
// appear whole after their lead-in. Output directly after a command waits a
// beat longer, standing in for the run itself.
const START_DELAY = 400;
const RUN_DELAY = 520;
const LEAD: Record<SessionLineKind, number> = { user: 300, agent: 950, cmd: 650, out: 90, tool: 950 };
const CHAR_MS: Partial<Record<SessionLineKind, number>> = { user: 26, cmd: 13 };

interface ScheduledLine extends SessionLine {
	start: number;
	dur: number;
}

const buildSchedule = (lines: SessionLine[]): { schedule: ScheduledLine[]; total: number } => {
	let t = START_DELAY;
	const schedule = lines.map((line, i) => {
		const lead = line.kind === 'out' && lines[i - 1]?.kind === 'cmd' ? RUN_DELAY : LEAD[line.kind];
		const start = t + lead;
		const dur = (CHAR_MS[line.kind] ?? 0) * line.text.length;
		t = start + dur;
		return { ...line, start, dur };
	});
	return { schedule, total: t };
};

// Drives the playback clock with an accumulated per-frame delta (capped so a
// backgrounded tab resumes where it paused instead of jumping to the end).
const usePlaybackClock = (total: number, running: boolean, playKey: number, skipToEnd: boolean) => {
	const [clock, setClock] = useState(skipToEnd ? total : 0);

	useEffect(() => {
		if (skipToEnd) {
			setClock(total);
			return;
		}
		if (!running) return;
		setClock(0);
		let raf = 0;
		let last = performance.now();
		let elapsed = 0;
		const step = (now: number) => {
			elapsed += Math.min(now - last, 100);
			last = now;
			setClock(elapsed);
			if (elapsed < total) raf = requestAnimationFrame(step);
		};
		raf = requestAnimationFrame(step);
		return () => cancelAnimationFrame(raf);
	}, [total, running, playKey, skipToEnd]);

	return clock;
};

const Cursor = ({ blink }: { blink?: boolean }) => (
	<span
		aria-hidden='true'
		className={`-mb-0.5 ml-px inline-block h-[1.05em] w-[7px] translate-y-[2px] bg-cream/80 ${
			blink ? 'animate-[session-cursor-blink_1.1s_steps(2,jump-none)_infinite]' : ''
		}`}
	/>
);

// One transcript row. Typed rows render a partial slice with the cursor at the
// write head; whole rows fade in on mount.
const SessionRow = ({ line, chars, typing }: { line: ScheduledLine; chars: number; typing: boolean }) => {
	const typed = line.dur > 0;
	const text = typed ? line.text.slice(0, chars) : line.text;

	const body = (() => {
		switch (line.kind) {
			case 'user':
				return (
					<span className='text-cream'>
						<span aria-hidden='true' className='mr-2.5 select-none text-accent'>
							›
						</span>
						{text}
						{typing && <Cursor />}
					</span>
				);
			case 'agent':
				return (
					<span className='text-cream/80'>
						<span
							aria-hidden='true'
							className='mr-2.5 -mt-px inline-block h-[7px] w-[7px] rounded-full bg-sage align-middle'
						/>
						{text}
					</span>
				);
			case 'cmd':
				return (
					<span className='text-cream/90'>
						<span aria-hidden='true' className='mr-2.5 select-none text-sage'>
							$
						</span>
						{text}
						{typing && <Cursor />}
					</span>
				);
			case 'out':
				return <span className='block whitespace-pre-wrap pl-[1.35rem] text-cream/45'>{text}</span>;
			case 'tool':
				return (
					<span className='ml-[1.35rem] inline-flex items-center gap-1.5 rounded-md border border-cream/[0.12] bg-cream/[0.05] px-2 py-0.5 text-[12px] text-cream/70'>
						<SquarePen aria-hidden='true' className='h-3 w-3 text-cream/45' />
						{text}
					</span>
				);
		}
	})();

	if (typed) return <div>{body}</div>;
	return (
		<motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.18 }}>
			{body}
		</motion.div>
	);
};

export const AgentSessionDemo = () => {
	const reduced = useReducedMotion() ?? false;
	const [started, setStarted] = useState(false);
	const [playKey, setPlayKey] = useState(0);
	const scrollRef = useRef<HTMLDivElement>(null);

	const { schedule, total } = useMemo(() => buildSchedule(SESSION), []);
	const clock = usePlaybackClock(total, started, playKey, reduced);

	const visible = schedule.filter((line) => clock >= line.start);
	const done = clock >= total;

	// Keep the newest line in view while the session plays. Instant, not
	// smooth: smooth scrolling lags behind per-frame typing updates.
	useEffect(() => {
		const el = scrollRef.current;
		if (el) el.scrollTop = el.scrollHeight;
	}, [visible.length, clock, done]);

	return (
		<motion.div
			onViewportEnter={() => setStarted(true)}
			viewport={{ once: true, margin: '-20% 0px' }}
			className='mx-auto max-w-3xl'
		>
			<InkPanel caption={CAPTION} captionAside={CAPTION_ASIDE}>
				<div className='flex items-center gap-2 border-b border-cream/10 px-4 py-3'>
					<div className='h-3 w-3 rounded-full bg-cream/15' />
					<div className='h-3 w-3 rounded-full bg-cream/15' />
					<div className='h-3 w-3 rounded-full bg-cream/15' />
					<span className='ml-2 font-mono text-xs text-cream/45'>{WINDOW_TITLE}</span>
					{!reduced && (
						<button
							type='button'
							onClick={() => {
								setStarted(true);
								setPlayKey((k) => k + 1);
							}}
							aria-label='Replay session'
							className='ml-auto flex h-7 w-7 items-center justify-center rounded-md text-cream/40 transition-colors hover:bg-cream/[0.06] hover:text-cream/80'
						>
							<RotateCcw className='h-3.5 w-3.5' />
						</button>
					)}
				</div>

				<div ref={scrollRef} className='h-[420px] overflow-y-auto scroll-p-6 p-6 font-code text-[13px] leading-[1.75]'>
					<div className='flex flex-col gap-0.5'>
						{visible.map((line, i) => {
							const typing = line.dur > 0 && clock < line.start + line.dur;
							const chars = line.dur > 0 ? Math.ceil(((clock - line.start) / line.dur) * line.text.length) : line.text.length;
							return <SessionRow key={i} line={line} chars={Math.min(chars, line.text.length)} typing={typing} />;
						})}
						{/* Idle prompt once the session finishes: the machine is still there. */}
						{done && !reduced && (
							<motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.18, delay: 0.5 }}>
								<span aria-hidden='true' className='mr-2.5 select-none text-sage'>
									$
								</span>
								<Cursor blink />
							</motion.div>
						)}
					</div>
				</div>
			</InkPanel>
		</motion.div>
	);
};
