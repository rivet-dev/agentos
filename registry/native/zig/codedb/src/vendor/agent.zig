const std = @import("std");

/// An Agent is a first-class citizen — not a string column, but a live entity
/// with its own identity, write log, cursor position, and capabilities.
pub const AgentId = u64;

pub const AgentState = enum(u8) {
    active = 0,
    idle = 1,
    crashed = 2, // missed heartbeat
    gone = 3, // explicitly disconnected
};

pub const Agent = struct {
    id: AgentId,
    name: []const u8,
    state: AgentState,
    /// The version number this agent has seen up to.
    /// Anything after this cursor is "new" to this agent.
    cursor: u64,
    /// Timestamp of last heartbeat (ms since epoch).
    last_seen: i64,
    /// How many edits this agent has made total.
    edit_count: u64,
    /// Which files this agent currently holds advisory locks on.
    locked_paths: std.StringHashMap(i64), // path → expiry timestamp
};

/// Registry of all known agents. The source of truth for "who's alive".
pub const AgentRegistry = struct {
    agents: std.AutoHashMap(AgentId, Agent),
    next_id: AgentId,
    allocator: std.mem.Allocator,
    mu: std.Thread.Mutex = .{},

    pub fn init(allocator: std.mem.Allocator) AgentRegistry {
        return .{
            .agents = std.AutoHashMap(AgentId, Agent).init(allocator),
            .next_id = 1,
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *AgentRegistry) void {
        var iter = self.agents.iterator();
        while (iter.next()) |entry| {
            var lp_iter = entry.value_ptr.locked_paths.keyIterator();
            while (lp_iter.next()) |key| {
                self.allocator.free(key.*);
            }
            self.allocator.free(entry.value_ptr.name);
            entry.value_ptr.locked_paths.deinit();
        }
        self.agents.deinit();
    }

    /// Register a new agent. Returns its unique ID.
    pub fn register(self: *AgentRegistry, name: []const u8) !AgentId {
        self.mu.lock();
        defer self.mu.unlock();

        const id = self.next_id;
        self.next_id += 1;

        const duped_name = try self.allocator.dupe(u8, name);
        errdefer self.allocator.free(duped_name);

        try self.agents.put(id, .{
            .id = id,
            .name = duped_name,
            .state = .active,
            .cursor = 0,
            .last_seen = std.time.milliTimestamp(),
            .edit_count = 0,
            .locked_paths = std.StringHashMap(i64).init(self.allocator),
        });

        return id;
    }

    /// Agent heartbeat — proves it's still alive.
    pub fn heartbeat(self: *AgentRegistry, id: AgentId) void {
        self.mu.lock();
        defer self.mu.unlock();

        if (self.agents.getPtr(id)) |a| {
            a.last_seen = std.time.milliTimestamp();
            if (a.state == .crashed) a.state = .active;
        }
    }

    /// Mark agents as crashed if they haven't heartbeated in `timeout_ms`.
    pub fn reapStale(self: *AgentRegistry, timeout_ms: i64) void {
        self.mu.lock();
        defer self.mu.unlock();

        const now = std.time.milliTimestamp();
        var iter = self.agents.iterator();
        while (iter.next()) |entry| {
            const a = entry.value_ptr;
            if (a.state == .active and (now - a.last_seen) > timeout_ms) {
                a.state = .crashed;
                // Release all locks held by crashed agent.
                var key_iter = a.locked_paths.keyIterator();
                while (key_iter.next()) |key| {
                    self.allocator.free(key.*);
                }
                a.locked_paths.clearAndFree();
            }
        }
    }

    /// Try to acquire an advisory lock on a file path for an agent.
    pub fn tryLock(self: *AgentRegistry, agent_id: AgentId, path: []const u8, ttl_ms: i64) !bool {
        self.mu.lock();
        defer self.mu.unlock();

        const now = std.time.milliTimestamp();

        // Check if any other active agent holds this lock.
        var iter = self.agents.iterator();
        while (iter.next()) |entry| {
            const a = entry.value_ptr;
            if (a.id == agent_id) continue;
            if (a.locked_paths.get(path)) |expiry| {
                if (now < expiry) return false; // someone else holds it
                // Expired: remove it and free the duped key.
                if (a.locked_paths.fetchRemove(path)) |kv| {
                    self.allocator.free(kv.key);
                }
            }
        }

        // Grant the lock.
        if (self.agents.getPtr(agent_id)) |a| {
            if (a.locked_paths.getPtr(path)) |expiry| {
                expiry.* = now + ttl_ms;
                return true;
            }

            const duped = try self.allocator.dupe(u8, path);
            errdefer self.allocator.free(duped);
            try a.locked_paths.put(duped, now + ttl_ms);
            return true;
        }
        return false;
    }
    pub fn releaseLock(self: *AgentRegistry, agent_id: AgentId, path: []const u8) void {
        self.mu.lock();
        defer self.mu.unlock();
        if (self.agents.getPtr(agent_id)) |a| {
            if (a.locked_paths.fetchRemove(path)) |kv| {
                self.allocator.free(kv.key);
            }
        }
    }
};
