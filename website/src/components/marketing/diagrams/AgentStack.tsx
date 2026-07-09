'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { EASE, VIEWPORT } from '../motion';

// ---------------------------------------------------------------------------
// The architecture as containment, gVisor-diagram style: your backend is the
// outer box; inside it sit per-agent VMs, each pairing a guest (agent +
// runtimes) with its own virtual kernel that services every syscall; the VMs
// rest on the agentOS library bar spanning the bottom of the process. Dark
// bars mark the agentOS layers, dashed connectors carry the syscall flow.
// ---------------------------------------------------------------------------

const VMS = [
	{ agent: 'Pi', logo: '/images/agent-logos/pi.svg' },
	{ agent: 'Claude Code', logo: '/images/agent-logos/claude-code.svg' },
];

// A dashed vertical connector with a label beside it and, unless reduced
// motion is on, a dot travelling the syscall path.
const Connector = ({ label, reduced, delay }: { label?: string; reduced: boolean | null; delay: number }) => (
	<div className='relative mx-auto flex h-6 items-center justify-center gap-2'>
		<span aria-hidden='true' className='relative block h-full w-px border-l border-dashed border-ink/30'>
			{!reduced && (
				<motion.span
					aria-hidden='true'
					initial={{ top: 0, opacity: 0 }}
					animate={{ top: ['0%', '80%'], opacity: [0, 1, 1, 0] }}
					transition={{ duration: 1.3, repeat: Infinity, ease: 'easeInOut', delay, repeatDelay: 0.9 }}
					className='absolute -left-[2.5px] h-1 w-1 rounded-full bg-pine'
				/>
			)}
		</span>
		{label && <span className='font-mono text-[10px] text-ink-faint'>{label}</span>}
	</div>
);

const Appear = ({ at, reduced, children, className }: { at: number; reduced: boolean | null; children: React.ReactNode; className?: string }) => (
	<motion.div
		initial={reduced ? undefined : { opacity: 0, y: 8 }}
		whileInView={reduced ? undefined : { opacity: 1, y: 0 }}
		viewport={VIEWPORT}
		transition={{ duration: 0.35, delay: at, ease: [...EASE] }}
		className={className}
	>
		{children}
	</motion.div>
);

export const AgentStack = () => {
	const reduced = useReducedMotion();
	return (
		<div
			role='img'
			aria-label='agentOS architecture: inside your backend process, each agent runs in its own VM where a guest (the agent on Node, Python, and shell runtimes) makes syscalls against a per-VM virtual kernel; the VMs sit on the agentOS library with no hypervisor in the path.'
			className='rounded-2xl bg-white/45 p-4 ring-1 ring-ink/[0.09] shadow-[inset_0_1px_0_rgba(255,255,255,0.8),0_8px_24px_-14px_rgba(20,20,22,0.20)] md:p-5'
		>
			{/* Outer box: your backend process */}
			<div className='mb-3 flex items-baseline justify-between gap-4'>
				<span className='text-sm font-medium text-ink'>Your backend</span>
				<div className='flex items-center gap-2.5' title='Works with Eve, Flue, and RivetKit'>
					<img src='/images/frameworks/eve.svg' alt='Eve' className='h-2.5 w-auto opacity-70' />
					<img src='/images/frameworks/flue.svg' alt='Flue' className='h-4 w-4 object-contain opacity-70' />
					<img src='/rivet-icon.svg' alt='RivetKit' className='h-4 w-4 object-contain opacity-70' />
				</div>
			</div>

			{/* Per-agent VMs */}
			<div className='grid grid-cols-2 gap-3'>
				{VMS.map((vm, i) => (
					<Appear key={vm.agent} at={0.1 + i * 0.12} reduced={reduced} className='rounded-xl bg-white p-3 ring-1 ring-ink/[0.09] shadow-[0_1px_2px_rgba(20,20,22,0.06),0_4px_10px_-6px_rgba(20,20,22,0.12)]'>
						<p className='mb-2 text-center font-mono text-[10px] text-ink-faint'>agent vm</p>

						{/* Guest: the agent and its runtimes */}
						<div className='rounded-lg bg-ink/[0.06] px-3 py-2.5 ring-1 ring-ink/[0.08]'>
							<div className='flex items-center justify-center gap-2'>
								<img src={vm.logo} alt='' aria-hidden='true' className='h-4 w-4 object-contain' />
								<span className='text-[13px] font-medium text-ink'>{vm.agent}</span>
							</div>
							<p className='mt-0.5 text-center font-mono text-[10px] text-ink-faint'>node · python · shell</p>
						</div>

						<Connector label='syscalls' reduced={reduced} delay={0.4 + i * 0.5} />

						{/* The per-VM virtual kernel */}
						<div className='rounded-lg bg-ink px-3 py-2 text-center'>
							<span className='text-[12px] font-medium text-cream'>virtual kernel</span>
							<p className='mt-0.5 font-mono text-[9.5px] text-cream/55'>fs · processes · sockets · permissions</p>
						</div>
					</Appear>
				))}
			</div>

			{/* Kernel -> library: still inside the same process */}
			<div className='relative grid grid-cols-2 gap-3'>
				<Connector reduced={reduced} delay={1.1} />
				<Connector reduced={reduced} delay={1.6} />
				<span className='absolute inset-x-0 top-1/2 -translate-y-1/2 text-center font-mono text-[10px] text-ink-faint'>
					<span className='bg-[#efefef] px-2'>in-process · no hypervisor</span>
				</span>
			</div>

			{/* The library bar the VMs rest on */}
			<Appear at={0.35} reduced={reduced}>
				<div className='flex items-baseline justify-between gap-4 rounded-lg bg-ink px-4 py-2.5'>
					<span className='text-[13px] font-medium text-cream'>agentOS library</span>
					<span className='font-mono text-[10px] text-cream/55'>runs as a Rivet Actor</span>
				</div>
			</Appear>
		</div>
	);
};
