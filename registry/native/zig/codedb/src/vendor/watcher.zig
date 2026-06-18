const std = @import("std");
const Store = @import("store.zig").Store;
const Explorer = @import("explore.zig").Explorer;

pub const EventKind = enum(u8) {
    created,
    modified,
    deleted,
};

pub const FsEvent = struct {
    path_buf: [std.fs.max_path_bytes]u8 = undefined,
    path_len: usize,
    kind: EventKind,
    seq: u64,

    pub fn init(src_path: []const u8, kind: EventKind, seq: u64) ?FsEvent {
        // Gracefully skip paths exceeding the max instead of panicking.
        if (src_path.len > std.fs.max_path_bytes) return null;
        var event = FsEvent{
            .path_len = src_path.len,
            .kind = kind,
            .seq = seq,
        };
        @memcpy(event.path_buf[0..src_path.len], src_path);
        return event;
    }

    pub fn path(self: *const FsEvent) []const u8 {
        return self.path_buf[0..self.path_len];
    }
};

pub const EventQueue = struct {
    const CAPACITY = 4096;

    events: [CAPACITY]?FsEvent = [_]?FsEvent{null} ** CAPACITY,
    head: std.atomic.Value(usize) = std.atomic.Value(usize).init(0),
    tail: std.atomic.Value(usize) = std.atomic.Value(usize).init(0),
    mu: std.Thread.Mutex = .{},

    pub fn push(self: *EventQueue, event: FsEvent) bool {
        self.mu.lock();
        defer self.mu.unlock();

        // Mutex provides the memory ordering guarantee; .monotonic is sufficient here.
        const cur_tail = self.tail.load(.monotonic);
        const next_tail = (cur_tail + 1) % CAPACITY;
        if (next_tail == self.head.load(.monotonic)) return false;
        self.events[cur_tail] = event;
        self.tail.store(next_tail, .monotonic);
        return true;
    }

    pub fn pop(self: *EventQueue) ?FsEvent {
        self.mu.lock();
        defer self.mu.unlock();

        // Mutex provides the memory ordering guarantee; .monotonic is sufficient here.
        const cur_head = self.head.load(.monotonic);
        if (cur_head == self.tail.load(.monotonic)) return null;
        const event = self.events[cur_head];
        self.head.store((cur_head + 1) % CAPACITY, .monotonic);
        return event;
    }
};

const FileState = struct {
    mtime: i64,   // milliseconds since epoch — cheap stat check
    size: u64,    // cheap change discriminator before hashing
    hash: u64,    // wyhash of content — confirms actual change
    seen: bool,   // set during current poll cycle for deletion detection
};

const FileMap = std.StringHashMap(FileState);

const skip_dirs = [_][]const u8{
    ".git",
    ".claude",
    ".codedb",
    "node_modules",
    ".zig-cache",
    "zig-out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "dist",
    "build",
    ".build",
    ".output",
    "out",
    "__pycache__",
    ".venv",
    "venv",
    ".env",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "target",          // rust, java/maven
    ".gradle",
    ".idea",
    ".vs",
    "vendor",          // go, php
    "Pods",            // cocoapods
    ".dart_tool",
    ".pub-cache",
    "coverage",
    ".nyc_output",
    ".turbo",
    ".parcel-cache",
    ".cache",
    ".tmp",
    ".temp",
    ".DS_Store",
};

fn shouldSkip(path: []const u8) bool {
    // Check each path component against skip list
    var rest = path;
    while (true) {
        for (skip_dirs) |skip| {
            if (rest.len >= skip.len and
                std.mem.eql(u8, rest[0..skip.len], skip) and
                (rest.len == skip.len or rest[skip.len] == '/'))
                return true;
        }
        // Advance to next component
        if (std.mem.indexOfScalar(u8, rest, '/')) |sep| {
            rest = rest[sep + 1 ..];
        } else break;
    }
    return false;
}

fn shouldSkipDir(name: []const u8) bool {
    for (skip_dirs) |skip| {
        if (std.mem.eql(u8, name, skip)) return true;
    }
    return false;
}

