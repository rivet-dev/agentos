'use client';

import { useId, useState, useEffect, useRef, useCallback, useMemo } from 'react';
import {
	ArrowRight,
	FolderOpen,
	Layers,
	Globe,
	Bot,
	ListChecks,
	Wrench,
	ExternalLink,
	Activity,
	HardDrive,
	Users,
	Workflow,
	ChevronLeft,
	ChevronRight,
	Copy,
	Check,
	X,
	ShieldCheck,
	Package,
	Server,
} from 'lucide-react';
import { AnimatePresence, motion, useMotionValue, useReducedMotion, useSpring, useTransform, type MotionValue } from 'framer-motion';
import { InkPanel } from '../editorial/InkPanel';
import { registry } from '../../../data/registry';
import { REGISTRY_ICONS } from '../../../data/registry-icons';
import { AGENT_PROMPT } from '../agentPrompt';
import { HERO_H1_CLASS, SectionHeading } from '../typography';
import { AgentOsTopologyCell, SandboxTopologyCell } from '../diagrams/TopologyCells';
import { ColdStartTimeline } from '../diagrams/ColdStartTimeline';
import { AgentSessionDemo } from '../diagrams/AgentSessionDemo';
import { Reveal } from '../motion';

// Premium porcelain card surface, shared by the architecture cards and the
// feature bento: a single top-down gradient, a hairline ring, an inset top
// edge-light, and a soft layered drop shadow. Hover deepens the shadow and
// lightens the ring with no transform, so it stays reduced-motion safe.
const CARD_SURFACE =
	'rounded-2xl bg-gradient-to-b from-white to-[#f9f9fa] ring-1 ring-ink/[0.08] ' +
	'shadow-[inset_0_1px_0_rgba(255,255,255,0.9),0_1px_2px_-1px_rgba(20,20,22,0.10),0_8px_24px_-14px_rgba(20,20,22,0.20)] ' +
	'transition-[box-shadow,--tw-ring-color] duration-300 ' +
	'hover:ring-ink/[0.14] hover:shadow-[inset_0_1px_0_rgba(255,255,255,0.95),0_2px_4px_-1px_rgba(20,20,22,0.12),0_12px_30px_-12px_rgba(20,20,22,0.26)] ' +
	'motion-reduce:transition-none';

interface HeroTabCode {
	key: string;
	fileName: string;
	code: string;
	highlightedCode: string;
}

interface AgentOSPageProps {
	heroTabs: HeroTabCode[];
}

// --- Animated agentOS Logo ---
interface AnimatedAgentOSLogoProps {
	className?: string;
	displayedAgent?: { src: string; name: string } | null;
	// Seconds for the main stroke-draw. Defaults to 3; pass a smaller value for
	// a faster intro (e.g. the v0.2 announcement card).
	drawDurationSec?: number;
}

export const AnimatedAgentOSLogo = ({ className, displayedAgent, drawDurationSec = 3 }: AnimatedAgentOSLogoProps) => {
	const containerRef = useRef<HTMLDivElement>(null);
	const [isReady, setIsReady] = useState(false);
	const osLayerRef = useRef<Element | null>(null);
	const agentImageRef = useRef<SVGImageElement | null>(null);

	useEffect(() => {
		const container = containerRef.current;
		if (!container) return;

		fetch('/images/agent-os/agentos-hero-logo-animated.svg')
			.then((res) => res.text())
			.then((svgText) => {
				container.innerHTML = svgText;

				const svg = container.querySelector('svg');
				if (!svg) return;

				svg.removeAttribute('width');
				svg.removeAttribute('height');
				svg.style.height = '100%';
				svg.style.width = 'auto';
				svg.style.display = 'block';

				const ns = 'http://www.w3.org/2000/svg';
				const textLayer = svg.querySelector('#text-layer');
				const strokeLayer = svg.querySelector('#stroke-layer');
				if (!textLayer || !strokeLayer) return;

				// Find and store reference to the OS layer (contains the "OS" text)
				const osLayer = svg.querySelector('#os-layer');
				if (osLayer) {
					osLayerRef.current = osLayer;
					// Set up transition for smooth opacity changes
					(osLayer as HTMLElement).style.transition = 'opacity 0.15s ease-out';
				}

				// Create agent image element inside the os-layer's parent, positioned like os-layer
				// The image will be positioned to appear inside the squircle where "OS" is
				const agentImg = document.createElementNS(ns, 'image');
				agentImg.setAttribute('id', 'agent-logo');
				// Position inside the squircle (viewBox is 0 0 305 102, squircle is on the right)
				agentImg.setAttribute('width', '32');
				agentImg.setAttribute('height', '32');
				agentImg.setAttribute('x', '249');
				agentImg.setAttribute('y', '25');
				agentImg.setAttribute('preserveAspectRatio', 'xMidYMid meet');
				agentImg.style.opacity = '0';
				agentImg.style.transition = 'opacity 0.15s ease-out';
				svg.appendChild(agentImg);
				agentImageRef.current = agentImg;

				const strokePath = strokeLayer.querySelector('path');
				if (!strokePath) return;

				const strokeStyle =
					'fill:none; stroke:white; stroke-width:10.57px; stroke-linecap:round; stroke-linejoin:round;';

				// Split the path data into main path and short tail path
				const fullD = strokePath.getAttribute('d') || '';
				const lastM = fullD.lastIndexOf('M');
				const mainD = fullD.substring(0, lastM);
				const tailD = fullD.substring(lastM);

				// Create mask
				const defs = document.createElementNS(ns, 'defs');
				svg.insertBefore(defs, svg.firstChild);

				const mask = document.createElementNS(ns, 'mask');
				mask.setAttribute('id', 'reveal-mask');
				mask.setAttribute('maskUnits', 'userSpaceOnUse');
				mask.setAttribute('x', '0');
				mask.setAttribute('y', '0');
				mask.setAttribute('width', '99999');
				mask.setAttribute('height', '99999');

				// Clone the stroke group transform wrapper for both paths
				const groupTransform = strokeLayer.getAttribute('transform') || '';

				// Main path
				const mainGroup = document.createElementNS(ns, 'g');
				mainGroup.setAttribute('transform', groupTransform);
				const mainPath = document.createElementNS(ns, 'path');
				mainPath.setAttribute('d', mainD);
				mainPath.setAttribute('style', strokeStyle);
				mainGroup.appendChild(mainPath);
				mask.appendChild(mainGroup);

				// Tail path
				const tailGroup = document.createElementNS(ns, 'g');
				tailGroup.setAttribute('transform', groupTransform);
				const tailPath = document.createElementNS(ns, 'path');
				tailPath.setAttribute('d', tailD);
				tailPath.setAttribute('style', strokeStyle);
				tailGroup.appendChild(tailPath);
				mask.appendChild(tailGroup);

				defs.appendChild(mask);

				// Wrap text layer in a masked group
				const parent = textLayer.parentNode;
				if (parent) {
					const wrapper = document.createElementNS(ns, 'g');
					wrapper.setAttribute('mask', 'url(#reveal-mask)');
					parent.insertBefore(wrapper, textLayer);
					wrapper.appendChild(textLayer);
				}

				// Remove the original stroke layer
				strokeLayer.remove();

				// Measure path lengths
				const mainLength = mainPath.getTotalLength();
				const tailLength = tailPath.getTotalLength();

				// Set up dash offsets (hidden initially)
				mainPath.style.strokeDasharray = String(mainLength);
				mainPath.style.strokeDashoffset = String(mainLength);
				tailPath.style.strokeDasharray = String(tailLength);
				tailPath.style.strokeDashoffset = String(tailLength);

				// Animate: main path first, then tail after main finishes
				const mainDuration = drawDurationSec;
				const tailDuration = 0.3;

				// Add keyframes if not already present
				if (!document.querySelector('#agentos-logo-animation-style')) {
					const style = document.createElement('style');
					style.id = 'agentos-logo-animation-style';
					style.textContent = `
						@keyframes reveal-main {
							to { stroke-dashoffset: 0; }
						}
						@keyframes reveal-tail {
							to { stroke-dashoffset: 0; }
						}
					`;
					document.head.appendChild(style);
				}

				mainPath.style.animation = `reveal-main ${mainDuration}s ease forwards`;
				tailPath.style.animation = `reveal-tail ${tailDuration}s ease ${mainDuration}s forwards`;

				setIsReady(true);
			});

		return () => {
			if (container) {
				container.innerHTML = '';
			}
		};
	}, []);

	// Update OS layer and agent image visibility when displayedAgent changes
	useEffect(() => {
		if (!isReady) return;

		const osLayer = osLayerRef.current;
		const agentImg = agentImageRef.current;

		if (osLayer && agentImg) {
			if (displayedAgent) {
				// Hide OS layer, show agent logo
				(osLayer as HTMLElement).style.opacity = '0';
				agentImg.setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', displayedAgent.src);
				agentImg.setAttribute('href', displayedAgent.src);
				agentImg.style.opacity = '1';
			} else {
				// Show OS layer, hide agent logo
				(osLayer as HTMLElement).style.opacity = '1';
				agentImg.style.opacity = '0';
			}
		}
	}, [displayedAgent, isReady]);

	return (
		<div
			ref={containerRef}
			className={className}
			style={{
				opacity: isReady ? 1 : 0,
				transition: 'opacity 0.3s ease',
			}}
		/>
	);
};

// --- Set up with your agent (copies the agent prompt) ---
const SetupWithAgent = () => {
	const [copied, setCopied] = useState(false);

	const handleCopy = async () => {
		await navigator.clipboard.writeText(AGENT_PROMPT);
		setCopied(true);
		setTimeout(() => setCopied(false), 2000);
	};

	return (
		<button
			onClick={handleCopy}
			className='selection-dark inline-flex w-full items-center justify-center gap-2 whitespace-nowrap rounded-md bg-accent-deep px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent sm:w-auto'
		>
			{copied ? <Check className='h-4 w-4' /> : <Copy className='h-4 w-4' />}
			{/* Reserve the width of the longest label so the button doesn't shrink on copy */}
			<span className='grid place-items-center'>
				<span className='invisible col-start-1 row-start-1' aria-hidden='true'>Set up with your agent</span>
				<span className='col-start-1 row-start-1'>{copied ? 'Copied' : 'Set up with your agent'}</span>
			</span>
		</button>
	);
};

