import type { ReactNode } from "react";
import agentOsHeroLogo from "./assets/agentos-hero-logo.svg";
import { type ActionErrorLayer, isInspectorActionError } from "./lib/actor-client";
import { cn } from "./lib/cn";
import React, { useState } from "react";

/** Centered empty/placeholder state filling the tab body. */
export function AgentOsEmpty({ children }: { children: ReactNode }) {
	return (
		<div className="flex h-full flex-1 items-center justify-center p-8 text-center text-sm text-muted-foreground">
			{children}
		</div>
	);
}

const LAYER_LABEL: Record<ActionErrorLayer, string> = {
	gateway: "Gateway unreachable",
	auth: "Not authorized",
	contract: "Not supported by this runtime",
	runtime: "Runtime error",
	timeout: "Timed out",
};

/** "This runtime doesn't expose X" empty state for contract-layer failures —
 * the graceful-degradation path for actors built against other runtimes. */
export function UnsupportedAction({ action, className }: { action?: string; className?: string }) {
	return (
		<div className={cn("flex h-full flex-1 flex-col items-center justify-center gap-1 p-8 text-center", className)}>
			<span className="text-sm text-muted-foreground">Not supported by this runtime</span>
			{action ? (
				<span className="font-mono text-xs text-muted-foreground/70">
					the actor does not expose <span className="font-semibold">{action}</span>
				</span>
			) : null}
		</div>
	);
}

/** Compact inline error note: layer label + message + hint. Used by inline
 * (non-suspense) error paths; the suspense path renders via TabBoundary. */
export function ActionErrorNote({ error, className }: { error: unknown; className?: string }) {
	if (isInspectorActionError(error) && error.layer === "contract") {
		return <UnsupportedAction action={error.action} className={className} />;
	}
	const layer = isInspectorActionError(error) ? error.layer : undefined;
	const message = error instanceof Error ? error.message : String(error);
	const hint = isInspectorActionError(error) ? error.hint : undefined;
	return (
		<div className={cn("flex flex-col gap-1 p-4 text-sm", className)}>
			<div className="flex items-center gap-2">
				<StatusDot color={layer === "timeout" || layer === "gateway" ? "amber" : "red"} />
				<span className="font-medium text-destructive">
					{layer ? LAYER_LABEL[layer] : "Error"}
				</span>
			</div>
			<div className="text-muted-foreground">{message}</div>
			{hint ? <div className="text-xs text-muted-foreground/70">{hint}</div> : null}
		</div>
	);
}

export type DotColor = "green" | "amber" | "red" | "muted";
const DOT_CLASS: Record<DotColor, string> = {
	green: "text-green-500",
	amber: "text-amber-500",
	red: "text-red-500",
	muted: "text-muted-foreground/50",
};

export function StatusDot({ color, className }: { color: DotColor; className?: string }) {
	return (
		<svg
			viewBox="0 0 8 8"
			className={cn("size-2 shrink-0 fill-current", DOT_CLASS[color], className)}
			aria-hidden="true"
		>
			<circle cx="4" cy="4" r="4" />
		</svg>
	);
}

/** Inline chevron (replaces @rivet-gg/icons faChevronRight). */
export function ChevronRight({ className }: { className?: string }) {
	return (
		<svg
			className={className}
			viewBox="0 0 16 16"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
		>
			<path d="M6 4l4 4-4 4" />
		</svg>
	);
}

/** Clipboard + check glyphs for the copy button. */
function CopyIcon({ className }: { className?: string }) {
	return (
		<svg
			className={className}
			viewBox="0 0 16 16"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.5"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
		>
			<rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
			<path d="M10.5 5.5V4A1.5 1.5 0 0 0 9 2.5H4A1.5 1.5 0 0 0 2.5 4v5A1.5 1.5 0 0 0 4 10.5h1.5" />
		</svg>
	);
}
export function CheckIcon({ className }: { className?: string }) {
	return (
		<svg
			className={className}
			viewBox="0 0 16 16"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
		>
			<path d="M3 8.5l3.5 3.5L13 4.5" />
		</svg>
	);
}

/** Small, subtle copy-to-clipboard button. Shows a check briefly on success. */
export function CopyButton({ value, className }: { value: string; className?: string }) {
	const [copied, setCopied] = useState(false);
	return (
		<button
			type="button"
			aria-label={copied ? "Copied" : "Copy"}
			title={copied ? "Copied" : "Copy"}
			onClick={(e) => {
				e.stopPropagation();
				navigator.clipboard
					?.writeText(value)
					.then(() => {
						setCopied(true);
						setTimeout(() => setCopied(false), 1200);
					})
					.catch(() => {});
			}}
			className={cn(
				"inline-flex size-5 items-center justify-center rounded text-muted-foreground/40 transition-colors hover:bg-muted hover:text-foreground",
				copied && "text-green-500",
				className,
			)}
		>
			{copied ? <CheckIcon className="size-3" /> : <CopyIcon className="size-3.5" />}
		</button>
	);
}

