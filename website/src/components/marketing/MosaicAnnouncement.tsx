"use client";

// v0.2 announcement card (mosaic bento). On load the agentOS logo + "v0.2"
// play centered and large; once the intro finishes, the same block docks into
// the tall brand cell (left column) while the feature cells fade in. One logo
// element moves — it is not re-mounted — so the intro plays once and the dock
// is a pure transition.
//
// Sized for a small screenshot: large titles, no descriptions.
import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { AnimatedAgentOSLogo } from "./solutions/AgentOSPage";
import { Server, Users } from "lucide-react";

// Newly-supported agents shown as logo chips (matches the landing page).
const agentLogos = [
	{ src: "/images/registry/claude-code.svg", name: "Claude Code" },
	{ src: "/images/registry/codex.svg", name: "Codex" },
	{ src: "/images/registry/opencode.svg", name: "OpenCode" },
	{ src: "/images/registry/pi.svg", name: "Pi" },
];

// The uniform feature cells (the wide perf tile and the agent tile are custom).
const tiles = [
	{ Icon: Users, title: "Multiplayer & workflows" },
	{ Icon: Server, title: "Improved file system & limits" },
	{ img: "/rivet-icon.svg", title: "1-prompt deploy to Rivet Cloud" },
];

// Headline performance stats. Keep in sync with website/src/data/bench.ts
// (memory 1024/131≈8×, cold start 440ms/~0.85ms≈516×, cost ratio≈1738×).
const perfStats = [
	{ n: "516×", l: "faster cold starts" },
	{ n: "8×", l: "less memory" },
	{ n: "1738×", l: "cheaper to run" },
];

// Logo draw speed for this page, plus the dock timing: start shrinking into the
// bento cell just after the stroke finishes, with a slow, deliberate dock.
const LOGO_DRAW_SEC = 1.15;
const DOCK_DELAY_MS = LOGO_DRAW_SEC * 1000 + 150;
const DOCK_DURATION_SEC = 1.05;

const CARD_SURFACE =
	"rounded-2xl bg-gradient-to-b from-white to-[#f9f9fa] ring-1 ring-ink/[0.08] " +
	"shadow-[inset_0_1px_0_rgba(255,255,255,0.9),0_1px_2px_-1px_rgba(20,20,22,0.10),0_8px_24px_-14px_rgba(20,20,22,0.20)]";