/// Recursive directory walker that prunes skip_dirs before descending.
/// Unlike std.fs.Dir.walk(), this never enters .git, node_modules, etc.,
/// avoiding the CPU cost of traversing potentially huge directory trees.
const FilteredWalker = struct {
    const StackItem = struct {
        dir_handle: std.fs.Dir,
        iter: std.fs.Dir.Iterator,
    };

    stack: std.ArrayList(StackItem),
    name_buffer: std.ArrayList(u8),
    allocator: std.mem.Allocator,
    dir_prefix_len: usize = 0,

    pub const Entry = struct {
        path: []const u8, // relative path — valid until next call to next()
    };

    pub fn init(root: std.fs.Dir, allocator: std.mem.Allocator) !FilteredWalker {
        var self = FilteredWalker{
            .stack = .{},
            .name_buffer = .{},
            .allocator = allocator,
        };
        try self.stack.append(allocator, .{
            .dir_handle = root,
            .iter = root.iterate(),
        });
        return self;
    }

    pub fn deinit(self: *FilteredWalker) void {
        for (self.stack.items, 0..) |*item, i| {
            if (i > 0) item.dir_handle.close();
        }
        self.stack.deinit(self.allocator);
        self.name_buffer.deinit(self.allocator);
    }

    pub fn next(self: *FilteredWalker) !?Entry {
        // Trim any filename appended by the previous yield
        self.name_buffer.shrinkRetainingCapacity(self.dir_prefix_len);

        while (self.stack.items.len > 0) {
            const top = &self.stack.items[self.stack.items.len - 1];
            if (try top.iter.next()) |entry| {
                if (entry.kind == .directory) {
                    if (shouldSkipDir(entry.name)) continue;

                    const sub = top.dir_handle.openDir(entry.name, .{ .iterate = true }) catch continue;

                    // Extend the directory prefix in name_buffer
                    if (self.name_buffer.items.len > 0)
                        try self.name_buffer.append(self.allocator, '/');
                    try self.name_buffer.appendSlice(self.allocator, entry.name);
                    self.dir_prefix_len = self.name_buffer.items.len;

                    try self.stack.append(self.allocator, .{
                        .dir_handle = sub,
                        .iter = sub.iterate(),
                    });
                    continue;
                }

                if (entry.kind != .file) continue;

                // Build full relative path by appending filename
                if (self.dir_prefix_len > 0)
                    try self.name_buffer.append(self.allocator, '/');
                try self.name_buffer.appendSlice(self.allocator, entry.name);

                return .{ .path = self.name_buffer.items };
            } else {
                // Directory exhausted — pop and restore parent prefix
                if (self.stack.items.len > 1) {
                    var item = self.stack.pop().?;
                    item.dir_handle.close();
                } else {
                    _ = self.stack.pop();
                }
                if (std.mem.lastIndexOfScalar(u8, self.name_buffer.items[0..self.dir_prefix_len], '/')) |pos| {
                    self.dir_prefix_len = pos;
                } else {
                    self.dir_prefix_len = 0;
                }
                self.name_buffer.shrinkRetainingCapacity(self.dir_prefix_len);
            }
        }
        return null;
    }
};

/// Called from main thread to do the initial scan before listening.
pub fn initialScan(store: *Store, explorer: *Explorer, root: []const u8, allocator: std.mem.Allocator, skip_trigram: bool) !void {
    var dir = try std.fs.cwd().openDir(root, .{ .iterate = true });
    defer dir.close();

    var walker = try FilteredWalker.init(dir, allocator);
    defer walker.deinit();

    while (try walker.next()) |entry| {
        const stat = dir.statFile(entry.path) catch continue;
        _ = try store.recordSnapshot(entry.path, stat.size, 0);
        // Index outline + content + word/trigram for full search support
        indexFileContent(explorer, dir, entry.path, allocator, skip_trigram) catch {};
    }
}

/// Fast index: parse symbols/outline only, skip expensive word+trigram indexes.
fn indexFileOutline(explorer: *Explorer, dir: std.fs.Dir, path: []const u8, allocator: std.mem.Allocator) !void {
    if (shouldSkipFile(path)) return;
    const file = try dir.openFile(path, .{});
    defer file.close();
    const stat = try file.stat();
    if (stat.size > 512 * 1024) return;
    const content = try file.readToEndAlloc(allocator, 512 * 1024);
    defer allocator.free(content);
    const check_len = @min(content.len, 512);
    for (content[0..check_len]) |c| {
        if (c == 0) return;
    }
    try explorer.indexFileOutlineOnly(path, content);
}