// --- Hero Tabs (scrollable with fade + arrows) ---
interface HeroTabEntry {
	key: string;
	icon?: typeof Bot;
	iconSrc?: string;
	label: string;
	docsHref: string;
	fileName?: string;
	code?: string;
	highlightedCode?: string;
}

const HeroTabs = ({ tabs, activeTab, onTabChange }: { tabs: HeroTabEntry[]; activeTab: number; onTabChange: (idx: number) => void }) => {
	const scrollRef = useRef<HTMLDivElement>(null);
	const [canScrollLeft, setCanScrollLeft] = useState(false);
	const [canScrollRight, setCanScrollRight] = useState(false);
	// The page renders more than one tab strip; a per-instance layoutId keeps
	// each strip's active-pill animation from jumping to the other strip.
	const indicatorLayoutId = useId();

	const checkOverflow = useCallback(() => {
		const el = scrollRef.current;
		if (!el) return;
		setCanScrollLeft(el.scrollLeft > 2);
		setCanScrollRight(el.scrollLeft + el.clientWidth < el.scrollWidth - 2);
	}, []);

	useEffect(() => {
		const el = scrollRef.current;
		if (!el) return;
		checkOverflow();
		el.addEventListener('scroll', checkOverflow, { passive: true });
		const ro = new ResizeObserver(checkOverflow);
		ro.observe(el);
		return () => {
			el.removeEventListener('scroll', checkOverflow);
			ro.disconnect();
		};
	}, [checkOverflow]);

	const scroll = (direction: 'left' | 'right') => {
		const el = scrollRef.current;
		if (!el) return;
		const amount = el.clientWidth * 0.5;
		el.scrollBy({ left: direction === 'left' ? -amount : amount, behavior: 'smooth' });
	};

	// Fade the tab strip into the porcelain with a content mask rather than a
	// solid-color overlay. A flat overlay would cover the page grain and read as
	// an odd lighter band; masking the content lets the grain show through.
	const maskImage =
		canScrollLeft || canScrollRight
			? `linear-gradient(to right, ${canScrollLeft ? 'transparent 0, #000 6rem' : '#000 0'}, ${canScrollRight ? '#000 calc(100% - 6rem), transparent 100%' : '#000 100%'})`
			: undefined;
	const maskStyle = maskImage ? { maskImage, WebkitMaskImage: maskImage } : undefined;

	return (
		<div className='relative mb-4 overflow-hidden'>
			{/* Left fade + arrow */}
			{canScrollLeft && (
				<div
					className='pointer-events-none absolute inset-y-0 left-0 z-20 flex w-12 items-center justify-start'
				>
					<button
						type='button'
						onClick={() => scroll('left')}
						className='pointer-events-auto ml-1 flex h-8 w-8 items-center justify-center rounded-full text-ink-faint hover:text-ink'
						aria-label='Scroll tabs left'
					>
						<ChevronLeft className='h-4 w-4' />
					</button>
				</div>
			)}

			{/* Scrollable tabs */}
			<div ref={scrollRef} className='scrollbar-hide overflow-x-auto' style={maskStyle}>
				<div className='flex min-w-max flex-nowrap items-center justify-start gap-1'>
					{tabs.map((tab, idx) => {
						const LucideIcon = tab.icon;
						return (
							<button
								key={tab.label}
								type='button'
								onClick={() => onTabChange(idx)}
								className='relative inline-flex shrink-0 items-center gap-2 whitespace-nowrap rounded-lg px-3 py-1.5 font-sans text-xs transition-colors md:px-4'
							>
								{activeTab === idx && (
									<motion.div
										layoutId={indicatorLayoutId}
										className='absolute inset-0 rounded-lg bg-ink/[0.07]'
										transition={{ type: 'spring', bounce: 0.2, duration: 0.4 }}
									/>
								)}
								<span className={`relative z-10 flex items-center gap-2 ${activeTab === idx ? 'font-medium text-ink' : 'text-ink-soft hover:text-ink'}`}>
									{tab.iconSrc ? (
										<img src={tab.iconSrc} alt='' aria-hidden='true' className='h-4 w-4 object-contain' />
									) : LucideIcon ? (
										<LucideIcon className='h-4 w-4' />
									) : null}
									{tab.label}
								</span>
							</button>
						);
					})}
				</div>
			</div>

			{/* Right fade + arrow */}
			{canScrollRight && (
				<div
					className='pointer-events-none absolute inset-y-0 right-0 z-20 flex w-12 items-center justify-end'
				>
					<button
						type='button'
						onClick={() => scroll('right')}
						className='pointer-events-auto mr-1 flex h-8 w-8 items-center justify-center rounded-full text-ink-faint hover:text-ink'
						aria-label='Scroll tabs right'
					>
						<ChevronRight className='h-4 w-4' />
					</button>
				</div>
			)}
		</div>
	);
};

// --- Hero ---
const agents = [
	{ src: '/images/agent-logos/pi.svg', name: 'Pi' },
	{ src: '/images/agent-logos/claude-code.svg', name: 'Claude Code' },
	{ src: '/images/agent-logos/codex.svg', name: 'Codex' },
	{ src: '/images/agent-logos/opencode.svg', name: 'OpenCode' },
];

// Frameworks agentOS works with, shown as a quiet second works-with row under
// the harness stack. Eve's mark is its wordmark, so its chip renders the logo
// alone; the others pair a square mark with the name.
const frameworks = [
	{ src: '/images/frameworks/eve.svg', name: 'Eve', wordmark: true, href: 'https://vercel.com/eve' },
	{ src: '/images/frameworks/flue.svg', name: 'Flue', href: 'https://flueframework.com' },
	{ src: '/rivet-icon.svg', name: 'RivetKit', href: 'https://rivet.dev' },
];
// Tab metadata for the orchestration code panel, leading with agents
// coordinating other agents. Filters the highlighted-snippet array passed
// from index.astro by key. (The execution section renders recorded agent
// sessions instead; see AgentSessionDemo.)
interface HeroTabMeta {
	key: string;
	icon?: typeof Bot;
	iconSrc?: string;
	label: string;
	docsHref: string;
}

const orchestrationTabMeta: HeroTabMeta[] = [
	{ key: 'agents', icon: Bot, label: 'Agents', docsHref: '/docs/sessions' },
	{ key: 'agent-agent', icon: Layers, label: 'Agent-Agent', docsHref: '/docs/agent-to-agent' },
	{ key: 'workflows', icon: Workflow, label: 'Workflows', docsHref: '/docs/workflows' },
	{ key: 'multiplayer', icon: Users, label: 'Multiplayer', docsHref: '/docs/multiplayer' },
	{ key: 'tools', icon: Wrench, label: 'Bindings', docsHref: '/docs/bindings' },
	{ key: 'permissions', icon: ShieldCheck, label: 'Permissions', docsHref: '/docs/permissions' },
];

// Joins tab metadata with the highlighted snippets rendered at Astro build
// time, dropping any tab whose snippet is missing.
const joinTabs = (meta: HeroTabMeta[], heroTabs: HeroTabCode[]) =>
	meta.flatMap((tab) => {
		const snippet = heroTabs.find((heroTab) => heroTab.key === tab.key);
		return snippet ? [{ ...tab, ...snippet }] : [];
	});