export default function MosaicAnnouncement() {
	const cardRef = useRef<HTMLDivElement>(null);
	const brandRef = useRef<HTMLDivElement>(null);
	const [docked, setDocked] = useState(false);
	const [target, setTarget] = useState({ x: 0, y: 0 });

	useEffect(() => {
		const measure = () => {
			const card = cardRef.current;
			const brand = brandRef.current;
			if (!card || !brand) return;
			const brandCx = brand.offsetLeft + brand.offsetWidth / 2;
			const brandCy = brand.offsetTop + brand.offsetHeight / 2;
			setTarget({ x: brandCx - card.clientWidth / 2, y: brandCy - card.clientHeight / 2 });
		};
		measure();
		window.addEventListener("resize", measure);
		const t = setTimeout(() => setDocked(true), DOCK_DELAY_MS);
		return () => {
			clearTimeout(t);
			window.removeEventListener("resize", measure);
		};
	}, []);

	const ease = [0.22, 1, 0.36, 1] as const;
	// Cards rise + scale in, staggered, as the logo docks.
	const cellAnim = (i: number) => ({
		initial: { opacity: 0, y: 18, scale: 0.96 },
		animate: docked ? { opacity: 1, y: 0, scale: 1 } : { opacity: 0, y: 18, scale: 0.96 },
		transition: { duration: 0.55, delay: docked ? 0.05 + i * 0.08 : 0, ease },
	});

	return (
		// Border frame around the 2:1 area; cells sit inside with their own borders.
		<div
			ref={cardRef}
			id="card"
			className="relative grid aspect-[2/1] w-full max-w-[1080px] grid-cols-4 grid-rows-2 gap-4 border border-ink/15 p-5"
		>
			{/* Tall brand cell — reserves the left column; the logo docks over it. */}
			<motion.div
				ref={brandRef}
				initial={{ opacity: 0 }}
				animate={{ opacity: docked ? 1 : 0 }}
				transition={{ duration: 0.5, ease }}
				className={`row-span-2 ${CARD_SURFACE}`}
			/>

			{/* Headline performance tile (wide), top-left aligned */}
			<motion.div {...cellAnim(0)} className={`col-span-2 flex flex-col gap-6 p-7 ${CARD_SURFACE}`}>
				<div className="flex items-center gap-4">
					<div className="flex h-14 w-14 items-center justify-center rounded-xl bg-ink/5">
						<img src="/images/rust.svg" alt="Rust" className="h-7 w-7 opacity-80" />
					</div>
					<span className="text-2xl font-semibold tracking-[-0.015em] text-ink">Faster, lighter, cheaper than a sandbox</span>
				</div>
				<div className="flex gap-10">
					{perfStats.map((s) => (
						<div key={s.l} className="flex flex-col gap-0.5">
							<span className="text-4xl font-semibold tracking-[-0.02em] text-ink">{s.n}</span>
							<span className="text-sm leading-snug text-ink-soft">{s.l}</span>
						</div>
					))}
				</div>
			</motion.div>

			{/* Run any agent — overlapping, slightly-rotated logo pile + title */}
			<motion.div {...cellAnim(1)} className={`flex flex-col items-start gap-4 p-6 text-left ${CARD_SURFACE}`}>
				<div className="flex items-center pl-1">
					{agentLogos.map((a, i) => (
						<div
							key={a.name}
							style={{
								transform: `rotate(${[-14, -5, 5, 14][i] ?? 0}deg)`,
								marginLeft: i === 0 ? 0 : -12,
								zIndex: i,
							}}
							className="relative flex h-11 w-11 items-center justify-center rounded-xl border border-ink/10 bg-white shadow-[0_2px_8px_-2px_rgba(20,20,22,0.18)]"
						>
							<img src={a.src} alt={a.name} className="h-6 w-6 object-contain" />
						</div>
					))}
				</div>
				<span className="text-xl font-semibold tracking-[-0.015em] text-ink">Run any agent or BYO agent</span>
			</motion.div>

			{/* Feature cells — icon + large title, top-left aligned. */}
			{tiles.map((tile, i) => {
				const Icon = tile.Icon;
				return (
					<motion.div
						key={tile.title}
						{...cellAnim(i + 2)}
						className={`flex flex-col items-start gap-4 p-6 text-left ${CARD_SURFACE}`}
					>
						<div className="flex h-14 w-14 items-center justify-center rounded-xl bg-ink/5">
							{tile.img ? (
								<img src={tile.img} alt="" className="h-7 w-7 rounded" />
							) : (
								<Icon className="h-7 w-7 text-ink-soft" strokeWidth={1.75} />
							)}
						</div>
						<span className="text-xl font-semibold tracking-[-0.015em] text-ink">{tile.title}</span>
					</motion.div>
				);
			})}

			{/* Floating brand block: centered + large on load, then docks into the cell. */}
			<div className="pointer-events-none absolute inset-0 flex items-center justify-center">
				<motion.div
					initial={{ x: 0, y: 0, scale: 1.35 }}
					animate={docked ? { x: target.x, y: target.y, scale: 1 } : { x: 0, y: 0, scale: 1.35 }}
					transition={{ duration: DOCK_DURATION_SEC, ease }}
					className="flex flex-col items-center gap-5 text-center"
				>
					<div className="logo-anim flex items-center justify-center">
						<AnimatedAgentOSLogo className="h-12 w-auto" drawDurationSec={LOGO_DRAW_SEC} />
					</div>
					{/* "Introducing" then "v0.2" stagger in shortly after the logo starts. */}
					<div className="flex flex-col items-center gap-2">
						<motion.span
							initial={{ opacity: 0, y: 6 }}
							animate={{ opacity: 1, y: 0 }}
							transition={{ duration: 0.5, delay: 0.2, ease }}
							className="font-mono text-sm uppercase tracking-[0.24em] text-pine"
						>
							Introducing
						</motion.span>
						<motion.span
							initial={{ opacity: 0, y: 8 }}
							animate={{ opacity: 1, y: 0 }}
							transition={{ duration: 0.5, delay: 0.4, ease }}
							className="text-6xl font-semibold tracking-[-0.02em] text-ink"
						>
							v0.2
						</motion.span>
					</div>
				</motion.div>
			</div>
		</div>
	);
}
