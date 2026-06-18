const std = @import("std");
const Store = @import("store.zig").Store;
const AgentRegistry = @import("agent.zig").AgentRegistry;
const AgentId = @import("agent.zig").AgentId;
const Explorer = @import("explore.zig").Explorer;
const Op = @import("version.zig").Op;

pub const EditRequest = struct {
    path: []const u8,
    agent_id: AgentId,
    op: Op,
    range: ?[2]usize = null,
    after: ?usize = null,
    content: ?[]const u8 = null,
};

pub const EditResult = struct {
    seq: u64,
    new_hash: u64,
    new_size: u64,
};

pub fn applyEdit(
    allocator: std.mem.Allocator,
    store: *Store,
    agents: *AgentRegistry,
    explorer: ?*Explorer,
    req: EditRequest,
) !EditResult {
    const has_lock = try agents.tryLock(req.agent_id, req.path, 30_000);
    if (!has_lock) return error.FileLocked;
    errdefer agents.releaseLock(req.agent_id, req.path);

    const file = try std.fs.cwd().openFile(req.path, .{});
    defer file.close();
    const source = try file.readToEndAlloc(allocator, 10 * 1024 * 1024);
    defer allocator.free(source);

    var lines: std.ArrayList([]const u8) = .{};
    defer lines.deinit(allocator);
    var iter = std.mem.splitScalar(u8, source, '\n');
    while (iter.next()) |line| try lines.append(allocator, line);

    // A trailing newline produces an empty final element; don't count it as a line
    const had_trailing_newline = lines.items.len > 0 and lines.items[lines.items.len - 1].len == 0;
    if (had_trailing_newline) {
        _ = lines.pop();
    }

    switch (req.op) {
        .replace => {
            if (req.range) |range| {
                if (range[0] == 0 or range[1] < range[0] or range[0] > lines.items.len) return error.InvalidRange;
                const start = range[0] - 1;
                const end = @min(range[1], lines.items.len);
                const new_content = req.content orelse return error.MissingContent;
                var new_lines: std.ArrayList([]const u8) = .{};
                defer new_lines.deinit(allocator);
                var ni = std.mem.splitScalar(u8, new_content, '\n');
                while (ni.next()) |nl| try new_lines.append(allocator, nl);
                try lines.replaceRange(allocator, start, end - start, new_lines.items);
            }
        },
        .insert => {
            if (req.after) |after_line| {
                const pos = @min(after_line, lines.items.len);
                const content = req.content orelse return error.MissingContent;
                try lines.insert(allocator, pos, content);
            }
        },
        .delete => {
            if (req.range) |range| {
                if (range[0] == 0 or range[1] < range[0] or range[0] > lines.items.len) return error.InvalidRange;
                const start = range[0] - 1;
                const end = @min(range[1], lines.items.len);
                // Remove lines [start..end) by replacing with nothing
                try lines.replaceRange(allocator, start, end - start, &.{});
            }
        },
        else => {},
    }

    // Restore trailing newline if the original file had one
    if (had_trailing_newline) {
        try lines.append(allocator, "");
    }

    const result = try std.mem.join(allocator, "\n", lines.items);
    defer allocator.free(result);

    // Atomic write: write to temp file then rename to prevent corruption on crash
    const dir = std.fs.cwd();
    const tmp_path = try std.fmt.allocPrint(allocator, "{s}.codedb_tmp", .{req.path});
    defer allocator.free(tmp_path);

    {
        const tmp_file = try dir.createFile(tmp_path, .{});
        defer tmp_file.close();
        try tmp_file.writeAll(result);
    }

    dir.rename(tmp_path, req.path) catch |err| {
        // Clean up temp file on rename failure
        dir.deleteFile(tmp_path) catch {};
        return err;
    };

    const hash: u64 = std.hash.Wyhash.hash(0, result);
    // KNOWN LIMITATION: if recordEdit fails here, the file is already on disk but not
    // in the store. This leaves the disk and store inconsistent. Recovery would require
    // re-reading the file and re-recording, or a crash-recovery scan at startup.
    const seq = try store.recordEdit(req.path, req.agent_id, req.op, hash, result.len, req.content);
    if (explorer) |exp| {
        try exp.indexFile(req.path, result);
    }

    agents.releaseLock(req.agent_id, req.path);

    return .{
        .seq = seq,
        .new_hash = hash,
        .new_size = result.len,
    };
}