const Hero = () => {
	const [hoveredAgent, setHoveredAgent] = useState<{ src: string; name: string } | null>(null);
	const [autoPlayAgent, setAutoPlayAgent] = useState<{ src: string; name: string } | null>(null);
	const [autoPlayComplete, setAutoPlayComplete] = useState(false);

	// Highlight stats — best-case "up to" figures, sourced from bench.ts.
	// Figures match the benchmark section's default view (the shell/execution
	// workload, p50, AWS ARM): the chart a chip links to must show the same
	// number the chip claims.
	const heroStats = [
		{ value: `${Math.round(benchColdStart[0].sandbox / benchColdStart[0].agentOS)}×`, label: 'faster cold starts', sub: `p50 · vs. ${SANDBOX_COLDSTART_PROVIDER}`, href: '#bench-cold-start' },
		{ value: `${benchWorkloads.shell.memory.multiplier.split('x')[0]}×`, label: 'less memory', sub: 'vs. 1 GiB sandbox minimum', href: '#bench-memory' },
		{ value: `${benchWorkloads.shell.cost.find((tier) => tier.label === 'AWS ARM')?.ratio ?? Math.min(...benchWorkloads.shell.cost.map((tier) => tier.ratio))}×`, label: 'cheaper to run', sub: `self-hosted vs. ${SANDBOX_COST_PROVIDER}`, href: '#bench-cost' },
	];

	// Auto-cycle through agents starting 2.5s before stroke animation ends
	useEffect(() => {
		const logoAnimationDuration = 800; // Start cycling 2.5s before the 3.3s animation ends
		const agentDisplayDuration = 400; // Time to show each agent

		const startAutoPlay = setTimeout(() => {
			let currentIndex = 0;

			const cycleAgents = () => {
				if (currentIndex < agents.length) {
					setAutoPlayAgent(agents[currentIndex]);
					currentIndex++;
					setTimeout(cycleAgents, agentDisplayDuration);
				} else {
					// End on OS (null)
					setAutoPlayAgent(null);
					setAutoPlayComplete(true);
				}
			};

			cycleAgents();
		}, logoAnimationDuration);

		return () => clearTimeout(startAutoPlay);
	}, []);

	// Displayed agent is either hovered (if autoplay complete) or autoplay agent
	const displayedAgent = autoPlayComplete ? hoveredAgent : autoPlayAgent;

	return (
		// 92svh, not full height: the code panel's top edge should enter the first
		// viewport as a visible invitation to scroll rather than a stray sliver.
		<section className='relative flex min-h-[92svh] flex-col justify-center px-6 pt-28 pb-8 md:pt-32'>
			<div className='mx-auto flex w-full max-w-3xl flex-col items-center text-center'>
				{/* Centered single-column hero */}
				<div className='flex w-full flex-col items-center text-center'>
					{/* Title */}
					<motion.div
						id='hero-logo'
						initial={{ opacity: 0, y: 20 }}
						animate={{ opacity: 1, y: 0 }}
						transition={{ duration: 0.5, delay: 0.05 }}
						className='mb-7 flex'
					>
						<AnimatedAgentOSLogo className='h-11 w-auto md:h-12' displayedAgent={displayedAgent} />
					</motion.div>

					{/* Headline */}
					<motion.h1
						initial={{ opacity: 0, y: 20 }}
						animate={{ opacity: 1, y: 0 }}
						transition={{ duration: 0.5, delay: 0.1 }}
						className={`mb-4 max-w-2xl ${HERO_H1_CLASS}`}
					>
						The virtual operating system for agents.
					</motion.h1>

					{/* Description */}
					<motion.p
						initial={{ opacity: 0, y: 20 }}
						animate={{ opacity: 1, y: 0 }}
						transition={{ duration: 0.5, delay: 0.13 }}
						className='mb-7 max-w-xl text-base leading-relaxed text-ink-soft md:text-lg'
					>
						An isolated operating system for every agent, embedded in the backend you already
						run: a file system, code execution, tools, and human review.
					</motion.p>

					{/* Benchmark highlights — proof for "faster, lighter, cheaper", linked to the benchmarks below */}
					<motion.div
						initial={{ opacity: 0, y: 10 }}
						animate={{ opacity: 1, y: 0 }}
						transition={{ duration: 0.5, delay: 0.16 }}
						className='mb-8 flex flex-wrap items-start justify-center gap-x-8 gap-y-3'
					>
						{heroStats.map((stat) => (
							<a
								key={stat.label}
								href={stat.href}
								aria-label={`${stat.value} ${stat.label} (${stat.sub}) — jump to the benchmark`}
								className='group inline-flex flex-col items-center gap-0.5'
							>
								<span className='inline-flex items-baseline gap-1.5'>
									<span className='text-xl font-medium text-pine md:text-2xl'>{stat.value}</span>
									<span className='text-sm text-ink-soft transition-colors group-hover:text-ink md:text-base'>{stat.label}</span>
								</span>
								<span className='font-mono text-[11px] text-ink-faint'>{stat.sub}</span>
							</a>
						))}
					</motion.div>

					{/* Buttons */}
					<motion.div
						initial={{ opacity: 0, y: 20 }}
						animate={{ opacity: 1, y: 0 }}
						transition={{ duration: 0.5, delay: 0.19 }}
						className='flex w-full flex-col flex-wrap items-center gap-x-4 gap-y-3 sm:flex-row sm:justify-center'
					>
						<SetupWithAgent />
						<a
							href='/docs'
							className='inline-flex w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-ink/20 bg-white px-4 py-2 text-sm font-medium text-ink transition-colors hover:border-ink/40 hover:bg-ink/[0.02] sm:w-auto'
						>
							Read the Docs
							<ArrowRight className='h-4 w-4' />
						</a>
					</motion.div>

					{/* Works with — supported agent harnesses, then frameworks */}
					<motion.div
						initial={{ opacity: 0, y: 20 }}
						animate={{ opacity: 1, y: 0 }}
						transition={{ duration: 0.5, delay: 0.22 }}
						className='mt-7 flex flex-wrap items-center justify-center gap-3'
					>
						<span className='inline-flex items-center gap-2 font-mono text-sm text-ink-faint'>
							runs
						</span>
						{/* Overlapping logo stack: a casually rotated pile (~30° fan) that
						    spreads out and straightens on hover. Hovering a chip still drives
						    the hero logo swap; the tile's title shows the agent name. */}
						<motion.div
							className='flex items-center pl-1.5'
							initial='rest'
							whileHover='spread'
							animate='rest'
							onMouseLeave={() => autoPlayComplete && setHoveredAgent(null)}
						>
							{agents.map((agent, i) => {
								const tilt = [-16, -8, 0, 9, 17][i] ?? 0;
								return (
									<motion.div
										key={agent.name}
										onMouseEnter={() => autoPlayComplete && setHoveredAgent(agent)}
										variants={{
											rest: { rotate: tilt, marginLeft: i === 0 ? 0 : -10 },
											spread: { rotate: 0, marginLeft: i === 0 ? 0 : 4 },
										}}
										transition={{ duration: 0.3, ease: [0.22, 1, 0.36, 1] }}
										style={{ zIndex: i }}
										className='group relative flex h-9 w-9 cursor-pointer items-center justify-center rounded-xl border border-ink/10 bg-white shadow-[0_2px_6px_-1px_rgba(20,20,22,0.12)]'
									>
										<img src={agent.src} alt={agent.name} className='h-5 w-5 object-contain' />
										{/* Name, revealed below this card on hover */}
										<span className='pointer-events-none absolute left-1/2 top-full mt-2 -translate-x-1/2 whitespace-nowrap text-xs font-medium text-ink-soft opacity-0 transition-opacity duration-150 group-hover:opacity-100'>
											{agent.name}
										</span>
									</motion.div>
								);
							})}
						</motion.div>

						{/* Frameworks agentOS works with: same fanned-stack treatment as the
						    harness logos, names revealed below each tile on hover. */}
						<span aria-hidden='true' className='font-mono text-sm text-ink-faint'>
							&amp;
						</span>
						<motion.div className='flex items-center pl-1.5' initial='rest' whileHover='spread' animate='rest'>
							{frameworks.map((framework, i) => {
								const tilt = [-14, 6, 15][i] ?? 0;
								return (
									<motion.a
										key={framework.name}
										href={framework.href}
										target='_blank'
										rel='noopener noreferrer'
										aria-label={framework.name}
										variants={{
											rest: { rotate: tilt, marginLeft: i === 0 ? 0 : -10 },
											spread: { rotate: 0, marginLeft: i === 0 ? 0 : 4 },
										}}
										transition={{ duration: 0.3, ease: [0.22, 1, 0.36, 1] }}
										style={{ zIndex: i }}
										className='group relative flex h-9 w-9 cursor-pointer items-center justify-center rounded-xl border border-ink/10 bg-white shadow-[0_2px_6px_-1px_rgba(20,20,22,0.12)]'
									>
										<img
											src={framework.src}
											alt={framework.name}
											className={framework.wordmark ? 'w-6 object-contain' : 'h-5 w-5 object-contain'}
										/>
										{/* Name, revealed below this card on hover */}
										<span className='pointer-events-none absolute left-1/2 top-full mt-2 -translate-x-1/2 whitespace-nowrap text-xs font-medium text-ink-soft opacity-0 transition-opacity duration-150 group-hover:opacity-100'>
											{framework.name}
										</span>
									</motion.a>
								);
							})}
						</motion.div>
					</motion.div>
				</div>
			</div>
		</section>
	);
};


// --- Code Panel (tab strip + window chrome, shared by both code sections) ---
const CodePanel = ({ tabs }: { tabs: HeroTabEntry[] }) => {
	const [activeTab, setActiveTab] = useState(0);

	return (
		<div>
			<HeroTabs tabs={tabs} activeTab={activeTab} onTabChange={setActiveTab} />

			<div className='overflow-hidden rounded-xl border border-zinc-200 bg-zinc-50'>
				<div className='flex items-center gap-2 border-b border-zinc-200 px-4 py-3'>
					<div className='h-3 w-3 rounded-full bg-zinc-200' />
					<div className='h-3 w-3 rounded-full bg-zinc-200' />
					<div className='h-3 w-3 rounded-full bg-zinc-200' />
					<span className='ml-2 text-xs text-zinc-600'>{tabs[activeTab]?.fileName ?? 'index.ts'}</span>
				</div>
				<div className='relative h-[420px] overflow-y-auto'>
					<AnimatePresence mode='wait'>
						<motion.div
							key={activeTab}
							initial={{ opacity: 0 }}
							animate={{ opacity: 1 }}
							exit={{ opacity: 0 }}
							transition={{ duration: 0.2 }}
							className='overflow-x-auto p-6 font-code text-sm leading-relaxed text-zinc-600 [&_.line]:break-all [&_.shiki]:!m-0 [&_.shiki]:!bg-transparent [&_.shiki]:!p-0 [&_.shiki]:font-code [&_.shiki]:text-sm [&_.shiki]:leading-relaxed [&_pre]:whitespace-pre-wrap'
						>
							<span
								className='not-prose code'
								// biome-ignore lint/security/noDangerouslySetInnerHtml: generated at Astro render time
								dangerouslySetInnerHTML={{ __html: tabs[activeTab]?.highlightedCode ?? '' }}
							/>
						</motion.div>
					</AnimatePresence>
				</div>
			</div>
		</div>
	);
};

// --- Orchestration (heading + code showcase + capability row) ---
// A single row of four tiles under the code; the OS-primitive tiles
// (integrations, human-in-the-loop, persistence) live in the OS section.
const orchestrationFeatures = [
	{ icon: Users, title: 'Multiplayer', description: 'Multiple humans and agents in one live session, collaborating in realtime.', docsHref: '/docs/multiplayer' },
	{ icon: ListChecks, title: 'Durable sessions', description: 'Every run is a managed session with its own state, history, and lifecycle. Pause, resume, and replay.', docsHref: '/docs/sessions' },
	{ icon: Workflow, title: 'Workflows', description: 'Chain durable, multi-step workflows that survive restarts and pick up exactly where they left off.', docsHref: '/docs/workflows' },
	{ icon: Activity, title: 'Observability', description: 'Stream every event, tool call, and state change for full visibility into what your agents are doing.', docsHref: '/docs/sessions#stream-responses' },
];

const OrchestrationSection = ({ heroTabs }: { heroTabs: HeroTabCode[] }) => (
	<section className='border-t border-ink/10 px-6 py-16 md:py-32'>
		<div className='mx-auto max-w-7xl'>
			<Reveal>
				<SectionHeading
					title='Orchestrate fleets of agents in a few lines of code.'
					subtitle='Durable sessions, multiplayer, and agent-to-agent delegation are built in, and every event streams back to your code. It works with the frameworks you already use, including Eve and Flue.'
					className='mb-10 max-w-3xl md:mb-12'
				/>
			</Reveal>
			<Reveal>
				<CodePanel tabs={joinTabs(orchestrationTabMeta, heroTabs)} />
			</Reveal>
			<div className='mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:auto-rows-fr lg:grid-cols-4'>
				{orchestrationFeatures.map((feature) => (
					<CapabilityCard key={feature.title} {...feature} />
				))}
			</div>
		</div>
	</section>
);