/// Background thread: polls for incremental FS changes.
pub fn incrementalLoop(store: *Store, explorer: *Explorer, queue: *EventQueue, root: []const u8, shutdown: *std.atomic.Value(bool), scan_done: *std.atomic.Value(bool)) void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const backing = gpa.allocator();

    // Wait for initial scan to finish before building our known-file snapshot.
    // This prevents double-indexing: initialScan does full indexing, then we
    // only pick up changes that happen after.
    while (!scan_done.load(.acquire)) {
        if (shutdown.load(.acquire)) return;
        std.Thread.sleep(100 * std.time.ns_per_ms);
    }

    var known = FileMap.init(backing);
    defer {
        var iter = known.iterator();
        while (iter.next()) |kv| {
            backing.free(kv.key_ptr.*);
        }
        known.deinit();
    }
    // Build initial snapshot: stat every file, defer expensive hashing until mtime changes.
    {
        var snap_arena = std.heap.ArenaAllocator.init(backing);
        defer snap_arena.deinit();
        const tmp = snap_arena.allocator();
        var dir = std.fs.cwd().openDir(root, .{ .iterate = true }) catch return;
        defer dir.close();
        var walker = FilteredWalker.init(dir, tmp) catch return;
        defer walker.deinit();
        while (walker.next() catch null) |entry| {
            const stat = dir.statFile(entry.path) catch continue;
            const mtime: i64 = @intCast(@divTrunc(stat.mtime, std.time.ns_per_ms));
            const duped = backing.dupe(u8, entry.path) catch continue;
            known.put(duped, .{ .mtime = mtime, .size = stat.size, .hash = 0, .seen = false }) catch backing.free(duped);
        }
    }

    while (!shutdown.load(.acquire)) {
        // Check for muonry edit notifications (instant re-index, no 2s delay)
        drainNotifyFile(store, explorer, queue, &known, root, backing);

        // Poll every 2s — gentle on CPU, fast enough to catch saves
        std.Thread.sleep(2 * std.time.ns_per_s);

        // Each diff cycle gets its own arena so temporaries are freed
        var cycle_arena = std.heap.ArenaAllocator.init(backing);
        defer cycle_arena.deinit();

        incrementalDiff(store, explorer, queue, &known, root, backing, cycle_arena.allocator()) catch |err| {
            std.log.err("watcher: diff failed: {}", .{err});
        };
    }
}

fn hashFile(dir: std.fs.Dir, path: []const u8, size: u64) !u64 {
    // Returns 0 for intentional skip (large files, filtered extensions).
    // Returns maxInt(u64) on IO error so the value always differs from a valid
    // previously stored hash of 0, preventing a false "content unchanged" conclusion.
    if (shouldSkipFile(path)) return 0;
    const file = dir.openFile(path, .{}) catch return std.math.maxInt(u64);
    defer file.close();
    if (size > 512 * 1024) return 0;

    var hasher = std.hash.Wyhash.init(0);
    var buf: [16 * 1024]u8 = undefined;
    while (true) {
        const n = file.read(&buf) catch return std.math.maxInt(u64);
        if (n == 0) break;
        hasher.update(buf[0..n]);
    }
    return hasher.final();
}

fn pushEventOrWait(queue: *EventQueue, event: FsEvent) void {
    // Preserve prior drop-on-full behavior so producer never stalls permanently.
    _ = queue.push(event);
}


