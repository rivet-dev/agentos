'use client';

import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { Briefcase, ListChecks, SquareTerminal, Workflow } from 'lucide-react';
import { EASE, VIEWPORT } from '../motion';

// ---------------------------------------------------------------------------
// Harness architecture diagram. A plus/cross layout branded as agentOS: the
// running agent sits at the center (cycling through every supported agent) and
// the harness brokers bidirectional request/response traffic out to Tools,
// Session, Sandbox, and Orchestration. Flow dots animate along the arrows to
// show that everything routes through agentOS.
// ---------------------------------------------------------------------------

const ARROW_COLOR = '#8A8478'; // ink-faint
const ACCENT = '#CB5A33';
const AGENTOS_MARK = '/images/agent-os/agentos-logo-ink.svg';

const AGENT_LOGOS = [
	{ src: '/images/agent-logos/pi.svg', name: 'Pi' },
	{ src: '/images/agent-logos/claude-code.svg', name: 'Claude Code' },
	{ src: '/images/agent-logos/codex.svg', name: 'Codex' },
	{ src: '/images/agent-logos/opencode.svg', name: 'OpenCode' },
	{ src: '/images/agent-logos/amp.svg', name: 'Amp' },
];

// The center node: cross-fades through every supported agent logo, conveying
// that any agent plugs into the harness.
const CyclingAgentLogo = ({ size }: { size: string }) => {
	const reduced = useReducedMotion();
	const [i, setI] = useState(0);
	useEffect(() => {
		if (reduced) return;
		const id = setInterval(() => setI((p) => (p + 1) % AGENT_LOGOS.length), 1900);
		return () => clearInterval(id);
	}, [reduced]);
	const a = AGENT_LOGOS[i];
	return (
		<div className={`relative ${size}`}>
			<AnimatePresence mode='wait'>
				<motion.img
					key={a.src}
					src={a.src}
					alt={a.name}
					initial={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.7 }}
					animate={{ opacity: 1, scale: 1 }}
					exit={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.7 }}
					transition={{ duration: 0.4, ease: [...EASE] }}
					className='absolute inset-0 m-auto h-full w-full object-contain'
				/>
			</AnimatePresence>
		</div>
	);
};

// A single diagram card: a header strip with the title and a body with one icon.
const DiagramCard = ({
	title,
	subtitle,
	icon,
	accent,
}: {
	title: string;
	subtitle?: string;
	icon: ReactNode;
	accent?: boolean;
}) => (
	<div
		className={`flex h-full w-full flex-col overflow-hidden rounded-2xl border bg-white/75 shadow-[0_1px_3px_rgba(27,25,22,0.05)] ${
			accent ? 'border-accent/40 ring-1 ring-accent/15' : 'border-ink/10'
		}`}
	>
		<div
			className={`border-b px-2 py-1 text-center ${
				accent ? 'border-accent/25 bg-accent/10' : 'border-ink/10 bg-ink/[0.035]'
			}`}
		>
			<div className={`text-[10px] font-semibold leading-tight tracking-tight ${accent ? 'text-accent-deep' : 'text-ink'}`}>
				{title}
			</div>
			{subtitle ? <div className='text-[8px] leading-tight text-ink-faint'>{subtitle}</div> : null}
		</div>
		<div className='flex flex-1 items-center justify-center p-1'>{icon}</div>
	</div>
);

const SATELLITES = {
	tools: { title: 'Tools', subtitle: '+ Resources / MCP', icon: <Briefcase className='h-5 w-5 text-ink-soft' aria-hidden='true' /> },
	session: { title: 'Session', icon: <ListChecks className='h-5 w-5 text-ink-soft' aria-hidden='true' /> },
	sandbox: { title: 'Sandbox', icon: <SquareTerminal className='h-5 w-5 text-ink-soft' aria-hidden='true' /> },
	orchestration: { title: 'Orchestration', icon: <Workflow className='h-5 w-5 text-ink-soft' aria-hidden='true' /> },
} as const;

