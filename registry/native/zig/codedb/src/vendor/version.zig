const std = @import("std");
const AgentId = @import("agent.zig").AgentId;

pub const Version = struct {
    seq: u64,
    agent: AgentId,
    timestamp: i64,
    op: Op,
    hash: u64,
    size: u64,
    data_offset: ?u64 = null,
    data_len: u32 = 0,
};

pub const Op = enum(u8) {
    snapshot = 0,
    replace = 1,
    insert = 2,
    delete = 3,
    tombstone = 4,
};

pub const FileVersions = struct {
    path: []const u8,
    versions: std.ArrayList(Version) = .{},
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, path: []const u8) FileVersions {
        return .{
            .path = path,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *FileVersions) void {
        self.versions.deinit(self.allocator);
    }

    pub fn append(self: *FileVersions, seq: u64, agent: AgentId, op: Op, hash: u64, size: u64) !u64 {
        try self.versions.append(self.allocator, .{
            .seq = seq,
            .agent = agent,
            .timestamp = std.time.milliTimestamp(),
            .op = op,
            .hash = hash,
            .size = size,
        });
        return seq;
    }

    pub fn latest(self: *const FileVersions) ?Version {
        if (self.versions.items.len == 0) return null;
        return self.versions.items[self.versions.items.len - 1];
    }

    pub fn atCursor(self: *const FileVersions, cursor: u64) ?Version {
        var result: ?Version = null;
        for (self.versions.items) |v| {
            if (v.seq <= cursor) result = v;
        }
        return result;
    }

    pub fn countSince(self: *const FileVersions, since_seq: u64) usize {
        var count: usize = 0;
        for (self.versions.items) |v| {
            if (v.seq > since_seq) count += 1;
        }
        return count;
    }
};
