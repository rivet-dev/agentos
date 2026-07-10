"use client";

import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { ArrowUpRight, ChevronLeft, ChevronRight } from "lucide-react";
import type { RegistryEntryBase } from "../../../data/registry";
import type { RegistryIconName } from "../../../data/registry-icons";
import { RegistryIconTile } from "./RegistryIconTile";

// The slim, serializable slice of a RegistryEntry the storefront renders. The
// index page maps the full registry into this shape so multi-line config
// examples never ship in the island's serialized props.
export interface RegistryCardEntry {
	slug: string;
	title: string;
	description: string;
	types: RegistryEntryBase["types"];
	status: "available" | "coming-soon" | "docs" | "config" | "external";
	featured?: boolean;
	beta?: boolean;
	icon?: RegistryIconName;
	image?: string;
	// Only set for external entries; the row links straight to it.
	href?: string;
}

const CATEGORY_ORDER: {
	type: RegistryCardEntry["types"][number];
	label: string;
	description: string;
}[] = [
	{
		type: "agent",
		label: "Agents",
		description:
			"Coding agents with programmatic API access and universal transcript format (ACP).",
	},
	{
		type: "file-system",
		label: "File Systems",
		description:
			"Mount these file systems as the root or at any sub-path inside the agent's environment.",
	},
	{
		type: "browser",
		label: "Browsers",
		description:
			"Let agents browse the web from inside the VM with cloud browser providers.",
	},
	{
		type: "sandbox-extension",
		label: "Sandbox Mounting",
		description:
			"agentOS is a hybrid OS. Mount sandbox file systems and interact with them via tools for heavier workloads. Use agentOS natively for lightweight tasks.",
	},
	{
		type: "software",
		label: "Software",
		description:
			"Wasm command packages that run inside the agent's environment. Install individually or use meta-packages.",
	},
	{
		type: "tool",
		label: "Bindings",
		description:
			"Host-side tools and integrations that extend agent capabilities.",
	},
	{
		type: "deploy",
		label: "Deploy",
		description: "Run agentOS in production on the platform of your choice.",
	},
];

const MAX_SHELF_ITEMS = 6;
const CAROUSEL_INTERVAL = 6000;

// The featured banner's editorial card surface: the site's premium porcelain
// card treatment with a deeper resting shadow for the storefront hero moment.
const BANNER_SURFACE =
	"rounded-3xl bg-gradient-to-b from-white to-[#f9f9fa] ring-1 ring-ink/[0.08] " +
	"shadow-[inset_0_1px_0_rgba(255,255,255,0.9),0_1px_2px_-1px_rgba(20,20,22,0.10),0_16px_40px_-20px_rgba(20,20,22,0.25)]";

function entryHref(hrefBase: string, entry: RegistryCardEntry) {
	// External entries (deploy targets) link straight to their guide; there is
	// no detail page for them.
	if (entry.status === "external" && entry.href) return entry.href;
	return `${hrefBase}/${entry.slug}`;
}

function categoryLabelFor(entry: RegistryCardEntry) {
	return (
		CATEGORY_ORDER.find((category) => entry.types.includes(category.type))
			?.label ?? "Package"
	);
}

const PILL_BASE =
	"inline-flex h-7 shrink-0 items-center justify-center gap-1 rounded-md px-3 text-[13px] font-medium transition-colors duration-200 motion-reduce:transition-none";
const PILL_ACTIVE =
	"border border-ink/15 bg-white/45 text-ink-soft group-hover:border-ink/40 group-hover:text-ink";