// ---- Desktop cross geometry (viewBox 0 0 300 300) --------------------------
// Card centers map to 50 / 150 / 250 on each axis. Solid arrows point outward
// from the agent; dashed arrows point back in. Each is offset ±6 perpendicular.
type Arrow = { x1: number; y1: number; x2: number; y2: number };
const SOLID_ARROWS: Arrow[] = [
	{ x1: 144, y1: 109, x2: 144, y2: 86.5 }, // up to Tools
	{ x1: 109, y1: 144, x2: 86.5, y2: 144 }, // left to Session
	{ x1: 191, y1: 144, x2: 213.5, y2: 144 }, // right to Sandbox
	{ x1: 144, y1: 191, x2: 144, y2: 213.5 }, // down to Orchestration
];
const DASHED_ARROWS: Arrow[] = [
	{ x1: 156, y1: 86.5, x2: 156, y2: 109 }, // Tools -> agent
	{ x1: 86.5, y1: 156, x2: 109, y2: 156 }, // Session -> agent
	{ x1: 213.5, y1: 156, x2: 191, y2: 156 }, // Sandbox -> agent
	{ x1: 156, y1: 213.5, x2: 156, y2: 191 }, // Orchestration -> agent
];
const d = (a: Arrow) => `M${a.x1} ${a.y1} L${a.x2} ${a.y2}`;

const FlowDot = ({ a, color, delay, reduced }: { a: Arrow; color: string; delay: number; reduced: boolean | null }) => {
	if (reduced) return null;
	return (
		<motion.circle
			r={2.1}
			fill={color}
			initial={{ cx: a.x1, cy: a.y1, opacity: 0 }}
			animate={{ cx: [a.x1, a.x2], cy: [a.y1, a.y2], opacity: [0, 1, 1, 0] }}
			transition={{ duration: 1.7, repeat: Infinity, ease: 'easeInOut', delay, repeatDelay: 0.5 }}
		/>
	);
};

const CrossCard = ({
	left,
	top,
	width,
	delay,
	children,
	reduced,
}: {
	left: string;
	top: string;
	width: string;
	delay: number;
	children: ReactNode;
	reduced: boolean | null;
}) => (
	<div className='absolute z-10' style={{ left, top, width, aspectRatio: '1', transform: 'translate(-50%, -50%)' }}>
		<motion.div
			className='h-full w-full'
			initial={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.94 }}
			whileInView={reduced ? { opacity: 1 } : { opacity: 1, scale: 1 }}
			viewport={VIEWPORT}
			transition={{ duration: 0.5, ease: [...EASE], delay }}
		>
			{children}
		</motion.div>
	</div>
);

