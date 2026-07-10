'use client';

import { motion, useReducedMotion } from 'framer-motion';
import { EASE, VIEWPORT } from '../motion';

// ---------------------------------------------------------------------------
// The architecture as containment: your backend is the outer box. Inside it,
// your code drives sessions into per-agent agentOS VMs. Each VM pairs a
// guest — an off-the-shelf agent like Pi, or one you build with a framework
// like Eve or Flue, on Node, Python, and shell, running on V8 isolates and
// WASM — with its own virtual kernel that services every syscall. The
// library hosts the VMs in a sidecar process it manages, so "your backend"
// is the deployment boundary rather than a literal single process — and
// there is no hypervisor or container in the path. White cards are elements
// of your backend, light chips are guest workloads, ink bars are the
// per-VM kernels.
// ---------------------------------------------------------------------------

// Eve's mark is its wordmark, so it renders wider and shorter than the
// square marks.
const VMS = [
	{
		agent: 'Pi',
		marks: [{ src: '/images/agent-logos/pi.svg', className: 'h-4 w-4 object-contain' }],
	},
	{
		agent: 'your agent',
		marks: [
			{ src: '/images/frameworks/eve.svg', className: 'h-2.5 w-auto object-contain' },
			{ src: '/images/frameworks/flue.svg', className: 'h-4 w-4 object-contain' },
		],
	},
];

// A dashed vertical connector with a label beside it and, unless reduced
// motion is on, a dot travelling the flow path.
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
			aria-label='agentOS architecture: inside your backend, your code drives sessions into per-agent agentOS VMs. In each VM an agent, such as Pi or one you build with a framework like Eve or Flue, runs Node, Python, and shell on V8 isolates and WebAssembly, and every syscall is served by that VM&apos;s own virtual kernel: file system, processes, sockets, and deny-by-default permissions. There is no hypervisor or container in the path.'
			className='rounded-2xl bg-white/45 p-4 ring-1 ring-ink/[0.09] shadow-[inset_0_1px_0_rgba(255,255,255,0.8),0_8px_24px_-14px_rgba(20,20,22,0.20)] md:p-5'
		>
			{/* Outer box: your backend */}
			<div className='mb-3'>
				<span className='text-sm font-medium text-ink'>Your backend</span>
			</div>

			{/* The driver: your code holds the sessions */}
			<Appear at={0.05} reduced={reduced}>
				<div className='rounded-xl bg-white px-3 py-2.5 text-center ring-1 ring-ink/[0.09] shadow-[0_1px_2px_rgba(20,20,22,0.06),0_4px_10px_-6px_rgba(20,20,22,0.12)]'>
					<span className='text-[13px] font-medium text-ink'>your code</span>
				</div>
			</Appear>

			{/* Driver -> VMs: one session per agent */}
			<div className='relative grid grid-cols-2 gap-3'>
				<Connector reduced={reduced} delay={0.2} />
				<Connector reduced={reduced} delay={0.7} />
				<span className='absolute inset-x-0 top-1/2 -translate-y-1/2 text-center font-mono text-[10px] text-ink-faint'>
					<span className='bg-[#efefef] px-2'>drives sessions</span>
				</span>
			</div>

			{/* Per-agent agentOS VMs */}
			<div className='grid grid-cols-2 gap-3'>
				{VMS.map((vm, i) => (
					<Appear key={vm.agent} at={0.15 + i * 0.12} reduced={reduced} className='rounded-xl bg-white p-3 ring-1 ring-ink/[0.09] shadow-[0_1px_2px_rgba(20,20,22,0.06),0_4px_10px_-6px_rgba(20,20,22,0.12)]'>
						<p className='mb-2 text-center font-mono text-[10px]'>
							<span className='text-ink/80'>agentOS</span> <span className='text-ink-faint'>vm</span>
						</p>

						{/* Guest: the agent and its execution engines */}
						<div className='rounded-lg bg-ink/[0.06] px-3 py-2.5 ring-1 ring-ink/[0.08]'>
							<div className='flex items-center justify-center gap-2'>
								{vm.marks.map((mark) => (
									<img key={mark.src} src={mark.src} alt='' aria-hidden='true' className={mark.className} />
								))}
								<span className='text-[13px] font-medium text-ink'>{vm.agent}</span>
							</div>
							<p className='mt-0.5 text-center font-mono text-[10px] text-ink-faint'>node · python · shell</p>
							<p className='text-center font-mono text-[10px] text-ink-faint'>on v8 isolates &amp; wasm</p>
						</div>

						<Connector label='every syscall' reduced={reduced} delay={0.6 + i * 0.5} />

						{/* The per-VM virtual kernel */}
						<div className='rounded-lg bg-ink px-3 py-2 text-center'>
							<span className='text-[12px] font-medium text-cream'>virtual kernel</span>
							<p className='mt-0.5 font-mono text-[9.5px] text-cream/55'>fs · processes · sockets · permissions</p>
						</div>
					</Appear>
				))}
			</div>

			{/* What is not in the path */}
			<p className='mt-3 text-center font-mono text-[10px] text-ink-faint'>no hypervisor · no containers</p>
		</div>
	);
};
