import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { AgentOsEmpty, ChevronRight, FileGlyph, formatBytes, relativeTime } from "../common";
import { cn } from "../lib/cn";
import { agentOsSource } from "../lib/source";
import type { FsEntry } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import React from "react";

function FileTreeItem({
	actorId,
	entry,
	depth,
	selectedPath,
	onSelect,
}: {
	actorId: string;
	entry: FsEntry;
	depth: number;
	selectedPath: string | null;
	onSelect: (e: FsEntry) => void;
}) {
	const [open, setOpen] = useState(false);
	const isSelected = entry.path === selectedPath;
	const expandable = entry.dir && !entry.virtual;
	const childrenQuery = useQuery(
		agentOsSource.listDirQueryOptions(actorId, entry.path, expandable && open),
	);

	return (
		<div>
			<div className="flex items-center gap-1" style={{ paddingLeft: `${depth * 12}px` }}>
				<button
					type="button"
					onClick={() => expandable && setOpen((v) => !v)}
					className={cn(
						"flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-muted/50",
						!expandable && "pointer-events-none opacity-0",
					)}
					disabled={!expandable}
				>
					<ChevronRight className={cn("size-3 transition-transform", open && "rotate-90")} />
				</button>
				<button
					type="button"
					onClick={() => (expandable ? setOpen((v) => !v) : entry.dir ? undefined : onSelect(entry))}
					className={cn(
						"flex min-w-0 flex-1 items-center gap-2 rounded px-2 py-1 text-left text-sm",
						isSelected ? "bg-muted text-foreground" : "hover:bg-muted/50",
					)}
				>
					<FileGlyph dir={entry.dir} className="size-3.5 shrink-0 text-muted-foreground" />
					<span className="flex-1 truncate">{entry.name}</span>
					{!entry.dir && entry.size != null ? (
						<span className="shrink-0 text-[10px] tabular-nums text-muted-foreground/60">
							{formatBytes(entry.size)}
						</span>
					) : null}
				</button>
			</div>
			{expandable && open ? (
				childrenQuery.isLoading ? (
					<div className="py-1 text-xs text-muted-foreground" style={{ paddingLeft: `${(depth + 1) * 12 + 28}px` }}>
						loading…
					</div>
				) : (
					(childrenQuery.data ?? []).map((child) => (
						<FileTreeItem
							key={child.path}
							actorId={actorId}
							entry={child}
							depth={depth + 1}
							selectedPath={selectedPath}
							onSelect={onSelect}
						/>
					))
				)
			) : null}
		</div>
	);
}

function FileViewer({ actorId, path }: { actorId: string; path: string | null }) {
	const { data, error, isFetching } = useQuery(
		agentOsSource.fileContentQueryOptions(actorId, path),
	);
	if (!path) return <AgentOsEmpty>Select a file to view its contents.</AgentOsEmpty>;
	if (error) return <AgentOsEmpty>Failed to read {path}: {(error as Error).message}</AgentOsEmpty>;
	if (!data || isFetching) return <AgentOsEmpty>Loading {path}…</AgentOsEmpty>;
	return (
		<div className="flex h-full flex-col">
			<div className="flex items-center gap-3 border-b px-4 py-3">
				<span className="truncate font-mono text-sm">{data.path}</span>
				<span className="ml-auto shrink-0 text-xs text-muted-foreground">{formatBytes(data.sizeBytes)}</span>
				<span className="shrink-0 text-xs text-muted-foreground">{relativeTime(data.mtimeMs)}</span>
			</div>
			<ScrollArea className="min-h-0 flex-1">
				{data.text === null ? (
					<div className="flex h-full items-center justify-center p-8 text-center text-sm text-muted-foreground">
						Binary file ({formatBytes(data.sizeBytes)}) — preview unavailable.
					</div>
				) : (
					<pre className="whitespace-pre-wrap break-words p-4 font-mono text-xs leading-relaxed">
						{data.text}
					</pre>
				)}
			</ScrollArea>
		</div>
	);
}

/** Normalize a user-typed root: ensure a single leading slash, collapse
 * repeated slashes, and drop a trailing slash (except for root itself). */
function normalizeRoot(input: string): string {
	let s = input.trim();
	if (s === "") return "/";
	if (!s.startsWith("/")) s = `/${s}`;
	s = s.replace(/\/+/g, "/");
	if (s.length > 1 && s.endsWith("/")) s = s.slice(0, -1);
	return s;
}

export function FilesystemTabConnected({ actorId }: { actorId: string }) {
	// `root` drives the listing/refetch; `draft` tracks keystrokes locally so
	// typing never refetches. `root` is committed 500ms after typing stops (or
	// immediately on Enter) so we don't refetch on every keystroke.
	const [root, setRoot] = useState("/");
	const [draft, setDraft] = useState("/");
	const [selectedPath, setSelectedPath] = useState<string | null>(null);

	const rootsQuery = useQuery(agentOsSource.listDirQueryOptions(actorId, root));
	// `null` data = the path is not a listable directory (does not exist / is a
	// file); `[]` = an empty directory. Keep them distinct for the message.
	const notADir = rootsQuery.data === null;
	const roots = rootsQuery.data ?? [];

	// Debounce: commit the normalized draft once the user pauses for 500ms.
	useEffect(() => {
		const id = setTimeout(() => {
			const next = normalizeRoot(draft);
			setRoot((cur) => (next !== cur ? next : cur));
		}, 500);
		return () => clearTimeout(id);
	}, [draft]);

	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-2/5 flex-col border-r">
				<div className="border-b px-3 py-1.5">
					<input
						value={draft}
						onChange={(e) => setDraft(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") {
								const next = normalizeRoot(draft);
								setDraft(next);
								setRoot((cur) => (next !== cur ? next : cur));
							}
						}}
						spellCheck={false}
						autoComplete="off"
						aria-label="Root directory"
						placeholder="/"
						className="w-full bg-transparent font-mono text-xs text-muted-foreground outline-none placeholder:text-muted-foreground/40 focus:text-foreground"
					/>
				</div>
				<ScrollArea className="min-h-0 flex-1 p-2">
					{rootsQuery.isLoading ? (
						<AgentOsEmpty>Loading {root}…</AgentOsEmpty>
					) : rootsQuery.error ? (
						<AgentOsEmpty>
							Failed to list {root}: {(rootsQuery.error as Error).message}
						</AgentOsEmpty>
					) : notADir ? (
						<AgentOsEmpty>Not a directory, or does not exist: {root}</AgentOsEmpty>
					) : roots.length === 0 ? (
						<AgentOsEmpty>Empty directory.</AgentOsEmpty>
					) : (
						roots.map((entry) => (
							<FileTreeItem
								key={entry.path}
								actorId={actorId}
								entry={entry}
								depth={0}
								selectedPath={selectedPath}
								onSelect={(e) => setSelectedPath(e.path)}
							/>
						))
					)}
				</ScrollArea>
			</div>
			<div className="min-h-0 w-3/5">
				<FileViewer actorId={actorId} path={selectedPath} />
			</div>
		</div>
	);
}