// --- Any execution layer (runtimes + recorded agent session) ---
// Three shipped runtimes, each grounded in its docs page. The full-Linux story
// stays in the argument section's sandbox-mounting footnote.
const executionLayers = [
	{
		imgSrc: '/images/registry/nodejs.svg',
		title: 'JavaScript & TypeScript',
		description: 'Node v22 compatible on native V8 isolates. node, npm, and npx on the PATH at full JIT speed.',
		docsHref: '/docs/nodejs-runtime',
	},
	{
		imgSrc: '/images/registry/python.svg',
		title: 'Python',
		description: 'CPython 3.13 with pip. Native wheels like numpy and pandas work.',
		docsHref: '/docs/python-runtime',
	},
	{
		imgSrc: '/images/registry/linux.svg',
		title: 'Linux shell',
		description: 'A POSIX userland with a process table, PTYs, TCP and UDP with DNS, and deny-by-default permissions.',
		docsHref: '/docs/architecture',
	},
];

const ExecutionLayersSection = () => (
	<section className='border-t border-ink/10 px-6 py-16 md:py-32'>
		<div className='mx-auto max-w-7xl'>
			<Reveal>
				<SectionHeading
					title='Any execution layer.'
					subtitle='Agents write JavaScript, Python, and shell. agentOS runs all three natively behind one exec API, so an agent can script its work in JavaScript as easily as in bash.'
					className='mb-10 max-w-3xl md:mb-12'
				/>
			</Reveal>
			<div className='mb-10 grid grid-cols-1 gap-4 sm:grid-cols-3'>
				{executionLayers.map((layer) => (
					<CapabilityCard key={layer.title} {...layer} />
				))}
			</div>
			{/* Recorded agent sessions instead of SDK snippets: each runtime tab
			    replays the same task with the agent writing that language, so the
			    demo shows an agent using the exec API rather than client code
			    calling it. */}
			<Reveal>
				<AgentSessionDemo />
			</Reveal>
		</div>
	</section>
);


// Magnetic Docs link: on hover it grows slightly and drifts toward the cursor
// (~0.3× the pointer's offset from the link's center), spring-eased back on leave.
const DocsLink = ({ href }: { href: string }) => {
	const reduce = useReducedMotion() ?? false;
	const ref = useRef<HTMLAnchorElement>(null);
	// Drive the CSS `translate`/`scale` transform properties directly so the
	// browser compositor interpolates them (smooth, GPU). They're independent
	// properties, so the drift can ease slowly while the zoom stays snappy.
	const handleMove = useCallback((e: React.MouseEvent<HTMLAnchorElement>) => {
		const el = ref.current;
		if (!el) return;
		const rect = el.getBoundingClientRect();
		const x = (e.clientX - (rect.left + rect.width / 2)) * 0.15;
		const y = (e.clientY - (rect.top + rect.height / 2)) * 0.15;
		el.style.translate = `${x}px ${y}px`;
		el.style.scale = '1.02';
	}, []);
	const handleLeave = useCallback(() => {
		const el = ref.current;
		if (!el) return;
		el.style.translate = '0px 0px';
		el.style.scale = '1';
	}, []);
	return (
		// Padding + matching negative margin enlarge the hover/hit area without
		// shifting layout, so the magnet engages well before the cursor reaches
		// the text. Slow ease on translate, snappy on scale.
		<a
			ref={ref}
			href={href}
			onMouseMove={reduce ? undefined : handleMove}
			onMouseLeave={reduce ? undefined : handleLeave}
			style={
				reduce
					? undefined
					: {
							transition:
								'translate 700ms cubic-bezier(0.22,1,0.36,1), scale 180ms ease-out, color 150ms ease',
							willChange: 'translate, scale',
						}
			}
			className='-mx-5 -my-4 inline-flex w-fit items-center gap-1 px-5 py-4 text-sm text-ink-soft hover:text-ink'
		>
			Docs <span aria-hidden='true'>→</span>
		</a>
	);
};

// --- Capability Card (shared by the OS grid and the orchestration bento) ---
// `span` drives bento placement (e.g. 'lg:col-span-2 lg:row-span-2'); `featured`
// scales up the icon, title, and copy so the larger tile reads as the headline.
const CapabilityCard = ({
	icon: Icon,
	imgSrc,
	title,
	description,
	docsHref,
	span,
	featured,
	noIcon,
}: {
	icon?: React.ComponentType<{ className?: string }>;
	imgSrc?: string;
	title: string;
	description: string;
	docsHref?: string;
	span?: string;
	featured?: boolean;
	noIcon?: boolean;
}) => (
	<motion.div
		initial={{ opacity: 0, y: 20 }}
		whileInView={{ opacity: 1, y: 0 }}
		viewport={{ once: true }}
		transition={{ duration: 0.5 }}
		className={`group flex flex-col p-6 ${CARD_SURFACE} ${span ?? ''}`}
	>
		{!noIcon && (imgSrc || Icon) && (
			<div className={`mb-4 flex items-center justify-center rounded-xl bg-ink/5 ${featured ? 'h-14 w-14' : 'h-12 w-12'}`}>
				{imgSrc ? (
					<img src={imgSrc} alt='' aria-hidden='true' className={featured ? 'h-6 w-6 object-contain' : 'h-5 w-5 object-contain'} />
				) : Icon ? (
					<Icon className={featured ? 'h-6 w-6 text-ink-soft' : 'h-5 w-5 text-ink-soft'} />
				) : null}
			</div>
		)}
		<h3 className={`mb-2 font-medium tracking-[-0.015em] text-ink ${featured ? 'text-2xl md:text-3xl' : 'text-base'}`}>{title}</h3>
		<p className={`leading-relaxed text-ink-soft ${featured ? 'max-w-md text-base md:text-lg' : 'text-sm'}`}>{description}</p>
		{docsHref && (
			<div className='mt-auto pt-4'>
				<DocsLink href={docsHref} />
			</div>
		)}
	</motion.div>
);

// --- Operating System section (the OS primitives agents actually use) ---
// Maps the OS-needs list one-to-one: persistence, memory, integrations,
// human-in-the-loop, and state previews, with execution covered by its own
// section. Each tile links to an existing docs page.
// Bento layout: "Any agent harness" leads as the headline tile. The span
// pattern (one 2×2 + two 1×1 + three col-span-2) tiles a 4-column grid exactly
// with `grid-flow-row-dense`.
const osFeatures = [
	{ icon: Bot, title: 'Any agent harness', description: 'Claude Code, Codex, OpenCode, and Pi all run behind one unified API. Swap or add agents without touching your infrastructure.', docsHref: '/docs/sessions', featured: true, span: 'lg:col-span-2 lg:row-span-2' },
	{ icon: FolderOpen, title: 'Persistent file system', description: 'Mount a host directory, S3, Google Drive, or a custom JavaScript file system. State survives sleep and wake.', docsHref: '/docs/filesystem' },
	{ icon: Globe, title: 'State previews', description: 'Every app the agent runs gets its own unique preview URL, served straight from the VM.', docsHref: '/docs/networking' },
	{ icon: Wrench, title: 'Integrations', description: 'Bindings turn your JavaScript functions into CLI commands inside the VM. Connect MCP servers, skills, and other agents.', docsHref: '/docs/bindings', span: 'lg:col-span-2' },
	{ icon: ShieldCheck, title: 'Human in the loop', description: 'Each tool call raises a permission request your app approves or denies, from a client UI or a server-side hook. Unanswered requests reject after 120 seconds, and interactive PTYs let a person take over.', docsHref: '/docs/approvals', span: 'lg:col-span-2' },
	{ icon: HardDrive, title: 'Memory', description: 'Sessions persist automatically with replayable transcripts, and sqlite3 runs inside the VM for structured agent memory.', docsHref: '/docs/persistence', span: 'lg:col-span-2' },
];

// --- Floating agent logos for the featured "Any agent harness" tile ---
// Each tile idles with a gentle bob and drifts with the cursor (parallax) while
// the card is hovered. `depth` varies per tile so nearer logos move further.
const FLOATING_AGENTS = [
	{ src: '/images/registry/claude-code.svg', label: 'Claude Code', left: '75%', top: '9%', size: 84, depth: 13, float: 11, dur: 6.6, delay: 0, rot: -8 },
	{ src: '/images/registry/codex.svg', label: 'Codex', left: '83%', top: '43%', size: 64, depth: 17, float: 9, dur: 7.8, delay: 0.6, rot: 6 },
	{ src: '/images/registry/opencode.svg', label: 'OpenCode', left: '64%', top: '65%', size: 80, depth: 20, float: 13, dur: 7.1, delay: 1.2, rot: -5 },
	{ src: '/images/registry/pi.svg', label: 'PI', left: '16%', top: '59%', size: 74, depth: 23, float: 9, dur: 8.2, delay: 1.6, rot: 9 },
	{ src: '/images/frameworks/eve.svg', label: 'Eve', left: '40%', top: '78%', size: 68, depth: 15, float: 10, dur: 7.4, delay: 0.9, rot: 7 },
	{ src: '/images/frameworks/flue.svg', label: 'Flue', left: '90%', top: '74%', size: 58, depth: 21, float: 12, dur: 6.9, delay: 1.9, rot: -7 },
] as const;

type FloatingAgent = (typeof FLOATING_AGENTS)[number];

const FloatingAgentTile = ({ agent, mx, my, reduce }: { agent: FloatingAgent; mx: MotionValue<number>; my: MotionValue<number>; reduce: boolean }) => {
	const x = useTransform(mx, (v) => v * agent.depth);
	const y = useTransform(my, (v) => v * agent.depth);
	const logo = Math.round(agent.size * 0.52);
	return (
		<motion.div className='absolute' style={reduce ? { left: agent.left, top: agent.top } : { left: agent.left, top: agent.top, x, y }}>
			<motion.div
				animate={reduce ? undefined : { y: [0, -agent.float, 0] }}
				transition={reduce ? undefined : { duration: agent.dur, delay: agent.delay, repeat: Infinity, ease: 'easeInOut' }}
				className='flex items-center justify-center rounded-2xl bg-gradient-to-b from-white to-[#f1f1f3] ring-1 ring-ink/10 shadow-[0_2px_6px_-1px_rgba(20,20,22,0.10),0_16px_34px_-12px_rgba(20,20,22,0.26)]'
				style={{ width: agent.size, height: agent.size, rotate: agent.rot }}
			>
				<img src={agent.src} alt={agent.label} width={logo} height={logo} className='object-contain' style={{ width: logo, height: logo }} />
			</motion.div>
		</motion.div>
	);
};