fn incrementalDiff(store: *Store, explorer: *Explorer, queue: *EventQueue, known: *FileMap, root: []const u8, persistent: std.mem.Allocator, tmp: std.mem.Allocator) !void {
    var dir = try std.fs.cwd().openDir(root, .{ .iterate = true });
    defer dir.close();

    // Mark all known files unseen for this cycle.
    var known_iter = known.iterator();
    while (known_iter.next()) |kv| {
        kv.value_ptr.seen = false;
    }

    var walker = try FilteredWalker.init(dir, tmp);
    defer walker.deinit();

    while (try walker.next()) |entry| {
        const stat = dir.statFile(entry.path) catch continue;
        const mtime: i64 = @intCast(@divTrunc(stat.mtime, std.time.ns_per_ms));

        if (known.getEntry(entry.path)) |known_entry| {
            const old = known_entry.value_ptr;
            old.seen = true;

            // Mtime unchanged -> skip (cheap path, no IO)
            if (old.mtime == mtime) continue;

            // Size changed -> definitely changed, skip expensive hash.
            var hash: u64 = 0;
            if (old.size == stat.size) {
                // Same size + changed mtime -> hash to confirm content actually differs.
                hash = hashFile(dir, entry.path, stat.size) catch 0;
            }
            if (old.size == stat.size and hash != 0 and old.hash != 0 and hash == old.hash) {
                // Content identical (e.g. touch, git checkout) -> update metadata only.
                old.mtime = mtime;
                old.size = stat.size;
                continue;
            }

            const seq = try store.recordSnapshot(entry.path, stat.size, hash);
            old.mtime = mtime;
            old.size = stat.size;
            old.hash = hash;
            const stable_path = known_entry.key_ptr.*;
            if (FsEvent.init(stable_path, .modified, seq)) |ev| pushEventOrWait(queue, ev);
            indexFileContent(explorer, dir, stable_path, tmp, false) catch {};
        } else {
            // New files always generate an event, so skip the extra full-file hash pass.
            const duped = try persistent.dupe(u8, entry.path);
            errdefer persistent.free(duped);
            const seq = try store.recordSnapshot(duped, stat.size, 0);
            try known.put(duped, .{ .mtime = mtime, .size = stat.size, .hash = 0, .seen = true });
            if (FsEvent.init(duped, .created, seq)) |ev| pushEventOrWait(queue, ev);
            indexFileContent(explorer, dir, duped, tmp, false) catch {};
        }
    }

    // Detect deleted files
    var to_remove: std.ArrayList([]const u8) = .{};
    defer to_remove.deinit(tmp);

    var iter = known.iterator();
    while (iter.next()) |kv| {
        if (!kv.value_ptr.seen) {
            try to_remove.append(tmp, kv.key_ptr.*);
        }
    }
    for (to_remove.items) |path| {
        const seq = store.recordDelete(path, 0) catch continue;
        explorer.removeFile(path);
        if (known.fetchRemove(path)) |kv| {
            if (FsEvent.init(kv.key, .deleted, seq)) |ev| pushEventOrWait(queue, ev);
            persistent.free(kv.key);
        }
    }
}

const skip_extensions = [_][]const u8{
    ".png",  ".jpg",  ".jpeg", ".gif",  ".bmp",  ".ico",  ".icns", ".webp",
    ".svg",  ".ttf",  ".otf",  ".woff", ".woff2", ".eot",
    ".zip",  ".tar",  ".gz",   ".bz2",  ".xz",   ".7z",  ".rar",
    ".pdf",  ".doc",  ".docx", ".xls",  ".xlsx", ".pptx",
    ".mp3",  ".mp4",  ".wav",  ".avi",  ".mov",  ".flv",  ".ogg",  ".webm",
    ".exe",  ".dll",  ".so",   ".dylib", ".o",   ".a",    ".lib",
    ".wasm", ".pyc",  ".pyo",  ".class",
    ".db",   ".sqlite", ".sqlite3",
    ".lock", ".sum",
};

fn shouldSkipFile(path: []const u8) bool {
    for (skip_extensions) |ext| {
        if (std.mem.endsWith(u8, path, ext)) return true;
    }
    // Skip dotfiles like .DS_Store, .gitignore etc at any depth
    if (std.mem.endsWith(u8, path, ".DS_Store")) return true;
    // Skip sensitive files (.env, credentials, keys) — same rules as snapshot filtering
    if (isSensitivePath(path)) return true;
    return false;
}

