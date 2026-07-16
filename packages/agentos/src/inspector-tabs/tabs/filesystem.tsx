import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import {
	ActionErrorNote,
	AgentOsEmpty,
	AgentOsWordmark,
	ArrowLeftIcon,
	CheckIcon,
	ChevronRight,
	DownloadIcon,
	FileGlyph,
	FolderPlusIcon,
	formatBytes,
	IconButton,
	PencilIcon,
	RefreshIcon,
	relativeTime,
	TrashIcon,
	UploadIcon,
} from "../common";
import { cn } from "../lib/cn";
import { agentOsSource } from "../lib/source";
import type { FsEntry } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import { VmBootGate } from "../vm-boot-gate";
import { VmStatusBadges } from "../vm-status-badges";
import React from "react";

const IMAGE_EXTENSIONS = /\.(png|jpe?g|gif|webp|svg|ico|bmp|avif)$/i;

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
					onClick={() => {
						if (expandable) setOpen((v) => !v);
						// Always report the click: the parent moves the path bar to the
						// clicked location so folder actions target it.
						onSelect(entry);
					}}
					className={cn(
						"flex min-w-0 flex-1 items-center gap-2 rounded px-2 py-1 text-left text-sm",
						isSelected ? "bg-muted text-foreground" : "hover:bg-muted/50",
					)}
				>
					<FileGlyph dir={entry.dir} className="size-3.5 shrink-0 text-muted-foreground" />
					<span className="flex-1 truncate">{entry.name}</span>
					{entry.symlink ? (
						<span
							title="Symbolic link (not followed)"
							className="shrink-0 font-mono text-[10px] text-muted-foreground/50"
						>
							→
						</span>
					) : null}
					{!entry.dir && entry.size != null ? (
						<span className="shrink-0 text-[10px] tabular-nums text-muted-foreground/60">
							{formatBytes(entry.size)}
						</span>
					) : null}
				</button>
			</div>
			{/* No loading row: most directories list in well under a frame, so a
			    transient "loading…" reads as a glitch. Children simply appear. */}
			{expandable && open
				? (childrenQuery.data ?? []).map((child) => (
						<FileTreeItem
							key={child.path}
							actorId={actorId}
							entry={child}
							depth={depth + 1}
							selectedPath={selectedPath}
							onSelect={onSelect}
						/>
					))
				: null}
		</div>
	);
}

function downloadBytes(bytes: Uint8Array, filename: string): void {
	const url = URL.createObjectURL(new Blob([bytes as BlobPart]));
	const anchor = document.createElement("a");
	anchor.href = url;
	anchor.download = filename;
	anchor.click();
	URL.revokeObjectURL(url);
}