// Featured bento tile: the floating agent logos drift behind the copy, with a
// cursor-driven parallax. Used for the OS section's "Any agent harness" card.
const FeaturedHarnessCard = ({ feature }: { feature: { title: string; description: string; docsHref?: string; span?: string } }) => {
	const reduce = useReducedMotion() ?? false;
	const mxRaw = useMotionValue(0);
	const myRaw = useMotionValue(0);
	const mx = useSpring(mxRaw, { stiffness: 90, damping: 18, mass: 0.6 });
	const my = useSpring(myRaw, { stiffness: 90, damping: 18, mass: 0.6 });

	const handleMove = useCallback(
		(e: React.MouseEvent<HTMLDivElement>) => {
			const rect = e.currentTarget.getBoundingClientRect();
			mxRaw.set(((e.clientX - rect.left) / rect.width - 0.5) * 2);
			myRaw.set(((e.clientY - rect.top) / rect.height - 0.5) * 2);
		},
		[mxRaw, myRaw],
	);
	const handleLeave = useCallback(() => {
		mxRaw.set(0);
		myRaw.set(0);
	}, [mxRaw, myRaw]);

	return (
		<motion.div
			initial={{ opacity: 0, y: 20 }}
			whileInView={{ opacity: 1, y: 0 }}
			viewport={{ once: true }}
			transition={{ duration: 0.5 }}
			onMouseMove={reduce ? undefined : handleMove}
			onMouseLeave={reduce ? undefined : handleLeave}
			className={`group relative flex flex-col overflow-hidden p-6 ${CARD_SURFACE} ${feature.span ?? ''}`}
		>
			{/* Floating agent logos — desktop only, behind the copy */}
			<div aria-hidden className='pointer-events-none absolute inset-0 z-0 hidden lg:block'>
				{FLOATING_AGENTS.map((agent) => (
					<FloatingAgentTile key={agent.label} agent={agent} mx={mx} my={my} reduce={reduce} />
				))}
			</div>

			<div className='relative z-10 flex h-full flex-col'>
				<h3 className='mb-2 text-2xl font-medium tracking-[-0.015em] text-ink md:text-3xl'>{feature.title}</h3>
				<p className='max-w-sm text-base leading-relaxed text-ink-soft md:text-lg'>{feature.description}</p>
				{feature.docsHref && (
					<div className='mt-auto pt-4'>
						<DocsLink href={feature.docsHref} />
					</div>
				)}
			</div>
		</motion.div>
	);
};

const OperatingSystemSection = () => (
	<section className='border-t border-ink/10 px-6 py-24 md:py-32'>
		<div className='mx-auto max-w-7xl'>
			<Reveal>
				<SectionHeading
					title='Everything your agent expects from an operating system.'
					subtitle='A persistent file system, session memory, integrations, human oversight, and preview URLs, in a single npm package.'
					className='mb-10 max-w-3xl md:mb-12'
				/>
			</Reveal>

			<div className='grid grid-flow-row-dense grid-cols-1 gap-4 sm:grid-cols-2 lg:auto-rows-fr lg:grid-cols-4'>
				{osFeatures.map((feature) =>
					feature.featured ? (
						<FeaturedHarnessCard key={feature.title} feature={feature} />
					) : (
						<CapabilityCard key={feature.title} {...feature} noIcon />
					),
				)}
			</div>
		</div>
	</section>
);

// --- agentOS Features Section ---
const REGISTRY_TYPE_LABELS: Record<string, string> = {
  agent: 'Agent',
  'file-system': 'File System',
  'sandbox-extension': 'Sandbox',
  software: 'Software',
  tool: 'Tool',
};

// Two marquee rows of registry apps, split into opposing-direction tracks:
// software tools first, then file systems, browsers, and sandboxes. Pulled
// live from the registry so titles, icons, and status stay in sync.
const REGISTRY_ROW_A = ['git', 'ripgrep', 'jq', 'sqlite3', 'duckdb', 'curl', 'vim', 'wget'];
const REGISTRY_ROW_B = ['browserbase', 's3', 'google-drive', 'docker', 'e2b', 'daytona', 'modal', 'pi'];
const pickRegistry = (slugs: string[]) =>
  slugs
    .map((slug) => registry.find((entry) => entry.slug === slug))
    .filter((entry): entry is (typeof registry)[number] => entry !== undefined);
const registryRowA = pickRegistry(REGISTRY_ROW_A);
const registryRowB = pickRegistry(REGISTRY_ROW_B);

const RegistryAppTile = ({ entry, hidden }: { entry: (typeof registry)[number]; hidden?: boolean }) => {
  const available = entry.status === 'available';
  const category = REGISTRY_TYPE_LABELS[entry.types[0]] ?? 'Integration';
  const IconComponent = entry.icon ? REGISTRY_ICONS[entry.icon] : undefined;
  return (
    <a
      href={`/registry/${entry.slug}`}
      aria-hidden={hidden}
      tabIndex={hidden ? -1 : undefined}
      className='group/tile flex w-64 shrink-0 items-center gap-3.5 rounded-xl border border-ink/10 bg-white/55 p-3 transition-colors hover:border-ink/25 hover:bg-white/80'
    >
      <div className='flex h-12 w-12 shrink-0 items-center justify-center rounded-xl border border-ink/10 bg-ink/5'>
        {entry.image ? (
          <img src={entry.image} alt={entry.title} width={26} height={26} className='object-contain' />
        ) : IconComponent ? (
          <IconComponent style={{ width: 24, height: 24 }} className='text-ink' />
        ) : (
          <span className='font-mono text-base font-medium text-ink-soft'>{entry.title.charAt(0)}</span>
        )}
      </div>
      <div className='min-w-0 flex-1'>
        <h4 className='truncate text-sm font-medium text-ink'>{entry.title}</h4>
        <p className='truncate text-xs text-ink-faint'>{category}</p>
      </div>
      <span
        className={`shrink-0 rounded-full border px-3 py-1 text-[11px] font-semibold uppercase tracking-wide transition-colors ${
          available
            ? 'border-ink/15 text-ink-soft group-hover/tile:border-ink group-hover/tile:bg-ink group-hover/tile:text-cream'
            : 'border-ink/10 text-ink-faint'
        }`}
      >
        {available ? 'Get' : 'Soon'}
      </span>
    </a>
  );
};

const RegistryMarqueeRow = ({
  apps,
  direction,
}: {
  apps: (typeof registry)[number][];
  direction: 'left' | 'right';
}) => (
  <div className='group relative overflow-hidden [-webkit-mask-image:linear-gradient(to_right,transparent,#000_6%,#000_94%,transparent)] [mask-image:linear-gradient(to_right,transparent,#000_6%,#000_94%,transparent)]'>
    <div
      className={`flex w-max gap-3 ${
        direction === 'left'
          ? 'animate-[registry-marquee-left_46s_linear_infinite]'
          : 'animate-[registry-marquee-right_46s_linear_infinite]'
      } group-hover:[animation-play-state:paused] motion-reduce:animate-none`}
    >
      {[...apps, ...apps].map((entry, i) => (
        <RegistryAppTile key={`${entry.slug}-${i}`} entry={entry} hidden={i >= apps.length} />
      ))}
    </div>
  </div>
);

const RegistryCallout = () => (
  <section className='border-t border-ink/10 px-6 py-24 md:py-40'>
    <div className='mx-auto max-w-7xl'>
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        whileInView={{ opacity: 1, y: 0 }}
        viewport={{ once: true }}
        transition={{ duration: 0.5 }}
        className='overflow-hidden rounded-2xl border border-ink/10 bg-white/55 p-6 md:p-10'
      >
        <div className='mb-8 max-w-2xl'>
          <h3 className='mb-2 text-2xl font-medium tracking-[-0.015em] text-ink md:text-3xl'>
            Supports the tools you&apos;d expect.
          </h3>
          <p className='text-base leading-relaxed text-ink-soft'>
            git, curl, ripgrep, jq, and SQLite run inside the VM. Browsers, file system mounts,
            and sandboxes install the same way from the registry.
          </p>
        </div>

        <div className='flex flex-col gap-3'>
          <RegistryMarqueeRow apps={registryRowA} direction='left' />
          <RegistryMarqueeRow apps={registryRowB} direction='right' />
        </div>

        <div className='mt-8 flex items-center justify-end border-t border-ink/10 pt-5'>
          <a
            href='/registry'
            className='selection-dark inline-flex flex-shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md bg-ink px-4 py-2 text-sm font-medium text-cream transition-colors hover:bg-ink/85'
          >
            Explore the Registry
            <ArrowRight className='h-4 w-4' />
          </a>
        </div>
      </motion.div>
    </div>
  </section>
);

// --- Benchmarks ---
// Benchmark data (computed from raw inputs in bench.ts)
import { benchColdStart, benchWorkloads, BENCHMARK_DATE, SANDBOX_COLDSTART_PROVIDER, SANDBOX_COST_PROVIDER, type WorkloadKey } from '../../../data/bench';

function BenchInfoTooltip({ children }: { children: React.ReactNode }) {
	// The wrapper is intentionally not positioned so the tooltip spans the ink
	// card itself (the nearest positioned ancestor) instead of clipping at the
	// panel's overflow-hidden edge.
	return (
		<span className='group/tip ml-1.5 inline-flex align-middle'>
			<svg
				className='h-3.5 w-3.5 cursor-help text-cream/35 transition-colors group-hover/tip:text-cream/70'
				viewBox='0 0 16 16'
				fill='currentColor'
			>
				<path d='M8 0a8 8 0 100 16A8 8 0 008 0zm1 12H7V7h2v5zm-1-6a1 1 0 110-2 1 1 0 010 2z' />
			</svg>
			<span className='pointer-events-none absolute inset-x-3 bottom-12 z-50 rounded-lg border border-cream/15 bg-ink p-3 text-left text-[11px] leading-relaxed text-cream/80 opacity-0 shadow-xl transition-opacity duration-200 group-hover/tip:pointer-events-auto group-hover/tip:opacity-100 [&_a]:text-cream [&_a]:underline [&_a]:underline-offset-2 [&_strong]:font-medium [&_strong]:text-cream'>
				{children}
			</span>
		</span>
	);
}