// The trailing App Store-style action affordance, in the site's ghost-button
// language. Rendered as a span because the whole row is the link; nesting an
// interactive element inside it would break keyboard navigation.
function GetPill({ entry }: { entry: RegistryCardEntry }) {
	if (entry.status === "coming-soon") {
		return (
			<span className={`${PILL_BASE} border border-ink/10 text-ink-faint`}>
				Soon
			</span>
		);
	}
	if (entry.status === "external") {
		return (
			<span className={`${PILL_BASE} ${PILL_ACTIVE}`}>
				View
				<ArrowUpRight className="h-3 w-3" />
			</span>
		);
	}
	const label =
		entry.status === "docs" ? "Docs" : entry.status === "config" ? "Setup" : "Get";
	return <span className={`${PILL_BASE} ${PILL_ACTIVE}`}>{label}</span>;
}

function AppRow({
	entry,
	hrefBase,
}: {
	entry: RegistryCardEntry;
	hrefBase: string;
}) {
	const external = entry.status === "external";
	const comingSoon = entry.status === "coming-soon";

	return (
		<a
			href={entryHref(hrefBase, entry)}
			{...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
			className="group flex items-center gap-4 py-3.5 no-underline"
		>
			<RegistryIconTile
				title={entry.title}
				image={entry.image}
				icon={entry.icon}
				size={48}
				className={
					comingSoon
						? "opacity-70 saturate-50"
						: "transition-shadow duration-200 group-hover:ring-ink/[0.14]"
				}
			/>
			<div className="min-w-0 flex-1">
				<div className="flex items-center gap-2">
					<h3
						className={`truncate text-[15px] font-medium ${comingSoon ? "text-ink-soft" : "text-ink"}`}
					>
						{entry.title}
					</h3>
					{entry.beta && (
						<span className="shrink-0 rounded-full border border-ink/15 px-1.5 py-0.5 text-[10px] text-ink-faint">
							Beta
						</span>
					)}
				</div>
				<p className="mt-0.5 truncate text-[13px] leading-snug text-ink-soft">
					{entry.description}
				</p>
			</div>
			<GetPill entry={entry} />
		</a>
	);
}

function Shelf({
	label,
	description,
	entries,
	hrefBase,
}: {
	label: string;
	description: string;
	entries: RegistryCardEntry[];
	hrefBase: string;
}) {
	const [expanded, setExpanded] = useState(false);
	const collapsible = entries.length > MAX_SHELF_ITEMS;

	// The first MAX_SHELF_ITEMS stay mounted in a fixed column-major layout;
	// the remainder animate open/closed below. Splitting once (rather than
	// re-splitting on expand) keeps rows from jumping between columns as the
	// extra section reveals. Column-major chunking mirrors App Store charts —
	// 1, 2, 3 down the left column — where CSS grid flow would order rows
	// left-to-right instead.
	const base = entries.slice(0, MAX_SHELF_ITEMS);
	const extra = collapsible ? entries.slice(MAX_SHELF_ITEMS) : [];

	const renderColumns = (list: RegistryCardEntry[], seam: boolean) => {
		const split = Math.ceil(list.length / 2);
		const columns = [list.slice(0, split), list.slice(split)].filter(
			(column) => column.length > 0,
		);
		return (
			<div className="grid gap-x-10 sm:grid-cols-2">
				{columns.map((column, columnIndex) => (
					<div
						key={columnIndex}
						className={`flex flex-col divide-y divide-ink/[0.07] ${
							// Keep a hairline where the extra section meets the base rows,
							// and where columns stack on mobile.
							seam || columnIndex > 0
								? "border-t border-ink/[0.07]"
								: ""
						} ${columnIndex > 0 && !seam ? "sm:border-t-0" : ""}`}
					>
						{column.map((entry) => (
							<AppRow key={entry.slug} entry={entry} hrefBase={hrefBase} />
						))}
					</div>
				))}
			</div>
		);
	};

	return (
		<section className="mb-14 border-t border-ink/[0.08] pt-10">
			<div className="mb-4 flex items-end justify-between gap-4">
				<div>
					<h2 className="text-2xl font-medium tracking-[-0.015em] text-ink">
						{label}
					</h2>
					<p className="mt-1 max-w-2xl text-sm leading-relaxed text-ink-soft">
						{description}
					</p>
				</div>
				{collapsible && (
					<button
						type="button"
						onClick={() => setExpanded((value) => !value)}
						aria-expanded={expanded}
						className="shrink-0 pb-0.5 text-sm font-medium text-pine transition-colors hover:text-ink"
					>
						{expanded ? "Show Less" : `See All (${entries.length})`}
					</button>
				)}
			</div>
			{renderColumns(base, false)}
			{collapsible && (
				<div
					// grid-rows 0fr → 1fr animates to the content's natural height with
					// no measurement, so it stays smooth across viewport widths.
					className="grid transition-[grid-template-rows] duration-300 ease-out motion-reduce:transition-none"
					style={{ gridTemplateRows: expanded ? "1fr" : "0fr" }}
				>
					<div
						className={`overflow-hidden transition-opacity duration-300 ease-out motion-reduce:transition-none ${
							expanded ? "opacity-100" : "opacity-0"
						}`}
						aria-hidden={!expanded}
					>
						{renderColumns(extra, true)}
					</div>
				</div>
			)}
		</section>
	);
}

function FeaturedBanner({
	entries,
	hrefBase,
}: {
	entries: RegistryCardEntry[];
	hrefBase: string;
}) {
	const [index, setIndex] = useState(0);
	const [direction, setDirection] = useState(1);
	const [paused, setPaused] = useState(false);
	const reducedMotion = useReducedMotion();

	const go = useCallback(
		(next: number) => {
			if (next === index) return;
			setDirection(next > index ? 1 : -1);
			setIndex(next);
		},
		[index],
	);

	const goNext = useCallback(() => {
		setDirection(1);
		setIndex((current) => (current + 1) % entries.length);
	}, [entries.length]);

	const goPrev = useCallback(() => {
		setDirection(-1);
		setIndex((current) => (current - 1 + entries.length) % entries.length);
	}, [entries.length]);

	useEffect(() => {
		if (paused || reducedMotion || entries.length < 2) return;
		const timer = setInterval(goNext, CAROUSEL_INTERVAL);
		return () => clearInterval(timer);
		// Depending on `index` restarts the interval after manual navigation.
	}, [paused, reducedMotion, entries.length, goNext, index]);

	const entry = entries[index];

	const variants = {
		enter: (dir: number) => ({ x: reducedMotion ? 0 : dir * 48, opacity: 0 }),
		center: { x: 0, opacity: 1 },
		exit: (dir: number) => ({ x: reducedMotion ? 0 : dir * -48, opacity: 0 }),
	};

	return (
		<div
			className="group/banner relative mb-16"
			onMouseEnter={() => setPaused(true)}
			onMouseLeave={() => setPaused(false)}
			onFocusCapture={() => setPaused(true)}
			onBlurCapture={() => setPaused(false)}
		>
			<div className={`relative overflow-hidden ${BANNER_SURFACE}`}>
				{/* initial={false} keeps the SSR'd first slide visible; an enter
				    animation here would server-render the hero invisible until
				    hydration. */}
				<AnimatePresence mode="wait" custom={direction} initial={false}>
					<motion.a
						key={entry.slug}
						href={entryHref(hrefBase, entry)}
						custom={direction}
						variants={variants}
						initial="enter"
						animate="center"
						exit="exit"
						transition={{ duration: 0.45, ease: [0.32, 0.72, 0, 1] }}
						className="flex h-[300px] flex-col items-start justify-center gap-6 px-7 no-underline sm:h-[320px] sm:flex-row sm:items-center sm:gap-12 sm:px-12"
					>
						<div className="min-w-0 flex-1">
							<h3 className="text-3xl font-medium tracking-[-0.015em] text-ink md:text-4xl">
								{entry.title}
							</h3>
							<p className="mt-3 max-w-lg line-clamp-2 text-base leading-relaxed text-ink-soft">
								{entry.description}
							</p>
							<span className="selection-dark mt-6 inline-flex items-center rounded-md bg-ink px-4 py-2 text-sm font-medium text-cream transition-colors duration-200 group-hover/banner:bg-ink/85 motion-reduce:transition-none">
								View
							</span>
						</div>
						<RegistryIconTile
							title={entry.title}
							image={entry.image}
							icon={entry.icon}
							size={96}
							className="order-first sm:order-last"
						/>
					</motion.a>
				</AnimatePresence>
			</div>

			{entries.length > 1 && (
				<>
					<button
						type="button"
						aria-label="Previous"
						onClick={goPrev}
						className="absolute left-3 top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full bg-white/85 text-ink-soft opacity-0 ring-1 ring-ink/10 transition-opacity duration-200 hover:text-ink focus-visible:opacity-100 group-hover/banner:opacity-100"
					>
						<ChevronLeft className="h-4 w-4" />
					</button>
					<button
						type="button"
						aria-label="Next"
						onClick={goNext}
						className="absolute right-3 top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full bg-white/85 text-ink-soft opacity-0 ring-1 ring-ink/10 transition-opacity duration-200 hover:text-ink focus-visible:opacity-100 group-hover/banner:opacity-100"
					>
						<ChevronRight className="h-4 w-4" />
					</button>

					<div className="mt-4 flex items-center justify-center gap-2">
						{entries.map((candidate, candidateIndex) => (
							<button
								key={candidate.slug}
								type="button"
								aria-label={`Show ${candidate.title}`}
								aria-current={candidateIndex === index}
								onClick={() => go(candidateIndex)}
								className={`h-1.5 rounded-full transition-all duration-300 motion-reduce:transition-none ${
									candidateIndex === index
										? "w-5 bg-pine"
										: "w-1.5 bg-ink/25 hover:bg-ink/40"
								}`}
							/>
						))}
					</div>
				</>
			)}
		</div>
	);
}

function FooterCta() {
	return (
		<div className="mt-4 border-t border-ink/[0.08] pt-10 text-center">
			<p className="mb-5 text-sm text-ink-soft">
				Want to add your own package to the registry?
			</p>
			<div className="flex flex-wrap items-center justify-center gap-3">
				<a
					href="https://github.com/rivet-dev/agentos/blob/main/registry/CONTRIBUTING.md"
					target="_blank"
					rel="noopener noreferrer"
					className="selection-dark inline-flex items-center gap-2 rounded-lg bg-ink px-5 py-2.5 text-sm font-medium text-cream no-underline transition-colors hover:bg-ink/85"
				>
					Publish a Package
					<ArrowUpRight className="h-3.5 w-3.5" />
				</a>
				<a
					href="https://github.com/rivet-dev/agentos/issues"
					target="_blank"
					rel="noopener noreferrer"
					className="inline-flex items-center gap-2 rounded-lg border border-ink/20 px-5 py-2.5 text-sm font-medium text-ink-soft no-underline transition-colors hover:border-ink/40 hover:text-ink"
				>
					Request an Extension
					<ArrowUpRight className="h-3.5 w-3.5" />
				</a>
			</div>
		</div>
	);
}

export default function RegistryPageClient({
	entries,
	hrefBase = "/registry",
}: {
	entries: RegistryCardEntry[];
	hrefBase?: string;
}) {
	const featured = entries.filter((entry) => entry.featured);

	const categories = CATEGORY_ORDER.map(({ type, label, description }) => ({
		label,
		description,
		entries: entries.filter((entry) => entry.types.includes(type)),
	})).filter((category) => category.entries.length > 0);

	return (
		<>
			{featured.length > 0 && (
				<FeaturedBanner entries={featured} hrefBase={hrefBase} />
			)}

			{categories.map((category) => (
				<Shelf
					key={category.label}
					label={category.label}
					description={category.description}
					entries={category.entries}
					hrefBase={hrefBase}
				/>
			))}

			<FooterCta />
		</>
	);
}
