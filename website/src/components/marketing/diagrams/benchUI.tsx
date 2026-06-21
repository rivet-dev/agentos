'use client';

import { useId, useState, useEffect, useRef, useMemo } from 'react';
import type { ReactNode } from 'react';
import { motion, useReducedMotion } from 'framer-motion';

// ---------------------------------------------------------------------------
// Shared primitives for the benchmark diagrams. Extracted from AgentOSPage so
// the cold-start / memory / execution diagram components can reuse them without
// importing from the page component. Styled for the light marketing cards.
// ---------------------------------------------------------------------------

// A help icon with a hover tooltip. The wrapper is intentionally not positioned
// so the tooltip spans the ink card itself (the nearest positioned ancestor)
// instead of clipping at the panel's overflow-hidden edge.
export function BenchInfoTooltip({ children }: { children: ReactNode }) {
	return (
		<span className='group/tip ml-1.5 inline-flex align-middle'>
			<svg
				className='h-3.5 w-3.5 cursor-help text-ink/30 transition-colors group-hover/tip:text-ink/60'
				viewBox='0 0 16 16'
				fill='currentColor'
			>
				<path d='M8 0a8 8 0 100 16A8 8 0 008 0zm1 12H7V7h2v5zm-1-6a1 1 0 110-2 1 1 0 010 2z' />
			</svg>
			<span className='pointer-events-none absolute inset-x-3 bottom-12 z-50 rounded-lg border border-ink/15 bg-white p-3 text-left text-[11px] leading-relaxed text-ink-soft opacity-0 shadow-xl transition-opacity duration-200 group-hover/tip:pointer-events-auto group-hover/tip:opacity-100 [&_a]:text-ink [&_a]:underline [&_a]:underline-offset-2 [&_strong]:font-medium [&_strong]:text-ink'>
				{children}
			</span>
		</span>
	);
}

// Segmented control with a framer-motion layoutId pill. Used for the cold-start
// percentile toggle, the hardware-tier toggle, and the workload toggle.
export function BenchToggle({ options, active, onChange }: { options: string[]; active: number; onChange: (idx: number) => void }) {
	const layoutId = useId();
	const columns =
		options.length === 4 ? 'grid-cols-2 sm:grid-cols-4' : options.length === 3 ? 'grid-cols-3' : 'grid-cols-2';

	return (
		<div className={`grid w-full gap-1 rounded-lg border border-ink/10 bg-white/55 p-1 ${columns}`}>
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
							isActive ? 'text-cream' : 'text-ink-soft hover:text-ink'
						}`}
					>
						{isActive && (
							<motion.span
								layoutId={`bench-toggle-${layoutId}`}
								className='absolute inset-0 rounded-md bg-ink'
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

// Splits a stat string into a leading symbol prefix, the numeric portion, and a
// trailing unit suffix so the number can be counted up while the units stay put.
// Returns null when there is no number to animate (e.g. "Infinite").
export function parseStatNumber(text: string) {
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
export function CountUpStat({ text, active }: { text: string; active: boolean }) {
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