function BenchToggle({ options, active, onChange }: { options: string[]; active: number; onChange: (idx: number) => void }) {
  const layoutId = useId();
  const columns = options.length === 3 ? 'grid-cols-3' : 'grid-cols-2';

  return (
    <div className={`grid w-full gap-1 rounded-lg border border-cream/10 bg-cream/[0.03] p-1 ${columns}`}>
      {options.map((label, i) => {
        const isActive = i === active;
        return (
          <motion.button
            key={label}
            type='button'
            onClick={() => onChange(i)}
            aria-pressed={isActive}
            whileTap={{ scale: 0.94 }}
            className={`relative flex h-7 min-w-0 items-center justify-center rounded-md px-1.5 text-center font-mono text-[10px] uppercase tracking-[0.12em] transition-colors ${
              isActive ? 'text-ink' : 'text-cream/45 hover:text-cream/75'
            }`}
          >
            {isActive && (
              <motion.span
                layoutId={`bench-toggle-${layoutId}`}
                className='absolute inset-0 rounded-md bg-cream'
                transition={{ type: 'spring', stiffness: 480, damping: 38 }}
              />
            )}
            <span className='relative z-[1] truncate'>{label}</span>
          </motion.button>
        );
      })}
    </div>
  );
}

interface BenchRowEntry {
	label: React.ReactNode;
	value: string;
	highlight?: boolean;
}

// Splits a stat string into a leading symbol prefix, the numeric portion, and a
// trailing unit suffix so the number can be counted up while the units stay put.
// Returns null when there is no number to animate (e.g. "Infinite").
function parseStatNumber(text: string) {
	const match = text.match(/^([^\d-]*)(-?[\d,]*\.?\d+)(.*)$/);
	if (!match) return null;
	const [, prefix, rawNumber, suffix] = match;
	const normalized = rawNumber.replace(/,/g, '');
	const decimals = normalized.includes('.') ? normalized.split('.')[1].length : 0;
	return {
		prefix,
		suffix,
		value: Number.parseFloat(normalized),
		decimals,
		grouped: rawNumber.includes(','),
	};
}

// Counts the numeric part of a stat from 0 up to its value. The first run is
// gated on `active` (the card scrolling into view) and only fires once; later
// value changes (toggling workload or tier) re-trigger the count from the
// previous value. Honors reduced-motion by rendering the final value outright.
function CountUpStat({ text, active }: { text: string; active: boolean }) {
	const parsed = useMemo(() => parseStatNumber(text), [text]);
	const reducedMotion = useReducedMotion();
	const target = parsed?.value ?? 0;

	const [display, setDisplay] = useState(0);
	const startedRef = useRef(false);
	const fromRef = useRef(0);
	const rafRef = useRef(0);

	useEffect(() => {
		if (!parsed) return;
		if (reducedMotion) {
			setDisplay(target);
			fromRef.current = target;
			startedRef.current = true;
			return;
		}
		// Not yet scrolled into view: stay primed at zero for the first count-up.
		if (!active) {
			if (!startedRef.current) setDisplay(0);
			return;
		}
		const from = startedRef.current ? fromRef.current : 0;
		startedRef.current = true;
		const duration = 850;
		let start = 0;
		const step = (now: number) => {
			if (!start) start = now;
			const t = Math.min(1, (now - start) / duration);
			const eased = 1 - (1 - t) ** 3;
			setDisplay(from + (target - from) * eased);
			if (t < 1) {
				rafRef.current = requestAnimationFrame(step);
			} else {
				fromRef.current = target;
			}
		};
		rafRef.current = requestAnimationFrame(step);
		return () => cancelAnimationFrame(rafRef.current);
	}, [parsed, target, active, reducedMotion]);

	if (!parsed) return <>{text}</>;

	const formatted = parsed.grouped
		? display.toLocaleString(undefined, {
				minimumFractionDigits: parsed.decimals,
				maximumFractionDigits: parsed.decimals,
			})
		: display.toFixed(parsed.decimals);

	return (
		<span className='tabular-nums'>
			{parsed.prefix}
			{formatted}
			{parsed.suffix}
		</span>
	);
}

// Dark ink data card with a mono title, direction tag, headline stat,
// and label/value rows pinned to the card's foot.
function BenchCard({
  title,
  statNote,
  verb,
  toggle,
  rows,
  note,
}: {
  title: string;
  statNote: string;
  verb: string;
  toggle?: React.ReactNode;
  rows: BenchRowEntry[];
  note?: string;
}) {
  // Trigger the count-up the first time the card scrolls into view, once.
  const [inView, setInView] = useState(false);

  return (
    // overflow-visible so the agentOS info tooltip can extend above the card
    // instead of being clipped by the panel's rounded-corner mask.
    <InkPanel className='h-full' overflow='visible'>
      <motion.div
        className='flex h-full flex-col p-6 md:p-7'
        onViewportEnter={() => setInView(true)}
        viewport={{ once: true, margin: '-10% 0px' }}
      >
        {/* Eyebrow rail */}
        <div className='flex min-h-[2.5rem] items-start justify-between gap-3'>
          <span className='font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-accent'>{title}</span>
        </div>

        {/* Verdict: the headline multiplier */}
        <div className='mt-5 flex items-baseline gap-2'>
          <span className='font-sans text-[2.75rem] font-medium leading-[1.0] tracking-[-0.02em] tabular-nums text-cream md:text-5xl'>
            <CountUpStat text={statNote} active={inView} />
          </span>
          <span className='font-sans text-lg font-medium text-cream/45 md:text-xl'>{verb}</span>
        </div>

        {/* Comparison ledger: ours vs theirs, same unit, right-aligned */}
        <div className='mb-6 mt-6 divide-y divide-cream/10 border-y border-cream/10'>
          {rows.map((row, i) => (
            <div key={i} className='flex items-baseline justify-between gap-4 py-2.5'>
              <span className={`inline-flex min-w-0 items-baseline font-mono text-[13px] ${row.highlight ? 'font-medium text-cream' : 'font-normal text-cream/45'}`}>
                {row.label}
              </span>
              <span className={`whitespace-nowrap font-mono text-[15px] tabular-nums ${row.highlight ? 'font-medium text-accent' : 'font-normal text-cream/45'}`}>
                {row.value}
              </span>
            </div>
          ))}
        </div>

        {toggle}
        {note ? (
          <p className='mt-auto font-mono text-[10px] leading-relaxed text-cream/35'>{note}</p>
        ) : null}
      </motion.div>
    </InkPanel>
  );
}

function BenchColdStartChart() {
	const groups = benchColdStart;
	// Default to p50 so the hero chip's claim and this card's first-shown number agree.
	const [active, setActive] = useState(0);
	const g = groups[active];

	return (
		<BenchCard
			title='Cold Start'
			statNote={`${Math.round(g.sandbox / g.agentOS)}x`}
				verb='faster'
			toggle={<BenchToggle options={groups.map((t) => t.label)} active={active} onChange={setActive} />}
			rows={[
				{
					label: (
						<>
							agentOS
							<BenchInfoTooltip>
								<strong>What&apos;s measured:</strong> Time from requesting an execution to first code running.
								<br /><br />
								<strong>Why the gap:</strong> agentOS runs agents in-process — V8 isolates and Wasm inside your host. No VM to boot, no network hop, no disk image. Sandboxes must boot an entire environment, allocate memory, and establish a network connection before code can run.
								<br /><br />
								<strong>Sandbox baseline:</strong> {SANDBOX_COLDSTART_PROVIDER}, the fastest mainstream sandbox provider as of {BENCHMARK_DATE}.
								<br /><br />
								<strong>agentOS:</strong> Median of 10,000 runs (100 iterations x 100 samples) on Intel i7-12700KF.
							</BenchInfoTooltip>
						</>
					),
					value: `${g.agentOS} ms`,
					highlight: true,
				},
				{ label: 'Fastest sandbox', value: `${g.sandbox.toLocaleString()} ms` },
			]}
		/>
	);
}

function BenchMemoryBar({ workload }: { workload: WorkloadKey }) {
	const mem = benchWorkloads[workload].memory;
	const [memMult, memVerb] = mem.multiplier.split(' ');

	return (
		<BenchCard
			title='Memory Per Instance'
			statNote={memMult}
				verb={memVerb}
			rows={[
				{
					label: (
						<>
							agentOS
							<BenchInfoTooltip>
								<strong>What&apos;s measured:</strong> Memory footprint added per concurrent execution.
								<br /><br />
								<strong>Why the gap:</strong> In-process isolates share the host's memory. Each additional execution only adds its own heap and stack. Sandboxes allocate a dedicated environment with a minimum memory reservation, even if the code inside uses far less.
								<br /><br />
								<strong>Sandbox baseline:</strong> {SANDBOX_COST_PROVIDER}, the cheapest mainstream sandbox provider as of {BENCHMARK_DATE}. Default sandbox: 1 vCPU + 1 GiB RAM.
								<br /><br />
								<strong>agentOS:</strong> {workload === 'agent' ? `${benchWorkloads.agent.memory.agentOS} for a full Pi coding agent session with MCP servers and file system mounts.` : `${benchWorkloads.shell.memory.agentOS} for the minimal shell workload under sustained load.`}
							</BenchInfoTooltip>
						</>
					),
					value: mem.agentOS,
					highlight: true,
				},
				{ label: 'Cheapest sandbox', value: mem.sandbox },
			]}
			note='Sandboxes reserve idle RAM per agent.'
		/>
	);
}

function BenchCostChart({ workload }: { workload: WorkloadKey }) {
	const tiers = benchWorkloads[workload].cost;
	const sandboxCost = benchWorkloads[workload].sandboxCost;
	// Default to AWS ARM so the hero chip's claim and this card's first-shown number agree.
	const [active, setActive] = useState(() => Math.max(0, tiers.findIndex((tier) => tier.label === 'AWS ARM')));
	const t = tiers[active];
	const [costMult, costVerb] = t.multiplier.split(' ');

	return (
		<BenchCard
			title='Cost Per Execution-Second'
			statNote={costMult}
				verb={costVerb}
			toggle={<BenchToggle options={tiers.map((tier) => tier.label)} active={active} onChange={setActive} />}
			rows={[
				{
					label: (
						<>
							agentOS
							<BenchInfoTooltip>
								<strong>What&apos;s measured:</strong> <code className='rounded bg-cream/10 px-1 py-0.5 text-[10px]'>server price per second / concurrent executions per server</code>
								<br /><br />
								<strong>Why it&apos;s cheaper:</strong> Each execution uses {benchWorkloads[workload].memory.agentOS} instead of a {benchWorkloads[workload].memory.sandbox} sandbox minimum. And you run on your own hardware, which is significantly cheaper than per-second sandbox billing.
								<br /><br />
								<strong>Sandbox baseline:</strong> {SANDBOX_COST_PROVIDER}, the cheapest mainstream sandbox provider as of {BENCHMARK_DATE}. Default sandbox: 1 vCPU + 1 GiB RAM at $0.0504/vCPU-h + $0.0162/GiB-h.
								<br /><br />
								<strong>agentOS:</strong> {benchWorkloads[workload].memory.agentOS} baseline per execution, assuming 70% utilization (industry-standard HPA scaling threshold). Select a hardware tier above to compare.
							</BenchInfoTooltip>
						</>
					),
					value: t.value,
					highlight: true,
				},
				{ label: 'Cheapest sandbox', value: sandboxCost },
			]}
			note='Assumes one agent per sandbox, needed for isolation.'
		/>
	);
}