function FileViewer({
	actorId,
	path,
	onMutated,
	onDeleted,
	onRenamed,
}: {
	actorId: string;
	path: string | null;
	onMutated: () => void;
	onDeleted: () => void;
	onRenamed: (to: string) => void;
}) {
	const [force, setForce] = useState(false);
	const [renameDraft, setRenameDraft] = useState<string | null>(null);
	const [confirmingDelete, setConfirmingDelete] = useState(false);
	const [mutationError, setMutationError] = useState<unknown>(null);
	// Per-file view state resets when the selection changes.
	// biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset on path change
	useEffect(() => {
		setForce(false);
		setRenameDraft(null);
		setConfirmingDelete(false);
		setMutationError(null);
	}, [path]);
	const { data, error } = useQuery(
		agentOsSource.fileContentQueryOptions(actorId, path, force),
	);
	const imageUrl = useMemo(() => {
		if (!data?.bytes || data.text !== null || !IMAGE_EXTENSIONS.test(data.path)) return null;
		return URL.createObjectURL(new Blob([data.bytes as BlobPart]));
	}, [data]);
	useEffect(() => {
		return () => {
			if (imageUrl) URL.revokeObjectURL(imageUrl);
		};
	}, [imageUrl]);

	if (!path)
		return (
			<AgentOsEmpty>
				<div className="flex flex-col items-center gap-5">
					<AgentOsWordmark className="w-44" />
					<span>Select a file to view its contents.</span>
				</div>
			</AgentOsEmpty>
		);
	if (error) return <ActionErrorNote error={error} className="items-center justify-center text-center" />;
	// Only the very first open shows a loading state; switching files keeps the
	// previous content until the new one lands (keepPreviousData in source.ts).
	if (!data) return <AgentOsEmpty>Loading {path}…</AgentOsEmpty>;

	const filename = data.path.split("/").pop() ?? data.path;
	const rename = async () => {
		const to = renameDraft?.trim();
		if (!to || to === data.path) {
			setRenameDraft(null);
			return;
		}
		setMutationError(null);
		try {
			await agentOsSource.moveEntry(data.path, to);
			setRenameDraft(null);
			onRenamed(to);
			onMutated();
		} catch (err) {
			setMutationError(err);
		}
	};
	const remove = async () => {
		if (!confirmingDelete) {
			setConfirmingDelete(true);
			setTimeout(() => setConfirmingDelete(false), 3_000);
			return;
		}
		setMutationError(null);
		try {
			await agentOsSource.deleteFile(data.path, {});
			onDeleted();
			onMutated();
		} catch (err) {
			setMutationError(err);
		}
	};

	return (
		<div className="flex h-full flex-col">
			<div className="flex items-center gap-2 border-b px-4 py-2.5">
				{renameDraft !== null ? (
					<input
						value={renameDraft}
						onChange={(e) => setRenameDraft(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") void rename();
							if (e.key === "Escape") setRenameDraft(null);
						}}
						spellCheck={false}
						autoFocus
						aria-label="New path"
						className="min-w-0 flex-1 rounded border bg-background px-2 py-1 font-mono text-xs focus:outline-none"
					/>
				) : (
					<span className="min-w-0 flex-1 truncate font-mono text-sm">{data.path}</span>
				)}
				<span className="shrink-0 text-xs text-muted-foreground">{formatBytes(data.sizeBytes)}</span>
				<span className="shrink-0 text-xs text-muted-foreground">{relativeTime(data.mtimeMs)}</span>
				<IconButton
					title={data.bytes ? "Download this file" : "Load the file first"}
					disabled={!data.bytes}
					onClick={() => data.bytes && downloadBytes(data.bytes, filename)}
				>
					<DownloadIcon className="size-3.5" />
				</IconButton>
				<IconButton
					title={renameDraft !== null ? "Save new name" : "Rename this file"}
					onClick={() => (renameDraft !== null ? void rename() : setRenameDraft(data.path))}
				>
					{renameDraft !== null ? (
						<CheckIcon className="size-3.5" />
					) : (
						<PencilIcon className="size-3.5" />
					)}
				</IconButton>
				{/* Delete keeps a two-step confirm: the icon arms it, the explicit
				    text disarms accidental clicks on an irreversible action. */}
				{confirmingDelete ? (
					<button
						type="button"
						onClick={() => void remove()}
						className="shrink-0 rounded border border-destructive/50 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/10"
					>
						Confirm delete?
					</button>
				) : (
					<IconButton title="Delete this file" destructive onClick={() => void remove()}>
						<TrashIcon className="size-3.5" />
					</IconButton>
				)}
			</div>
			{mutationError ? <ActionErrorNote error={mutationError} className="border-b py-2" /> : null}
			<ScrollArea className="min-h-0 flex-1">
				{data.special ? (
					<div className="flex h-full items-center justify-center p-8 text-center text-sm text-muted-foreground">
						Device or stream — no readable contents.
					</div>
				) : data.oversize ? (
					<div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center text-sm text-muted-foreground">
						<span>
							Large file ({formatBytes(data.sizeBytes)}) — preview skipped to avoid dragging it
							through the gateway.
						</span>
						<button
							type="button"
							onClick={() => setForce(true)}
							className="rounded border px-2.5 py-1 text-xs transition-colors hover:bg-muted hover:text-foreground"
						>
							Load anyway
						</button>
					</div>
				) : data.text === null ? (
					imageUrl ? (
						<div className="flex items-start justify-center p-4">
							<img src={imageUrl} alt={filename} className="max-w-full rounded border" />
						</div>
					) : (
						<div className="flex h-full items-center justify-center p-8 text-center text-sm text-muted-foreground">
							Binary file ({formatBytes(data.sizeBytes)}) — use Download to save it.
						</div>
					)
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
/** Parent directory of a path ("/" for top-level entries). */
function parentDir(path: string): string {
	const idx = path.lastIndexOf("/");
	return idx <= 0 ? "/" : path.slice(0, idx);
}

function normalizeRoot(input: string): string {
	let s = input.trim();
	if (s === "") return "/";
	if (!s.startsWith("/")) s = `/${s}`;
	s = s.replace(/\/+/g, "/");
	if (s.length > 1 && s.endsWith("/")) s = s.slice(0, -1);
	return s;
}

const joinRoot = (root: string, name: string) => (root === "/" ? `/${name}` : `${root}/${name}`);

export function FilesystemTabConnected({ actorId }: { actorId: string }) {
	// The root filesystem is in-memory and served by the VM's kernel: a
	// sleeping VM has no file tree, and listing would boot it. Gate first.
	return (
		<VmBootGate
			actorId={actorId}
			note="VM not booted."
			actionLabel="Boot the VM and browse files"
		>
			<FilesystemLoaded actorId={actorId} />
		</VmBootGate>
	);
}

// Browsing state survives tab switches: the dashboard swaps the iframe per
// tab, so the root path and open file persist per actor (same pattern as the
// terminal's shells).
const fsStateKey = (actorId: string) => `agentos-inspector:fs:${actorId}`;

function loadFsState(actorId: string): { root: string; selectedPath: string | null } {
	try {
		const raw = sessionStorage.getItem(fsStateKey(actorId));
		if (raw) {
			const parsed = JSON.parse(raw) as { root?: unknown; selectedPath?: unknown };
			return {
				root: typeof parsed.root === "string" ? parsed.root : "/",
				selectedPath: typeof parsed.selectedPath === "string" ? parsed.selectedPath : null,
			};
		}
	} catch {
		// Malformed storage falls back to defaults.
	}
	return { root: "/", selectedPath: null };
}

function FilesystemLoaded({ actorId }: { actorId: string }) {
	// `root` drives the listing/refetch; `draft` tracks keystrokes locally so
	// typing never refetches. `root` is committed 500ms after typing stops (or
	// immediately on Enter) so we don't refetch on every keystroke.
	const initial = useRef(loadFsState(actorId)).current;
	const [root, setRoot] = useState(initial.root);
	const [draft, setDraft] = useState(initial.root);
	const [selectedPath, setSelectedPath] = useState<string | null>(initial.selectedPath);
	useEffect(() => {
		try {
			sessionStorage.setItem(fsStateKey(actorId), JSON.stringify({ root, selectedPath }));
		} catch (error) {
			console.warn("agentos inspector: failed to persist filesystem state", error);
		}
	}, [actorId, root, selectedPath]);
	const [newFolderDraft, setNewFolderDraft] = useState<string | null>(null);
	const [treeError, setTreeError] = useState<unknown>(null);
	const uploadInputRef = useRef<HTMLInputElement>(null);
	const queryClient = useQueryClient();

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

	// Every directory listing under this actor (the tree fetches per-level).
	const refreshTree = () =>
		queryClient.invalidateQueries({ queryKey: ["agent-os", actorId, "dir"] });

	// Folder actions target the location shown in the path bar, which follows
	// tree clicks (a folder moves it there; a file moves it to its parent).
	const currentDir = normalizeRoot(draft);

	const createFolder = async () => {
		const name = newFolderDraft?.trim();
		if (!name) {
			setNewFolderDraft(null);
			return;
		}
		setTreeError(null);
		try {
			await agentOsSource.mkdir(name.startsWith("/") ? name : joinRoot(currentDir, name));
			setNewFolderDraft(null);
			await refreshTree();
		} catch (error) {
			setTreeError(error);
		}
	};

	const upload = async (file: File) => {
		setTreeError(null);
		try {
			const bytes = new Uint8Array(await file.arrayBuffer());
			await agentOsSource.writeFile(joinRoot(currentDir, file.name), bytes);
			await refreshTree();
		} catch (error) {
			setTreeError(error);
		}
	};

	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-64 shrink-0 flex-col border-r">
				<div className="flex items-center gap-1 px-3 pb-1 pt-2.5">
					<span className="text-[11px] font-medium text-muted-foreground">Files</span>
					<span className="ml-auto" />
					<IconButton title="Refresh the tree" onClick={() => void refreshTree()}>
						<RefreshIcon className="size-3.5" />
					</IconButton>
					<IconButton
						title={`New folder under ${currentDir}`}
						onClick={() => setNewFolderDraft((v) => (v === null ? "" : null))}
					>
						<FolderPlusIcon className="size-3.5" />
					</IconButton>
					<IconButton
						title={`Upload a file into ${currentDir}`}
						onClick={() => uploadInputRef.current?.click()}
					>
						<UploadIcon className="size-3.5" />
					</IconButton>
					<input
						ref={uploadInputRef}
						type="file"
						className="hidden"
						onChange={(e) => {
							const file = e.target.files?.[0];
							e.target.value = "";
							if (file) void upload(file);
						}}
					/>
				</div>
				{/* The browsing root gets its own bar: sharing the header row with
				    the action icons left it truncated and cramped. Clicking into a
				    folder moves the bar there, so the arrow walks back up. */}
				<div className="flex items-center gap-1 border-b px-3 pb-1.5">
					<IconButton
						title={currentDir === "/" ? "Already at the root" : `Up to ${parentDir(currentDir)}`}
						disabled={currentDir === "/"}
						onClick={() => {
							const up = parentDir(currentDir);
							setDraft(up);
							setRoot((cur) => (up !== cur ? up : cur));
						}}
						className="size-5"
					>
						<ArrowLeftIcon className="size-3.5" />
					</IconButton>
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
				{newFolderDraft !== null ? (
					<div className="border-b px-3 py-1.5">
						<input
							value={newFolderDraft}
							onChange={(e) => setNewFolderDraft(e.target.value)}
							onKeyDown={(e) => {
								if (e.key === "Enter") void createFolder();
								if (e.key === "Escape") setNewFolderDraft(null);
							}}
							spellCheck={false}
							autoFocus
							aria-label="New folder name"
							placeholder={`folder name (created under ${currentDir})`}
							className="w-full rounded border bg-background px-2 py-1 font-mono text-xs focus:outline-none"
						/>
					</div>
				) : null}
				{treeError ? <ActionErrorNote error={treeError} className="border-b py-2" /> : null}
				<ScrollArea className="min-h-0 flex-1 p-2">
					{rootsQuery.isLoading ? (
						<AgentOsEmpty>Loading {root}…</AgentOsEmpty>
					) : rootsQuery.error ? (
						<ActionErrorNote error={rootsQuery.error} />
					) : notADir ? (
						<AgentOsEmpty>Not a directory, or does not exist: {root}</AgentOsEmpty>
					) : roots.length === 0 ? (
						<AgentOsEmpty>
							<div className="flex flex-col items-center gap-2">
								<FileGlyph dir className="size-8 text-muted-foreground/40" />
								<span>Empty directory.</span>
							</div>
						</AgentOsEmpty>
					) : (
						roots.map((entry) => (
							<FileTreeItem
								key={entry.path}
								actorId={actorId}
								entry={entry}
								depth={0}
								selectedPath={selectedPath}
								onSelect={(e) => {
									if (e.dir) {
										setDraft(e.path);
									} else {
										setSelectedPath(e.path);
										setDraft(parentDir(e.path));
									}
								}}
							/>
						))
					)}
				</ScrollArea>
			</div>
			<div className="relative min-h-0 min-w-0 flex-1">
				{/* VM trouble chips float over the viewer, below its header row so
				    they never cover the file action buttons; the dropdown has the
				    full pane width to open into (it clipped inside the sidebar). */}
				<div className="absolute right-3 top-11 z-10">
					<VmStatusBadges actorId={actorId} />
				</div>
				<FileViewer
					actorId={actorId}
					path={selectedPath}
					onMutated={() => void refreshTree()}
					onDeleted={() => setSelectedPath(null)}
					onRenamed={(to) => setSelectedPath(to)}
				/>
			</div>
		</div>
	);
}
