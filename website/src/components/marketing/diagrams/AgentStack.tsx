'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { EASE, VIEWPORT } from '../motion';

// ---------------------------------------------------------------------------
// The agentOS virtualization stack, top to bottom: agents run as guest
// processes on guest runtimes (native V8 isolates, CPython, WASM tools), every
// syscall those runtimes make lands in a per-agent virtual kernel, and the
// kernel is booted by a library inside your backend process, with no
// hypervisor anywhere in the path. The two agentOS layers carry the cream
// fill so the product's slice of the stack reads at a glance.
// ---------------------------------------------------------------------------

interface StackLayer {
	name: string;
	detail: string;
	logos?: { src: string; alt: string; wordmark?: boolean }[];
	product?: boolean;
}

const LAYERS: StackLayer[] = [
	{
		name: 'Agents',
		detail: 'Pi, Claude Code, Codex, and OpenCode run as guest processes.',
		logos: [
			{ src: '/images/agent-logos/pi.svg', alt: 'Pi' },
			{ src: '/images/agent-logos/claude-code.svg', alt: 'Claude Code' },
			{ src: '/images/agent-logos/codex.svg', alt: 'Codex' },
			{ src: '/images/agent-logos/opencode.svg', alt: 'OpenCode' },
		],
	},
	{
		name: 'Guest runtimes',
		detail: 'JavaScript on native V8 isolates, CPython 3.13, and WebAssembly builds of the POSIX toolbox.',
		logos: [
			{ src: '/images/registry/nodejs.svg', alt: 'Node.js' },
			{ src: '/images/registry/python.svg', alt: 'Python' },
			{ src: '/images/registry/linux.svg', alt: 'Linux tools' },
		],
	},
	{
		name: 'Virtual kernel',
		detail: 'One per agent VM. Every syscall lands here: file system, process table, sockets and DNS, PTYs, deny-by-default permissions.',
		product: true,
	},
	{
		name: 'agentOS library',
		detail: 'Boots the VMs inside your process. No hypervisor, no microVM, nothing to pull or boot.',
		product: true,
	},
	{
		name: 'Your backend',
		detail: 'Your own code, or a framework like Eve, Flue, or RivetKit, on infrastructure you already run.',
		logos: [
			{ src: '/images/frameworks/eve.svg', alt: 'Eve', wordmark: true },
			{ src: '/images/frameworks/flue.svg', alt: 'Flue' },
			{ src: '/rivet-icon.svg', alt: 'RivetKit' },
		],
	},
];

export const AgentStack = () => {
	const reduced = useReducedMotion();
	return (
		<div role='img' aria-label='The agentOS stack: agents run as guest processes on guest runtimes (native V8 isolates, CPython, and WebAssembly tools); every syscall lands in a per-agent virtual kernel; the agentOS library boots the VMs inside your backend with no hypervisor; your code or a framework like Eve or Flue drives it.'>
			<div className='flex flex-col gap-2'>
				{LAYERS.map((layer, i) => (
					<motion.div
						key={layer.name}
						initial={reduced ? undefined : { opacity: 0, y: 10 }}
						whileInView={reduced ? undefined : { opacity: 1, y: 0 }}
						viewport={VIEWPORT}
						transition={{ duration: 0.4, delay: 0.1 + i * 0.09, ease: [...EASE] }}
						className={`rounded-xl p-4 ring-1 ring-ink/[0.09] shadow-[0_1px_2px_rgba(20,20,22,0.06),0_4px_10px_-6px_rgba(20,20,22,0.14)] ${
							layer.product ? 'bg-[#faf8f3]' : 'bg-white'
						}`}
					>
						<div className='flex items-center justify-between gap-4'>
							<span className='text-sm font-medium text-ink'>{layer.name}</span>
							{layer.logos && (
								<div className='flex items-center gap-2.5'>
									{layer.logos.map((logo) => (
										<img
											key={logo.alt}
											src={logo.src}
											alt={logo.alt}
											title={logo.alt}
											className={logo.wordmark ? 'h-2.5 w-auto opacity-80' : 'h-4 w-4 object-contain opacity-80'}
										/>
									))}
								</div>
							)}
						</div>
						<p className='mt-1 text-xs leading-relaxed text-ink-soft'>{layer.detail}</p>
					</motion.div>
				))}
			</div>
		</div>
	);
};