function BenchmarkSection() {
	const [workload, setWorkload] = useState<WorkloadKey>('shell');
	const wl = benchWorkloads[workload];

	return (
		<motion.div
			initial={{ opacity: 0, y: 20 }}
			whileInView={{ opacity: 1, y: 0 }}
			viewport={{ once: true }}
			transition={{ duration: 0.5 }}
		>
			<div className='mb-6 flex items-center justify-between max-sm:flex-col max-sm:items-stretch max-sm:gap-2'>
				<p className='text-xs text-ink-faint max-sm:order-2 max-sm:px-1 max-sm:leading-relaxed'>
					Workload:{' '}
					<AnimatePresence mode='wait' initial={false}>
						<motion.span
							key={workload}
							initial={{ opacity: 0, y: 4 }}
							animate={{ opacity: 1, y: 0 }}
							exit={{ opacity: 0, y: -4 }}
							transition={{ duration: 0.2, ease: 'easeOut' }}
							className='inline-block'
						>
							{wl.description}
						</motion.span>
					</AnimatePresence>
				</p>
				<div className='flex gap-1 rounded-lg border border-ink/10 bg-white/55 p-1 max-sm:order-1 max-sm:grid max-sm:w-full max-sm:grid-cols-2 max-sm:rounded-xl'>
					{(Object.keys(benchWorkloads) as WorkloadKey[]).map((key) => {
              const isActive = workload === key;
              return (
                <motion.button
                  key={key}
                  onClick={() => setWorkload(key)}
                  aria-pressed={isActive}
                  whileTap={{ scale: 0.96 }}
                  className={`relative rounded-md px-2.5 py-1 text-xs font-medium transition-colors max-sm:flex max-sm:min-h-10 max-sm:w-full max-sm:items-center max-sm:justify-center max-sm:rounded-lg max-sm:py-2 max-sm:text-center ${
                    isActive ? 'text-cream' : 'text-ink-soft hover:text-ink'
                  }`}
                >
                  {isActive && (
                    <motion.span
                      layoutId='bench-workload-toggle'
                      className='absolute inset-0 rounded-md bg-ink max-sm:rounded-lg'
                      transition={{ type: 'spring', stiffness: 480, damping: 38 }}
                    />
                  )}
                  <span className='relative z-[1]'>{benchWorkloads[key].label}</span>
                </motion.button>
              );
            })}
				</div>
			</div>

			<div className='grid grid-cols-1 gap-6 md:grid-cols-2 lg:grid-cols-3'>
				<div id='bench-cold-start' className='scroll-mt-24'>
					<BenchColdStartChart />
				</div>
				<div id='bench-memory' className='scroll-mt-24'>
					<BenchMemoryBar workload={workload} />
				</div>
				<div id='bench-cost' className='scroll-mt-24'>
					<BenchCostChart workload={workload} />
				</div>
			</div>

			<p className='mt-8 font-mono text-xs leading-relaxed text-ink-faint'>
				Measured on Intel i7-12700KF. Cold start baseline: {SANDBOX_COLDSTART_PROVIDER}, the fastest mainstream sandbox provider as of {BENCHMARK_DATE}. Cost baseline: {SANDBOX_COST_PROVIDER}, the cheapest mainstream sandbox provider as of {BENCHMARK_DATE} (1 vCPU + 1 GiB default). Cost assumes 70% utilization on self-hosted hardware vs. per-second sandbox billing.{' '}
				<a
					href='/docs/benchmarks'
					className='inline-flex items-center gap-1 text-ink-soft underline underline-offset-2 transition-colors hover:text-ink'
				>
					Benchmark document
					<ExternalLink className='h-3 w-3' />
				</a>
			</p>
		</motion.div>
	);
}

// --- Argument (why an OS, not a sandbox) ---
// The narrative pivot right under the hero: what sandboxes provide, what agents
// actually use, and why that set ships better as a library. Rows come from
// docs/versus-sandbox.mdx (kept in sync in spirit); the docs table's Cost and
// Startup rows are omitted here because the benchmark charts directly below
// measure both.
const SANDBOX_COMPARISON = [
	{ label: 'Backend integration', agentOS: 'Bindings call your functions directly.', sandbox: 'Network calls back to your backend.' },
	{ label: 'Virtualization', agentOS: 'V8 isolates and WebAssembly: a kernel virtualized inside your process, nothing to boot.', sandbox: 'MicroVMs booted per agent on separate hosts.' },
	{ label: 'Isolation', agentOS: 'One VM per customer or job. VMs share no file system, memory, or crash fate.', sandbox: 'One container or microVM per agent.' },
	{
		label: 'Heavy workloads',
		agentOS: (
			<>
				Mount a Docker, E2B, or Daytona{' '}
				<a href='/docs/sandbox' className='underline underline-offset-2 hover:text-ink/80'>
					sandbox
				</a>{' '}
				into the same file system when a job needs native binaries, file watching, or a GPU.
			</>
		),
		sandbox: 'Native.',
	},
	{ label: 'Credentials', agentOS: 'Stay on the host. Bindings run your functions server-side; agents see only inputs and outputs.', sandbox: 'Injected into the sandbox environment.' },
	{ label: 'Permissions', agentOS: 'Granular and deny-by-default.', sandbox: 'Coarse, container-level.' },
	{ label: 'Infrastructure', agentOS: 'npm install', sandbox: 'Vendor account and API keys.' },
];

// Modal wrapper for the cold-start timeline, opened from the benchmarks
// header. Mounting the timeline on open replays its sequence each time.
const ColdStartModal = ({ open, onClose }: { open: boolean; onClose: () => void }) => {
	useEffect(() => {
		if (!open) return;
		const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
		document.addEventListener('keydown', onKey);
		const prevOverflow = document.body.style.overflow;
		document.body.style.overflow = 'hidden';
		return () => {
			document.removeEventListener('keydown', onKey);
			document.body.style.overflow = prevOverflow;
		};
	}, [open, onClose]);

	return (
		<AnimatePresence>
			{open && (
				<motion.div
					initial={{ opacity: 0 }}
					animate={{ opacity: 1 }}
					exit={{ opacity: 0 }}
					transition={{ duration: 0.2 }}
					className='fixed inset-0 z-[60] flex items-center justify-center bg-ink/50 p-4 md:p-8'
					onClick={onClose}
				>
					<motion.div
						role='dialog'
						aria-modal='true'
						aria-label='Cold-start timeline'
						initial={{ opacity: 0, y: 14, scale: 0.98 }}
						animate={{ opacity: 1, y: 0, scale: 1 }}
						exit={{ opacity: 0, y: 10, scale: 0.98 }}
						transition={{ duration: 0.25, ease: [0.22, 1, 0.36, 1] }}
						onClick={(e) => e.stopPropagation()}
						className='relative max-h-full w-full max-w-3xl overflow-y-auto rounded-2xl bg-paper p-5 shadow-[0_24px_70px_-20px_rgba(20,20,22,0.45)] ring-1 ring-ink/10 md:p-8'
					>
						<button
							type='button'
							onClick={onClose}
							aria-label='Close'
							className='absolute right-4 top-4 flex h-8 w-8 items-center justify-center rounded-full text-ink-faint transition-colors hover:bg-ink/[0.05] hover:text-ink'
						>
							<X className='h-4 w-4' />
						</button>
						<ColdStartTimeline />
					</motion.div>
				</motion.div>
			)}
		</AnimatePresence>
	);
};

const ArgumentSection = () => {
	const [showColdStart, setShowColdStart] = useState(false);

	return (
	<section className='border-t border-ink/10 px-6 py-16 md:py-32'>
		<div className='mx-auto max-w-7xl'>
			<Reveal>
				<SectionHeading
					title='The benefits of a sandbox, without the sandbox.'
					subtitle={
						<>
							Sandbox providers give every agent a full VM across a network gap and leave
							orchestration to you. agentOS runs each agent in its own VM inside your backend,
							with sessions, workflows, and approvals built in. Speed and footprint are measured
							below.
						</>
					}
					className='mb-10 max-w-3xl md:mb-12'
				/>
			</Reveal>

			{/* Comparison ledger, from docs/versus-sandbox.mdx. Its first row is
			    the topology picture the data rows annotate: agents inside your
			    backend vs a fleet across a network gap. */}
			<Reveal>
				<div className={`overflow-hidden p-6 md:p-8 ${CARD_SURFACE}`}>
					<div className='grid grid-cols-2 gap-x-6 gap-y-1 border-b border-ink/10 pb-3 sm:grid-cols-[minmax(0,0.7fr),1fr,1fr]'>
						<span className='hidden sm:block' aria-hidden='true' />
						<span className='text-sm font-medium text-pine'>agentOS</span>
						<span className='text-sm font-medium text-ink-faint'>Full sandbox</span>
					</div>
					<div className='grid grid-cols-2 gap-x-6 border-b border-ink/[0.06] py-4 sm:grid-cols-[minmax(0,0.7fr),1fr,1fr]'>
						<span className='hidden self-center text-sm text-ink-faint sm:block'>Where agents run</span>
						<AgentOsTopologyCell />
						<SandboxTopologyCell />
					</div>
					{SANDBOX_COMPARISON.map((row) => (
						<div
							key={row.label}
							className='grid grid-cols-2 gap-x-6 gap-y-1 border-b border-ink/[0.06] py-3 last:border-b-0 last:pb-0 sm:grid-cols-[minmax(0,0.7fr),1fr,1fr]'
						>
							<span className='col-span-2 text-sm text-ink-faint sm:col-span-1 sm:self-center'>
								{row.label}
							</span>
							<span className='text-sm leading-relaxed text-ink'>{row.agentOS}</span>
							<span className='text-sm leading-relaxed text-ink-faint'>{row.sandbox}</span>
						</div>
					))}
				</div>
			</Reveal>

			{/* The measurements behind the table's claims, same section: one proof
			    block instead of a second comparison section. */}
			<div className='mt-14 md:mt-20'>
				<Reveal>
					<div className='mb-8 max-w-2xl'>
						<h3 className='mb-2 text-2xl font-medium tracking-[-0.015em] text-ink md:text-3xl'>
							Faster starts, less memory, lower cost.
						</h3>
						<p className='text-base leading-relaxed text-ink-soft'>
							{`Measured against ${SANDBOX_COLDSTART_PROVIDER}, the fastest mainstream sandbox cold start, and ${SANDBOX_COST_PROVIDER}, the cheapest per-second sandbox, as of ${BENCHMARK_DATE}.`}{' '}
							<button
								type='button'
								onClick={() => setShowColdStart(true)}
								className='text-ink underline underline-offset-2 transition-colors hover:text-ink/80'
							>
								Watch how a cold start breaks down
							</button>
						</p>
					</div>
				</Reveal>
				<BenchmarkSection />
			</div>

		</div>

		<ColdStartModal open={showColdStart} onClose={() => setShowColdStart(false)} />
	</section>
	);
};

