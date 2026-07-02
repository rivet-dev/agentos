import type { ReactNode } from "react";
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
function CheckIcon({ className }: { className?: string }) {
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