const DesktopCross = ({ reduced }: { reduced: boolean | null }) => (
	<div className='relative mx-auto hidden aspect-square w-full max-w-[420px] md:block'>
		<svg
			viewBox='0 0 300 300'
			preserveAspectRatio='xMidYMid meet'
			className='pointer-events-none absolute inset-0 h-full w-full'
			aria-hidden='true'
		>
			<defs>
				<marker id='harness-arrow' markerWidth='8' markerHeight='8' refX='6.5' refY='3' orient='auto' markerUnits='userSpaceOnUse'>
					<path d='M0.5 0.5 L6.5 3 L0.5 5.5 Z' fill={ARROW_COLOR} />
				</marker>
			</defs>
			{SOLID_ARROWS.map((a, i) => (
				<motion.path
					key={`solid-${i}`}
					d={d(a)}
					fill='none'
					stroke={ARROW_COLOR}
					strokeWidth={1.5}
					strokeLinecap='round'
					markerEnd='url(#harness-arrow)'
					initial={{ opacity: 0 }}
					whileInView={{ opacity: 1 }}
					viewport={VIEWPORT}
					transition={{ duration: 0.4, ease: [...EASE], delay: 0.4 + i * 0.05 }}
				/>
			))}
			{DASHED_ARROWS.map((a, i) => (
				<motion.path
					key={`dashed-${i}`}
					d={d(a)}
					fill='none'
					stroke={ARROW_COLOR}
					strokeWidth={1.4}
					strokeLinecap='round'
					strokeDasharray='4 4'
					markerEnd='url(#harness-arrow)'
					initial={{ opacity: 0 }}
					whileInView={{ opacity: 1 }}
					viewport={VIEWPORT}
					transition={{ duration: 0.4, ease: [...EASE], delay: 0.45 + i * 0.05 }}
				/>
			))}
			{/* Live routing: dots flow out (accent request) and back (response) */}
			{SOLID_ARROWS.map((a, i) => (
				<FlowDot key={`fout-${i}`} a={a} color={ACCENT} delay={i * 0.22} reduced={reduced} />
			))}
			{DASHED_ARROWS.map((a, i) => (
				<FlowDot key={`fin-${i}`} a={a} color={ARROW_COLOR} delay={0.85 + i * 0.22} reduced={reduced} />
			))}
		</svg>

		<CrossCard left='50%' top='16.667%' width='23%' delay={0.1} reduced={reduced}>
			<DiagramCard {...SATELLITES.tools} />
		</CrossCard>
		<CrossCard left='16.667%' top='50%' width='23%' delay={0.16} reduced={reduced}>
			<DiagramCard {...SATELLITES.session} />
		</CrossCard>
		<CrossCard left='50%' top='50%' width='28%' delay={0} reduced={reduced}>
			<DiagramCard title='Agent' icon={<CyclingAgentLogo size='h-9 w-9' />} accent />
		</CrossCard>
		<CrossCard left='83.333%' top='50%' width='23%' delay={0.22} reduced={reduced}>
			<DiagramCard {...SATELLITES.sandbox} />
		</CrossCard>
		<CrossCard left='50%' top='83.333%' width='23%' delay={0.28} reduced={reduced}>
			<DiagramCard {...SATELLITES.orchestration} />
		</CrossCard>
	</div>
);

const StackRow = ({ title, subtitle, icon, accent }: { title: string; subtitle?: string; icon: ReactNode; accent?: boolean }) => (
	<div className={`flex items-center gap-3 rounded-2xl border bg-white/70 p-3 ${accent ? 'border-accent/40 bg-accent/[0.06] ring-1 ring-accent/15' : 'border-ink/10'}`}>
		<div className={`flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-xl ${accent ? 'bg-accent/10' : 'bg-ink/5'}`}>{icon}</div>
		<div className='min-w-0'>
			<div className={`text-sm font-medium ${accent ? 'text-accent-deep' : 'text-ink'}`}>{title}</div>
			{subtitle ? <div className='text-xs text-ink-faint'>{subtitle}</div> : null}
		</div>
	</div>
);

// Horizontal request/response branch peeling off the trunk into a satellite
// card. Mirrors the desktop cross: solid accent points INTO the card (request
// out), dashed ink points back toward the trunk (response in). Flow dots make
// the routing live, suppressed under reduced motion.
const Branch = ({ reduced }: { reduced: boolean | null }) => (
	<svg viewBox='0 0 28 16' className='h-4 w-7 flex-shrink-0 self-center' aria-hidden='true'>
		{/* request: out toward the card (accent, solid, arrowhead on the right) */}
		<path d='M2 5 L23 5' stroke={ACCENT} strokeWidth={1.4} strokeLinecap='round' />
		<path d='M20 2.5 L23.5 5 L20 7.5' fill='none' stroke={ACCENT} strokeWidth={1.4} strokeLinecap='round' strokeLinejoin='round' />
		{/* response: back toward the trunk (ink, dashed, arrowhead on the left) */}
		<path d='M25 11 L4 11' stroke={ARROW_COLOR} strokeWidth={1.3} strokeLinecap='round' strokeDasharray='3 3' />
		<path d='M7 8.5 L3.5 11 L7 13.5' fill='none' stroke={ARROW_COLOR} strokeWidth={1.3} strokeLinecap='round' strokeLinejoin='round' />
		{!reduced && (
			<>
				<motion.circle
					r={1.6}
					fill={ACCENT}
					initial={{ cx: 2, cy: 5, opacity: 0 }}
					animate={{ cx: [2, 23], cy: 5, opacity: [0, 1, 1, 0] }}
					transition={{ duration: 1.4, repeat: Infinity, ease: 'easeInOut', repeatDelay: 0.6 }}
				/>
				<motion.circle
					r={1.5}
					fill={ARROW_COLOR}
					initial={{ cx: 25, cy: 11, opacity: 0 }}
					animate={{ cx: [25, 4], cy: 11, opacity: [0, 1, 1, 0] }}
					transition={{ duration: 1.4, repeat: Infinity, ease: 'easeInOut', delay: 0.7, repeatDelay: 0.6 }}
				/>
			</>
		)}
	</svg>
);