// --- Deployment ---
// Mirrors the "Start local. Scale to millions." hosting section from rivet.dev:
// a three-card local -> managed -> self-host story, plus a row of deploy targets.
// Content is grounded in /docs/deployment (agentOS runs as Rivet Actors).

const DEPLOY_CARD_CLASS =
	'relative flex h-full flex-col border border-ink/10 bg-white/55 p-6 md:p-8';
const DEPLOY_CARD_TITLE_CLASS = 'text-base font-medium tracking-tight text-ink';
const DEPLOY_BUTTON_BASE =
	'inline-flex h-10 w-full items-center justify-center gap-2 whitespace-nowrap rounded-md px-4 text-sm font-medium transition-colors';
const DEPLOY_GHOST_BUTTON_CLASS = `${DEPLOY_BUTTON_BASE} border border-ink/15 text-ink-soft hover:border-ink/40 hover:text-ink`;
const DEPLOY_PRIMARY_BUTTON_CLASS = `${DEPLOY_BUTTON_BASE} selection-dark bg-ink text-cream hover:bg-ink/85`;

// Deploy targets, linking out to Rivet's deploy guides (agentOS is powered by Rivet).
const DEPLOY_TARGETS = [
	{ label: 'Rivet Compute', href: 'https://rivet.dev/docs/deploy/rivet-compute' },
	{ label: 'Vercel', href: 'https://rivet.dev/docs/deploy/vercel' },
	{ label: 'Railway', href: 'https://rivet.dev/docs/deploy/railway' },
	{ label: 'Kubernetes', href: 'https://rivet.dev/docs/deploy/kubernetes' },
	{ label: 'AWS', href: 'https://rivet.dev/docs/deploy/aws-ecs' },
	{ label: 'Google Cloud', href: 'https://rivet.dev/docs/deploy/gcp-cloud-run' },
	{ label: 'Hetzner', href: 'https://rivet.dev/docs/deploy/hetzner' },
	{ label: 'VM & Bare Metal', href: 'https://rivet.dev/docs/deploy/vm-and-bare-metal' },
];

const DeploymentSection = () => (
	<section className='border-t border-ink/10 py-16 md:py-32'>
		<div className='mx-auto max-w-7xl px-6'>
			<div className='mb-12'>
				<motion.h2
					initial={{ opacity: 0, y: 20 }}
					whileInView={{ opacity: 1, y: 0 }}
					viewport={{ once: true }}
					transition={{ duration: 0.5 }}
					className='mb-2 text-3xl font-medium tracking-[-0.015em] text-ink md:text-5xl'
				>
					Start local. Scale to millions.
				</motion.h2>
				<motion.p
					initial={{ opacity: 0, y: 20 }}
					whileInView={{ opacity: 1, y: 0 }}
					viewport={{ once: true }}
					transition={{ duration: 0.5, delay: 0.1 }}
					className='max-w-xl text-base leading-relaxed text-ink-soft'
				>
					agentOS runs as Rivet Actors, so agents live next to their state and deploy wherever you
					already ship. The same SDK runs as a library in development and as a platform in
					production.
				</motion.p>
			</div>

			<motion.div
				initial={{ opacity: 0, y: 20 }}
				whileInView={{ opacity: 1, y: 0 }}
				viewport={{ once: true }}
				transition={{ duration: 0.5 }}
				className='grid grid-cols-1 gap-6 md:grid-cols-3 md:items-stretch'
			>
				{/* Card 1: Just a Library */}
				<div className={DEPLOY_CARD_CLASS}>
					<div className='mb-3 flex h-6 items-center gap-2.5'>
						<Package className='h-4 w-4 text-olive' />
						<h3 className={DEPLOY_CARD_TITLE_CLASS}>Local Development as a Library</h3>
					</div>
					<p className='text-sm leading-relaxed text-ink-soft'>
						Install a package and run locally. No servers, no infrastructure &mdash; VMs and agents run in your process during development.
					</p>
					<div className='flex-1' />
					<a href='/docs/quickstart' className={`mt-6 ${DEPLOY_GHOST_BUTTON_CLASS}`}>
						Get Started
					</a>
				</div>

				{/* Card 2: Rivet Cloud (primary) */}
				<div className={`${DEPLOY_CARD_CLASS} border-ink/20`}>
					<div className='mb-3 flex h-6 items-center gap-2.5'>
						<img className='h-4 w-4' src='/rivet-icon.svg' alt='Rivet' />
						<h3 className={DEPLOY_CARD_TITLE_CLASS}>Rivet Cloud</h3>
					</div>
					<p className='text-sm leading-relaxed text-ink-soft'>
						Fully managed agentOS on a global edge network. Bring your own cloud or run on Rivet Compute. Zero-ops, scales to millions of agents.
					</p>
					<div className='flex-1' />
					<a
						href='https://dashboard.rivet.dev'
						target='_blank'
						rel='noopener noreferrer'
						className={`mt-6 ${DEPLOY_PRIMARY_BUTTON_CLASS}`}
					>
						Sign Up
					</a>
				</div>

				{/* Card 3: Self-Host */}
				<div className={DEPLOY_CARD_CLASS}>
					<div className='mb-3 flex h-6 items-center gap-2.5'>
						<Server className='h-4 w-4 text-olive' />
						<h3 className={DEPLOY_CARD_TITLE_CLASS}>Self-Host</h3>
					</div>
					<p className='text-sm leading-relaxed text-ink-soft'>
						Run the open-source Rivet platform on your own infrastructure &mdash; Kubernetes, Hetzner, VMs, and more.
					</p>
					<div className='flex-1' />
					<a href='/docs/deployment' className={`mt-6 ${DEPLOY_GHOST_BUTTON_CLASS}`}>
						Self-Hosting Docs
					</a>
				</div>
			</motion.div>

			<motion.div
				initial={{ opacity: 0, y: 20 }}
				whileInView={{ opacity: 1, y: 0 }}
				viewport={{ once: true }}
				transition={{ duration: 0.5, delay: 0.1 }}
				className='mt-10 flex flex-wrap items-center gap-x-2 gap-y-2 border-t border-ink/10 pt-6'
			>
				<span className='mr-3 font-mono text-[11px] uppercase tracking-[0.16em] text-ink-faint'>
					Deploys anywhere
				</span>
				{DEPLOY_TARGETS.map(({ label, href }) => (
					<a
						key={label}
						href={href}
						target='_blank'
						rel='noopener noreferrer'
						className='inline-flex items-center rounded-full border border-ink/12 bg-white/45 px-2.5 py-1 text-[13px] text-ink-soft transition-colors hover:border-ink/25 hover:text-ink'
					>
						{label}
					</a>
				))}
			</motion.div>
		</div>
	</section>
);

// --- Closing band ---
// The page opens with the argument (an OS, not a sandbox) and closes with the
// action. Repeats the hero CTAs so the reader never scrolls back up to act.
const ClosingCta = () => (
	<section className='border-t border-ink/10 px-6 py-24 md:py-32'>
		<div className='mx-auto max-w-7xl'>
			<Reveal>
				<InkPanel>
					<div className='flex flex-col items-center px-6 py-16 text-center md:py-24'>
						<h2 className='mb-3 max-w-2xl text-balance text-3xl font-medium tracking-[-0.015em] text-cream md:text-5xl'>
							Give your agents an OS.
						</h2>
						<p className='mb-8 text-base leading-relaxed text-cream/70'>
							agentOS is open source under Apache 2.0. Install it from npm and run it inside your
							existing backend.
						</p>
						<div className='flex flex-col flex-wrap items-center gap-x-4 gap-y-3 sm:flex-row sm:justify-center'>
							<SetupWithAgent />
							<a
								href='/docs'
								className='inline-flex w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-cream/25 px-4 py-2 text-sm font-medium text-cream transition-colors hover:border-cream/50 hover:bg-cream/[0.04] sm:w-auto'
							>
								Read the Docs
								<ArrowRight className='h-4 w-4' />
							</a>
							<a
								href='https://github.com/rivet-dev/agentos'
								target='_blank'
								rel='noopener noreferrer'
								className='inline-flex w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-cream/25 px-4 py-2 text-sm font-medium text-cream transition-colors hover:border-cream/50 hover:bg-cream/[0.04] sm:w-auto'
							>
								View on GitHub
								<ExternalLink className='h-4 w-4' />
							</a>
						</div>
					</div>
				</InkPanel>
			</Reveal>
		</div>
	</section>
);

// --- Main Page ---
export default function AgentOSPage({ heroTabs }: AgentOSPageProps) {
	return (
		<div className='paper-grain min-h-screen font-sans text-ink-soft' style={{ overflowX: 'clip' }}>
			<main>
				<Hero />
				<ArgumentSection />
				<ExecutionLayersSection />
				<OperatingSystemSection />
				<RegistryCallout />
				<OrchestrationSection heroTabs={heroTabs} />
				<DeploymentSection />
				<ClosingCta />
			</main>
		</div>
	);
}
