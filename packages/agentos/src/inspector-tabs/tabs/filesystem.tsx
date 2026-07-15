import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { ActionErrorNote, AgentOsEmpty, ChevronRight, FileGlyph, formatBytes, relativeTime } from "../common";
import { cn } from "../lib/cn";
import { agentOsSource } from "../lib/source";
import type { FsEntry } from "../lib/types";
import { ScrollArea } from "../ui/scroll-area";
import { VmBootGate } from "../vm-boot-gate";
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
					onClick={() => (expandable ? setOpen((v) => !v) : entry.dir ? undefined : onSelect(entry))}
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
	const { data, error, isFetching } = useQuery(
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

	if (!path) return <AgentOsEmpty>Select a file to view its contents.</AgentOsEmpty>;
	if (error) return <ActionErrorNote error={error} className="items-center justify-center text-center" />;
	if (!data || isFetching) return <AgentOsEmpty>Loading {path}…</AgentOsEmpty>;

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
				<button
					type="button"
					disabled={!data.bytes}
					onClick={() => data.bytes && downloadBytes(data.bytes, filename)}
					title={data.bytes ? "Download this file" : "Load the file first"}
					className="shrink-0 rounded border px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
				>
					Download
				</button>
				<button
					type="button"
					onClick={() => (renameDraft !== null ? void rename() : setRenameDraft(data.path))}
					className="shrink-0 rounded border px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
				>
					{renameDraft !== null ? "Save" : "Rename"}
				</button>
				<button
					type="button"
					onClick={() => void remove()}
					className="shrink-0 rounded border border-destructive/50 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/10"
				>
					{confirmingDelete ? "Confirm delete?" : "Delete"}
				</button>
			</div>
			{mutationError ? <ActionErrorNote error={mutationError} className="border-b py-2" /> : null}
			<ScrollArea className="min-h-0 flex-1">
				{data.oversize ? (
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
			note="VM not booted — the root filesystem is in-memory, so there are no files until it boots."
			actionLabel="Boot the VM and browse files"
		>
			<FilesystemLoaded actorId={actorId} />
		</VmBootGate>
	);
}

function FilesystemLoaded({ actorId }: { actorId: string }) {
	// `root` drives the listing/refetch; `draft` tracks keystrokes locally so
	// typing never refetches. `root` is committed 500ms after typing stops (or
	// immediately on Enter) so we don't refetch on every keystroke.
	const [root, setRoot] = useState("/");
	const [draft, setDraft] = useState("/");
	const [selectedPath, setSelectedPath] = useState<string | null>(null);
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

	const createFolder = async () => {
		const name = newFolderDraft?.trim();
		if (!name) {
			setNewFolderDraft(null);
			return;
		}
		setTreeError(null);
		try {
			await agentOsSource.mkdir(name.startsWith("/") ? name : joinRoot(root, name));
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
			await agentOsSource.writeFile(joinRoot(root, file.name), bytes);
			await refreshTree();
		} catch (error) {
			setTreeError(error);
		}
	};

	return (
		<div className="flex h-full min-h-0">
			<div className="flex h-full w-2/5 flex-col border-r">
				<div className="flex items-center gap-1 border-b px-3 py-1.5">
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
						className="min-w-0 flex-1 bg-transparent font-mono text-xs text-muted-foreground outline-none placeholder:text-muted-foreground/40 focus:text-foreground"
					/>
					<button
						type="button"
						onClick={() => void refreshTree()}
						title="Refresh the tree"
						className="shrink-0 rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
					>
						Refresh
					</button>
					<button
						type="button"
						onClick={() => setNewFolderDraft((v) => (v === null ? "" : null))}
						title={`Create a folder under ${root}`}
						className="shrink-0 rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
					>
						New folder
					</button>
					<button
						type="button"
						onClick={() => uploadInputRef.current?.click()}
						title={`Upload a file into ${root}`}
						className="shrink-0 rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
					>
						Upload
					</button>
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
							placeholder={`folder name (created under ${root})`}
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
							<span>
								Empty directory.
								<br />
								<span className="text-xs text-muted-foreground/70">
									The VM root filesystem is in-memory — files from previous VM boots do not
									survive restarts.
								</span>
							</span>
						</AgentOsEmpty>
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