// One satellite hung off the trunk: [trunk rail][branch][full-width card]. The
// rail draws the shared vertical trunk segment; isLast trims it to half height
// so the trunk terminates at the last branch instead of dangling onward.
const SpokeRow = ({
	title,
	subtitle,
	icon,
	isLast,
	reduced,
}: {
	title: string;
	subtitle?: string;
	icon: ReactNode;
	isLast?: boolean;
	reduced: boolean | null;
}) => (
	<div className={`flex items-stretch ${isLast ? '' : 'pb-2'}`}>
		<div className='relative w-3 flex-shrink-0' aria-hidden='true'>
			{/* Trunk segment. Non-last rows extend 0.5rem past their box to bridge
			    the pb-2 gap into the next row, so the spine reads as one continuous
			    line; the last row stops at half height so the trunk terminates at
			    the final branch instead of dangling onward. */}
			<span className={`absolute left-1/2 top-0 w-px -translate-x-1/2 bg-ink-faint/40 ${isLast ? 'h-1/2' : 'h-[calc(100%+0.5rem)]'}`} />
		</div>
		<Branch reduced={reduced} />
		<div className='min-w-0 flex-1'>
			<StackRow title={title} subtitle={subtitle} icon={icon} />
		</div>
	</div>
);

// Mobile hub-and-spoke ("comb"): a single trunk descends from the Agent hub and
// four horizontal branches fan out to the satellites, mirroring the desktop
// cross — the agent brokers each service, not a linear pipeline.
const MobileStack = ({ reduced }: { reduced: boolean | null }) => (
	<div className='mx-auto flex w-full max-w-sm flex-col md:hidden'>
		<StackRow title='Agent' subtitle='any supported agent' icon={<CyclingAgentLogo size='h-7 w-7' />} accent />
		{/* trunk stub: the spine visibly descends out of the agent hub. Its height
		    is the same 0.5rem gap that pb-2 puts between the satellite cards, so the
		    agent sits the same distance above Tools as the rest of the stack. */}
		<div className='flex h-2' aria-hidden='true'>
			<div className='relative w-3 flex-shrink-0'>
				<span className='absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-ink-faint/40' />
			</div>
		</div>
		<SpokeRow {...SATELLITES.tools} reduced={reduced} />
		<SpokeRow {...SATELLITES.session} reduced={reduced} />
		<SpokeRow {...SATELLITES.sandbox} reduced={reduced} />
		<SpokeRow {...SATELLITES.orchestration} isLast reduced={reduced} />
	</div>
);

export const HarnessArchitecture = ({ footer }: { footer?: ReactNode }) => {
	const reduced = useReducedMotion();
	return (
		<div
			className='relative rounded-3xl border border-ink/10 bg-gradient-to-b from-white/45 to-white/5 p-4 sm:p-6'
			role='img'
			aria-label='agentOS architecture: the agent sits at the center of the OS, which routes requests and responses out to Tools and Resources over MCP, Session state, the Sandbox where code runs, and the Orchestration layer.'
		>
			<span className='absolute -top-3 left-5 inline-flex items-center gap-1.5 rounded-full border border-ink/10 bg-paper px-2.5 py-1 text-[11px] font-medium text-ink shadow-sm'>
				<img src={AGENTOS_MARK} alt='' aria-hidden='true' className='h-3.5 w-3.5' />
				agentOS
			</span>
			<DesktopCross reduced={reduced} />
			<MobileStack reduced={reduced} />
			{footer}
		</div>
	);
};