/** agentOS wordmark — the website hero logo, verbatim, bundled as an asset.
 * The source SVG is stroked black (drawn for light backgrounds); `invert`
 * flips it white for the dark theme, and the low opacity makes the
 * empty-state watermark. */
export function AgentOsWordmark({ className }: { className?: string }) {
	return (
		<img
			src={agentOsHeroLogo}
			alt=""
			aria-hidden="true"
			draggable={false}
			className={cn("select-none opacity-[0.14] invert", className)}
		/>
	);
}

/** Small bordered icon button for tab chrome (new/refresh/upload/…). The
 * accessible name comes from `title`, which doubles as the tooltip. */
export function IconButton({
	title,
	onClick,
	disabled,
	destructive,
	className,
	children,
}: {
	title: string;
	onClick: () => void;
	disabled?: boolean;
	destructive?: boolean;
	className?: string;
	children: ReactNode;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			disabled={disabled}
			title={title}
			aria-label={title}
			className={cn(
				"inline-flex size-6 shrink-0 items-center justify-center rounded border text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40",
				destructive && "border-destructive/50 text-destructive hover:bg-destructive/10 hover:text-destructive",
				className,
			)}
		>
			{children}
		</button>
	);
}

const iconProps = {
	viewBox: "0 0 16 16",
	fill: "none",
	stroke: "currentColor",
	strokeWidth: 1.5,
	strokeLinecap: "round",
	strokeLinejoin: "round",
	"aria-hidden": true,
} as const;

export function PlusIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M8 3.5v9M3.5 8h9" />
		</svg>
	);
}

export function ArrowLeftIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M13 8H3M7 4L3 8l4 4" />
		</svg>
	);
}

export function RefreshIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" />
			<path d="M13.5 1.5v3h-3" />
		</svg>
	);
}

export function FolderPlusIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M1.5 4.5A1 1 0 0 1 2.5 3.5h3l1.5 1.5h5.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-10a1 1 0 0 1-1-1z" />
			<path d="M8 7.5v3M6.5 9h3" />
		</svg>
	);
}

export function UploadIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M2.5 10.5v2a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-2" />
			<path d="M8 10V3M4.5 6.5L8 3l3.5 3.5" />
		</svg>
	);
}

export function DownloadIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M2.5 10.5v2a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-2" />
			<path d="M8 3v7M4.5 6.5L8 10l3.5-3.5" />
		</svg>
	);
}

export function PencilIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M11.3 2.7a1.4 1.4 0 0 1 2 2L5.5 12.5l-3 .8.8-3z" />
		</svg>
	);
}

export function TrashIcon({ className }: { className?: string }) {
	return (
		<svg className={className} {...iconProps}>
			<path d="M3 4.5h10M6.5 4.5V3.5a1 1 0 0 1 1-1h1a1 1 0 0 1 1 1v1M4.5 4.5l.5 8a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1l.5-8" />
			<path d="M6.5 7v4M9.5 7v4" />
		</svg>
	);
}

/** Inline folder/file glyphs for the filesystem tree. */
export function FileGlyph({ dir, className }: { dir: boolean; className?: string }) {
	return (
		<svg
			className={className}
			viewBox="0 0 16 16"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.5"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
		>
			{dir ? (
				<path d="M1.5 4.5A1 1 0 0 1 2.5 3.5h3l1.5 1.5h5.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-10a1 1 0 0 1-1-1z" />
			) : (
				<>
					<path d="M4 1.5h5l3 3v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1z" />
					<path d="M9 1.5v3h3" />
				</>
			)}
		</svg>
	);
}

/** Human-readable byte size; "—" for nullish. */
export function formatBytes(bytes?: number | null): string {
	if (bytes == null) return "—";
	if (bytes < 1024) return `${bytes} B`;
	const units = ["KiB", "MiB", "GiB", "TiB"];
	let value = bytes / 1024;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	const f = value >= 100 ? value.toFixed(0) : value.toFixed(1);
	return `${f.replace(/\.0$/, "")} ${units[unit]}`;
}

/** Compact relative time ("5s ago", "3m ago") from an epoch-ms value. */
export function relativeTime(ms?: number | null): string {
	if (ms == null) return "—";
	const diff = Date.now() - ms;
	if (diff < 0) return "just now";
	const s = Math.floor(diff / 1000);
	if (s < 60) return `${s}s ago`;
	const m = Math.floor(s / 60);
	if (m < 60) return `${m}m ago`;
	const h = Math.floor(m / 60);
	if (h < 24) return `${h}h ago`;
	return `${Math.floor(h / 24)}d ago`;
}