/// Check if a path refers to a sensitive file (secrets, keys, credentials).
/// Replicates the filter from snapshot.zig so live indexing and snapshots
/// apply the same exclusion rules. Optimized: basename check + early exit.
fn isSensitivePath(path: []const u8) bool {
    const basename = if (std.mem.lastIndexOfScalar(u8, path, '/')) |sep| path[sep + 1 ..] else path;
    // Fast path: most source files have extensions like .zig, .ts, .py — none start with '.'
    // or match sensitive patterns. Skip the full check for common cases.
    if (basename.len == 0) return false;
    const first = basename[0];
    // Only check sensitive names if basename starts with '.', 'c', 's', 'i' or has key/cert extension
    if (first != '.' and first != 'c' and first != 's' and first != 'i') {
        // Still need to check extensions and directory patterns
        if (std.mem.endsWith(u8, basename, ".pem") or
            std.mem.endsWith(u8, basename, ".key") or
            std.mem.endsWith(u8, basename, ".p12") or
            std.mem.endsWith(u8, basename, ".pfx") or
            std.mem.endsWith(u8, basename, ".jks")) return true;
        if (std.mem.indexOf(u8, path, ".ssh/") != null or
            std.mem.indexOf(u8, path, ".gnupg/") != null or
            std.mem.indexOf(u8, path, ".aws/") != null) return true;
        return false;
    }
    // .env* wildcard (catches .env, .env.local, .env.production, etc.)
    if (basename.len >= 4 and std.mem.eql(u8, basename[0..4], ".env")) return true;
    // Exact matches
    const sensitive_names = [_][]const u8{
        ".dev.vars", ".npmrc", ".pypirc", ".netrc",
        "credentials.json", "service-account.json",
        "secrets.json", "secrets.yaml", "secrets.yml",
        "id_rsa", "id_ed25519",
    };
    for (sensitive_names) |name| {
        if (std.mem.eql(u8, basename, name)) return true;
    }
    if (std.mem.endsWith(u8, basename, ".pem") or
        std.mem.endsWith(u8, basename, ".key") or
        std.mem.endsWith(u8, basename, ".p12")) return true;
    if (std.mem.indexOf(u8, path, ".ssh/") != null or
        std.mem.indexOf(u8, path, ".gnupg/") != null or
        std.mem.indexOf(u8, path, ".aws/") != null) return true;
    return false;
}

fn indexFileContent(explorer: *Explorer, dir: std.fs.Dir, path: []const u8, allocator: std.mem.Allocator, skip_trigram: bool) !void {
    if (shouldSkipFile(path)) return;
    const file = try dir.openFile(path, .{});
    defer file.close();
    const stat = try file.stat();
    // Skip files over 512KB (likely minified bundles or generated)
    if (stat.size > 512 * 1024) return;
    const content = try file.readToEndAlloc(allocator, 512 * 1024);
    defer allocator.free(content);
    // Skip binary content (check first 512 bytes for null bytes)
    const check_len = @min(content.len, 512);
    for (content[0..check_len]) |c| {
        if (c == 0) return;
    }
    if (skip_trigram) {
        try explorer.indexFileSkipTrigram(path, content);
    } else {
        try explorer.indexFile(path, content);
    }
}

// ── muonry interop ───────────────────────────────────────────────────────────
//
// muonry appends changed file paths to /tmp/codedb-notify after each edit.
// We drain this file on every poll cycle and re-index the listed files
// immediately, eliminating the 2s polling delay for muonry-sourced edits.

fn drainNotifyFile(store: *Store, explorer: *Explorer, queue: *EventQueue, known: *FileMap, root: []const u8, alloc: std.mem.Allocator) void {
    // Atomically read + truncate
    const notify_path = "/tmp/codedb-notify";
    const file = std.fs.cwd().openFile(notify_path, .{ .mode = .read_write }) catch return;
    defer file.close();

    const data = file.readToEndAlloc(alloc, 64 * 1024) catch return;
    defer alloc.free(data);
    if (data.len == 0) return;

    // Truncate after reading
    file.seekTo(0) catch return;
    std.posix.ftruncate(file.handle, 0) catch return;

    // Re-index each notified path
    var dir = std.fs.cwd().openDir(root, .{}) catch return;
    defer dir.close();

    var lines = std.mem.splitScalar(u8, data, '\n');
    while (lines.next()) |line| {
        const path = std.mem.trim(u8, line, " \t\r");
        if (path.len == 0) continue;

        // Make path relative to root if it's absolute
        const rel = if (std.mem.startsWith(u8, path, root))
            std.mem.trimLeft(u8, path[root.len..], "/")
        else
            path;

        indexFileContent(explorer, dir, rel, alloc, false) catch continue;

        // Update known-file state so incrementalDiff doesn't double-process
        const stat = dir.statFile(rel) catch continue;
        const mtime: i64 = @intCast(@divTrunc(stat.mtime, std.time.ns_per_ms));
        const hash = hashFile(dir, rel, stat.size) catch continue;
        if (known.getPtr(rel)) |existing| {
            existing.mtime = mtime;
            existing.size = stat.size;
            existing.hash = hash;
        }

        // Push event to queue
        if (FsEvent.init(rel, .modified, store.currentSeq())) |ev| {
            _ = queue.push(ev);
        }
    }
}
