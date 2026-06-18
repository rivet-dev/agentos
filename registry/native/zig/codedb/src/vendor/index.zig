const std = @import("std");

// ── Inverted word index ─────────────────────────────────────
// Maps word → list of (path, line) hits. O(1) word lookup.

pub const WordHit = struct {
    path: []const u8,
    line_num: u32,
};

pub const WordIndex = struct {
    /// word → hits
    index: std.StringHashMap(std.ArrayList(WordHit)),
    /// path → set of words contributed (for efficient re-index cleanup)
    file_words: std.StringHashMap(std.StringHashMap(void)),
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) WordIndex {
        return .{
            .index = std.StringHashMap(std.ArrayList(WordHit)).init(allocator),
            .file_words = std.StringHashMap(std.StringHashMap(void)).init(allocator),
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *WordIndex) void {
        // Free hit lists and duped word keys
        var iter = self.index.iterator();
        while (iter.next()) |entry| {
            self.allocator.free(entry.key_ptr.*);
            entry.value_ptr.deinit(self.allocator);
        }
        self.index.deinit();

        // Free per-file word sets
        var fw_iter = self.file_words.iterator();
        while (fw_iter.next()) |entry| {
            entry.value_ptr.deinit();
        }
        self.file_words.deinit();
    }

    /// Remove all index entries for a file (call before re-indexing).
    pub fn removeFile(self: *WordIndex, path: []const u8) void {
        const words_set = self.file_words.getPtr(path) orelse return;

        // For each word this file contributed, remove hits with this path.
        // Prune empty buckets so churn does not leak key/list entries.
        var word_iter = words_set.keyIterator();
        while (word_iter.next()) |word_ptr| {
            if (self.index.getEntry(word_ptr.*)) |entry| {
                const hits = entry.value_ptr;
                var i: usize = 0;
                while (i < hits.items.len) {
                    if (std.mem.eql(u8, hits.items[i].path, path)) {
                        _ = hits.swapRemove(i);
                    } else {
                        i += 1;
                    }
                }
                if (hits.items.len == 0) {
                    const owned_word = entry.key_ptr.*;
                    hits.deinit(self.allocator);
                    _ = self.index.remove(word_ptr.*);
                    self.allocator.free(owned_word);
                }
            }
        }

        words_set.deinit();
        _ = self.file_words.remove(path);
    }


    /// Index a file's content — tokenizes into words and records hits.
    pub fn indexFile(self: *WordIndex, path: []const u8, content: []const u8) !void {
        // Clean up old entries first
        self.removeFile(path);

        var words_set = std.StringHashMap(void).init(self.allocator);
        errdefer words_set.deinit();
        var line_num: u32 = 0;
        var lines = std.mem.splitScalar(u8, content, '\n');

        while (lines.next()) |line| {
            line_num += 1;
            var tok = WordTokenizer{ .buf = line };
            while (tok.next()) |word| {
                if (word.len < 2) continue; // skip single chars

                // Ensure word is in the global index
                const gop = try self.index.getOrPut(word);
                if (!gop.found_existing) {
                    const duped_word = try self.allocator.dupe(u8, word);
                    gop.key_ptr.* = duped_word;
                    gop.value_ptr.* = .{};
                }

                if (gop.value_ptr.items.len > 0) {
                    const last = gop.value_ptr.items[gop.value_ptr.items.len - 1];
                    if (std.mem.eql(u8, last.path, path) and last.line_num == line_num) {
                        // Avoid duplicate hits for repeated words on the same line.
                        const wgop = try words_set.getOrPut(word);
                        if (!wgop.found_existing) wgop.key_ptr.* = gop.key_ptr.*;
                        continue;
                    }
                }

                try gop.value_ptr.append(self.allocator, .{
                    .path = path,
                    .line_num = line_num,
                });

                // Track that this file contributed this word
                const wgop = try words_set.getOrPut(word);
                if (!wgop.found_existing) {
                    // Point to the same key in the index (no extra alloc)
                    wgop.key_ptr.* = gop.key_ptr.*;
                }
            }
        }

        try self.file_words.put(path, words_set);
    }

    /// Look up all hits for a word. O(1) lookup + O(hits) iteration.
    pub fn search(self: *WordIndex, word: []const u8) []const WordHit {
        if (self.index.get(word)) |hits| {
            return hits.items;
        }
        return &.{};
    }

    /// Look up hits, returning results allocated by the caller.
    /// Deduplicates by (path, line_num).
pub fn searchDeduped(self: *WordIndex, word: []const u8, allocator: std.mem.Allocator) ![]const WordHit {
    const hits = self.search(word);
    if (hits.len == 0) return try allocator.alloc(WordHit, 0);
    if (hits.len == 1) {
        var out = try allocator.alloc(WordHit, 1);
        out[0] = hits[0];
        return out;
    }

    const DedupKey = struct { path_ptr: usize, line_num: u32 };
    var seen = std.AutoHashMap(DedupKey, void).init(allocator);
    defer seen.deinit();
    try seen.ensureTotalCapacity(@intCast(hits.len));

    var result: std.ArrayList(WordHit) = .{};
    errdefer result.deinit(allocator);
    try result.ensureTotalCapacity(allocator, hits.len);

    for (hits) |hit| {
        const key = DedupKey{ .path_ptr = @intFromPtr(hit.path.ptr), .line_num = hit.line_num };
        const gop = try seen.getOrPut(key);
        if (!gop.found_existing) {
            result.appendAssumeCapacity(hit);
        }
    }
    return result.toOwnedSlice(allocator);
}
};

// ── Trigram index ───────────────────────────────────────────
// Maps 3-byte sequences → set of file paths.
// Enables fast substring search: extract trigrams from query,
// intersect candidate file sets, then verify with actual match.

pub const Trigram = u24;

pub fn packTrigram(a: u8, b: u8, c: u8) Trigram {
    return @as(Trigram, a) << 16 | @as(Trigram, b) << 8 | @as(Trigram, c);
}


pub const PostingMask = struct {
    next_mask: u8 = 0, // bloom filter of chars following this trigram
    loc_mask: u8 = 0, // bit mask of (position % 8) where trigram appears
};


pub const TrigramIndex = struct {
    /// trigram → set of file paths
    index: std.AutoHashMap(Trigram, std.StringHashMap(PostingMask)),
    /// path → list of trigrams contributed (for cleanup)
    file_trigrams: std.StringHashMap(std.ArrayList(Trigram)),
    allocator: std.mem.Allocator,
    /// When true, deinit frees the path keys in file_trigrams (set by readFromDisk).
    owns_paths: bool = false,

    pub fn init(allocator: std.mem.Allocator) TrigramIndex {
        return .{
            .index = std.AutoHashMap(Trigram, std.StringHashMap(PostingMask)).init(allocator),
            .file_trigrams = std.StringHashMap(std.ArrayList(Trigram)).init(allocator),
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *TrigramIndex) void {
        var iter = self.index.iterator();
        while (iter.next()) |entry| {
            entry.value_ptr.deinit();
        }
        self.index.deinit();

        var ft_iter = self.file_trigrams.iterator();
        while (ft_iter.next()) |entry| {
            if (self.owns_paths) self.allocator.free(entry.key_ptr.*);
            entry.value_ptr.deinit(self.allocator);
        }
        self.file_trigrams.deinit();
    }

    pub fn removeFile(self: *TrigramIndex, path: []const u8) void {
        const trigrams = self.file_trigrams.getPtr(path) orelse return;
        for (trigrams.items) |tri| {
            if (self.index.getPtr(tri)) |file_set| {
                _ = file_set.remove(path);
                if (file_set.count() == 0) {
                    file_set.deinit();
                    _ = self.index.remove(tri);
                }
            }
        }
        trigrams.deinit(self.allocator);
        _ = self.file_trigrams.remove(path);
    }

    pub fn indexFile(self: *TrigramIndex, path: []const u8, content: []const u8) !void {
        self.removeFile(path);

        var seen_trigrams = std.AutoHashMap(Trigram, void).init(self.allocator);
        defer seen_trigrams.deinit();

        // Extract trigrams from content, recording PostingMask per (trigram, file)
        if (content.len >= 3) {
            for (0..content.len - 2) |i| {
                const tri = packTrigram(
                    normalizeChar(content[i]),
                    normalizeChar(content[i + 1]),
                    normalizeChar(content[i + 2]),
                );
                // Ensure the trigram → file_set entry exists
                const idx_gop = try self.index.getOrPut(tri);
                if (!idx_gop.found_existing) {
                    idx_gop.value_ptr.* = std.StringHashMap(PostingMask).init(self.allocator);
                }
                // Get or create the posting for this file
                const file_gop = try idx_gop.value_ptr.getOrPut(path);
                if (!file_gop.found_existing) {
                    file_gop.value_ptr.* = PostingMask{};
                    // Track this trigram for cleanup (only once per file)
                    try seen_trigrams.put(tri, {});
                }
                // OR in position masks
                file_gop.value_ptr.loc_mask |= @as(u8, 1) << @intCast(i % 8);
                if (i + 3 < content.len) {
                    file_gop.value_ptr.next_mask |= @as(u8, 1) << @intCast(normalizeChar(content[i + 3]) % 8);
                }
            }
        }

        // Store which trigrams this file contributed
        var tri_list: std.ArrayList(Trigram) = .{};
        errdefer tri_list.deinit(self.allocator);
        var tri_iter = seen_trigrams.keyIterator();
        while (tri_iter.next()) |tri_ptr| {
            try tri_list.append(self.allocator, tri_ptr.*);
        }
        try self.file_trigrams.put(path, tri_list);
    }


    /// Find candidate files that contain ALL trigrams from the query.
pub fn candidates(self: *TrigramIndex, query: []const u8, allocator: std.mem.Allocator) ?[]const []const u8 {
    if (query.len < 3) return null; // can't use trigrams for short queries

    const tri_count = query.len - 2;

    // Deduplicate query trigrams first so repeated trigrams don't do repeated work.
    var unique = std.AutoHashMap(Trigram, void).init(allocator);
    defer unique.deinit();
    unique.ensureTotalCapacity(@intCast(tri_count)) catch return null;
    for (0..tri_count) |i| {
        const tri = packTrigram(
            normalizeChar(query[i]),
            normalizeChar(query[i + 1]),
            normalizeChar(query[i + 2]),
        );
        _ = unique.getOrPut(tri) catch return null;
    }

    var sets: std.ArrayList(*std.StringHashMap(PostingMask)) = .{};
    defer sets.deinit(allocator);
    sets.ensureTotalCapacity(allocator, unique.count()) catch return null;

    var tri_iter = unique.keyIterator();
    while (tri_iter.next()) |tri_ptr| {
        const file_set = self.index.getPtr(tri_ptr.*) orelse {
            return allocator.alloc([]const u8, 0) catch null;
        };
        sets.appendAssumeCapacity(file_set);
    }

    if (sets.items.len == 0) {
        return allocator.alloc([]const u8, 0) catch null;
    }

    // Iterate the smallest set and check membership in all others.
    var min_idx: usize = 0;
    var min_count = sets.items[0].count();
    for (sets.items[1..], 1..) |set, i| {
        const count = set.count();
        if (count < min_count) {
            min_count = count;
            min_idx = i;
        }
    }

    var result: std.ArrayList([]const u8) = .{};
    errdefer result.deinit(allocator);
    result.ensureTotalCapacity(allocator, min_count) catch return null;

    var it = sets.items[min_idx].keyIterator();
    next_cand: while (it.next()) |path_ptr| {

        // Intersection check: candidate must be in all sets
        for (sets.items, 0..) |set, i| {
            if (i == min_idx) continue;
            if (!set.contains(path_ptr.*)) continue :next_cand;
        }

        // Bloom-filter check for consecutive trigram pairs
        if (tri_count >= 2) {
            for (0..tri_count - 1) |j| {
                const tri_a = packTrigram(
                    normalizeChar(query[j]),
                    normalizeChar(query[j + 1]),
                    normalizeChar(query[j + 2]),
                );
                const tri_b = packTrigram(
                    normalizeChar(query[j + 1]),
                    normalizeChar(query[j + 2]),
                    normalizeChar(query[j + 3]),
                );
                const set_a = self.index.getPtr(tri_a) orelse continue;
                const set_b = self.index.getPtr(tri_b) orelse continue;
                const mask_a = set_a.get(path_ptr.*) orelse continue;
                const mask_b = set_b.get(path_ptr.*) orelse continue;

                // next_mask: bit for query[j+3] must be set in tri_a's next_mask
                const next_bit: u8 = @as(u8, 1) << @intCast(normalizeChar(query[j + 3]) % 8);
                if ((mask_a.next_mask & next_bit) == 0) continue :next_cand;

                // loc_mask adjacency: use circular shift to handle position wrap-around
                const rotated = (mask_a.loc_mask << 1) | (mask_a.loc_mask >> 7);
                if ((rotated & mask_b.loc_mask) == 0) continue :next_cand;
            }
        }

        result.appendAssumeCapacity(path_ptr.*);
    }

    return result.toOwnedSlice(allocator) catch {
        result.deinit(allocator);
        return null;
    };
}


    /// Find candidate files matching a RegexQuery.
    /// Intersects AND trigrams, then for each OR group unions posting lists
    /// and intersects with the running result.
    pub fn candidatesRegex(self: *TrigramIndex, query: *const RegexQuery, allocator: std.mem.Allocator) ?[]const []const u8 {
        if (query.and_trigrams.len == 0 and query.or_groups.len == 0) return null;

        // Start with AND trigrams
        var result_set: ?std.StringHashMap(void) = null;
        defer if (result_set) |*rs| rs.deinit();

        if (query.and_trigrams.len > 0) {
            // Intersect all AND trigram posting lists
            for (query.and_trigrams) |tri| {
                const file_set = self.index.getPtr(tri) orelse {
                    // Trigram not in index → no files can match
                    var empty = allocator.alloc([]const u8, 0) catch return null;
                    _ = &empty;
                    return allocator.alloc([]const u8, 0) catch null;
                };
                if (result_set == null) {
                    // Initialize with all files from first trigram
                    result_set = std.StringHashMap(void).init(allocator);
                    var it = file_set.keyIterator();
                    while (it.next()) |key| {
                        result_set.?.put(key.*, {}) catch return null;
                    }
                } else {
                    // Intersect: remove files not in this posting list
                    var to_remove: std.ArrayList([]const u8) = .{};
                    defer to_remove.deinit(allocator);
                    var it = result_set.?.keyIterator();
                    while (it.next()) |key| {
                        if (!file_set.contains(key.*)) {
                            to_remove.append(allocator, key.*) catch return null;
                        }
                    }
                    for (to_remove.items) |key| {
                        _ = result_set.?.remove(key);
                    }
                }
            }
        }

        // Process OR groups: for each group, union posting lists of its trigrams,
        // then intersect with result_set
        for (query.or_groups) |group| {
            if (group.len == 0) continue;

            // Union all posting lists in this OR group
            var union_set = std.StringHashMap(void).init(allocator);
            defer union_set.deinit();
            for (group) |tri| {
                const file_set = self.index.getPtr(tri) orelse continue;
                var it = file_set.keyIterator();
                while (it.next()) |key| {
                    union_set.put(key.*, {}) catch return null;
                }
            }

            if (result_set == null) {
                // First constraint — adopt the union
                result_set = std.StringHashMap(void).init(allocator);
                var it = union_set.keyIterator();
                while (it.next()) |key| {
                    result_set.?.put(key.*, {}) catch return null;
                }
            } else {
                // Intersect result_set with union_set
                var to_remove: std.ArrayList([]const u8) = .{};
                defer to_remove.deinit(allocator);
                var it = result_set.?.keyIterator();
                while (it.next()) |key| {
                    if (!union_set.contains(key.*)) {
                        to_remove.append(allocator, key.*) catch return null;
                    }
                }
                for (to_remove.items) |key| {
                    _ = result_set.?.remove(key);
                }
            }
        }

        if (result_set == null) return null;

        // Convert to slice
        var result: std.ArrayList([]const u8) = .{};
        errdefer result.deinit(allocator);
        result.ensureTotalCapacity(allocator, result_set.?.count()) catch return null;
        var it = result_set.?.keyIterator();
        while (it.next()) |key| {
            result.appendAssumeCapacity(key.*);
        }
        return result.toOwnedSlice(allocator) catch {
            result.deinit(allocator);
            return null;
        };
    }

    // ── Disk persistence ────────────────────────────────────

    const POSTINGS_MAGIC = [4]u8{ 'C', 'D', 'B', 'T' };
    const LOOKUP_MAGIC = [4]u8{ 'C', 'D', 'B', 'L' };
    const FORMAT_VERSION: u16 = 3;

    /// Posting entry for v3+: file_id (u32) + next_mask (u8) + loc_mask (u8) + pad (2 bytes) = 8 bytes
    const DiskPosting = extern struct {
        file_id: u32,
        next_mask: u8,
        loc_mask: u8,
        _pad: [2]u8 = .{ 0, 0 },
    };

    /// Posting entry for v1/v2 files: file_id (u16) + next_mask (u8) + loc_mask (u8) = 4 bytes
    const OldDiskPosting = extern struct {
        file_id: u16,
        next_mask: u8,
        loc_mask: u8,
    };

    /// Lookup entry: trigram (u32 low 24 bits) + offset (u32) + count (u32) = 12 bytes
    const LookupEntry = extern struct {
        trigram: u32,
        offset: u32,
        count: u32,
    };

    /// Write the current in-memory index to disk in a two-file format.
    /// Files are written atomically (write to tmp, then rename).
    pub fn writeToDisk(self: *TrigramIndex, dir_path: []const u8, git_head: ?[40]u8) !void {
        // Step 1: Build file table (assign u16 IDs to all unique paths)
        var file_table: std.ArrayList([]const u8) = .{};
        defer file_table.deinit(self.allocator);
        var path_to_id = std.StringHashMap(u32).init(self.allocator);
        defer path_to_id.deinit();

        var ft_iter = self.file_trigrams.keyIterator();
        while (ft_iter.next()) |path_ptr| {
            const id: u32 = @intCast(file_table.items.len);
            try file_table.append(self.allocator, path_ptr.*);
            try path_to_id.put(path_ptr.*, id);
        }

        const file_count: u32 = @intCast(file_table.items.len);

        // Step 2: Collect all trigrams, sort them, serialize postings contiguously
        var trigrams_sorted: std.ArrayList(Trigram) = .{};
        defer trigrams_sorted.deinit(self.allocator);
        {
            var tri_iter = self.index.keyIterator();
            while (tri_iter.next()) |tri_ptr| {
                try trigrams_sorted.append(self.allocator, tri_ptr.*);
            }
        }
        std.mem.sort(Trigram, trigrams_sorted.items, {}, struct {
            fn lt(_: void, a: Trigram, b: Trigram) bool {
                return a < b;
            }
        }.lt);

        // Step 3: Build postings blob and lookup entries
        var postings_buf: std.ArrayList(DiskPosting) = .{};
        defer postings_buf.deinit(self.allocator);
        var lookup_entries: std.ArrayList(LookupEntry) = .{};
        defer lookup_entries.deinit(self.allocator);

        for (trigrams_sorted.items) |tri| {
            const file_set = self.index.getPtr(tri) orelse continue;
            const offset: u32 = @intCast(postings_buf.items.len);
            var count: u32 = 0;
            var fs_iter = file_set.iterator();
            while (fs_iter.next()) |entry| {
                const fid = path_to_id.get(entry.key_ptr.*) orelse continue;
                try postings_buf.append(self.allocator, .{
                    .file_id = fid,
                    .next_mask = entry.value_ptr.next_mask,
                    .loc_mask = entry.value_ptr.loc_mask,
                });
                count += 1;
            }
            try lookup_entries.append(self.allocator, .{
                .trigram = @as(u32, tri),
                .offset = offset,
                .count = count,
            });
        }

        // Step 4: Write postings file atomically (random suffix prevents collisions)
        const post_rand = std.crypto.random.int(u64);
        const postings_tmp = try std.fmt.allocPrint(self.allocator, "{s}/trigram.postings.{x}.tmp", .{ dir_path, post_rand });
        defer self.allocator.free(postings_tmp);
        const postings_final = try std.fmt.allocPrint(self.allocator, "{s}/trigram.postings", .{dir_path});
        defer self.allocator.free(postings_final);

        {
            const file = try std.fs.cwd().createFile(postings_tmp, .{});
            defer file.close();

            // Header v3: magic(4) + version(2) + file_count(4) + head_len(1) + head(40) = 51 bytes
            try file.writeAll(&POSTINGS_MAGIC);
            var ver_buf: [2]u8 = undefined;
            std.mem.writeInt(u16, &ver_buf, FORMAT_VERSION, .little);
            try file.writeAll(&ver_buf);
            var fc_buf: [4]u8 = undefined;
            std.mem.writeInt(u32, &fc_buf, file_count, .little);
            try file.writeAll(&fc_buf);
            // Git HEAD: head_len (1 byte) + head (40 bytes)
            if (git_head) |head| {
                try file.writeAll(&.{40});
                try file.writeAll(&head);
            } else {
                try file.writeAll(&.{0});
                try file.writeAll(&([_]u8{0} ** 40));
            }

            // File table: for each file, path_len(u16) + path bytes
            for (file_table.items) |path| {
                var pl_buf: [2]u8 = undefined;
                std.mem.writeInt(u16, &pl_buf, @intCast(path.len), .little);
                try file.writeAll(&pl_buf);
                try file.writeAll(path);
            }

            // Postings data
            const postings_bytes = std.mem.sliceAsBytes(postings_buf.items);
            try file.writeAll(postings_bytes);
        }
        try std.fs.cwd().rename(postings_tmp, postings_final);

        // Step 5: Write lookup file atomically (random suffix prevents collisions)
        const lk_rand = std.crypto.random.int(u64);
        const lookup_tmp = try std.fmt.allocPrint(self.allocator, "{s}/trigram.lookup.{x}.tmp", .{ dir_path, lk_rand });
        defer self.allocator.free(lookup_tmp);
        const lookup_final = try std.fmt.allocPrint(self.allocator, "{s}/trigram.lookup", .{dir_path});
        defer self.allocator.free(lookup_final);

        {
            const file = try std.fs.cwd().createFile(lookup_tmp, .{});
            defer file.close();

            // Header: magic(4) + version(2) + pad(2) + entry_count(4) = 12 bytes
            try file.writeAll(&LOOKUP_MAGIC);
            var ver_buf2: [2]u8 = undefined;
            std.mem.writeInt(u16, &ver_buf2, FORMAT_VERSION, .little);
            try file.writeAll(&ver_buf2);
            var pad_buf: [2]u8 = .{ 0, 0 };
            try file.writeAll(&pad_buf);
            var ec_buf: [4]u8 = undefined;
            std.mem.writeInt(u32, &ec_buf, @intCast(lookup_entries.items.len), .little);
            try file.writeAll(&ec_buf);

            // Entries (already aligned at 12 bytes each)
            const entry_bytes = std.mem.sliceAsBytes(lookup_entries.items);
            try file.writeAll(entry_bytes);
        }
        try std.fs.cwd().rename(lookup_tmp, lookup_final);
    }

    /// Load index from disk files into a fresh TrigramIndex.
    /// Returns null if files don't exist or are corrupt/stale.
    pub fn readFromDisk(dir_path: []const u8, allocator: std.mem.Allocator) ?TrigramIndex {
        return readFromDiskInner(dir_path, allocator) catch null;
    }

    fn readFromDiskInner(dir_path: []const u8, allocator: std.mem.Allocator) !?TrigramIndex {
        const postings_path = try std.fmt.allocPrint(allocator, "{s}/trigram.postings", .{dir_path});
        defer allocator.free(postings_path);
        const lookup_path = try std.fmt.allocPrint(allocator, "{s}/trigram.lookup", .{dir_path});
        defer allocator.free(lookup_path);

        // Read both files
        const postings_data = std.fs.cwd().readFileAlloc(allocator, postings_path, 64 * 1024 * 1024) catch return null;
        defer allocator.free(postings_data);
        const lookup_data = std.fs.cwd().readFileAlloc(allocator, lookup_path, 64 * 1024 * 1024) catch return null;
        defer allocator.free(lookup_data);

        // Validate postings header (v1: 8 bytes, v2: 49 bytes, v3: 51 bytes)
        if (postings_data.len < 8) return null;
        if (!std.mem.eql(u8, postings_data[0..4], &POSTINGS_MAGIC)) return null;
        const post_version = std.mem.readInt(u16, postings_data[4..6], .little);
        if (post_version < 1 or post_version > FORMAT_VERSION) return null;
        const file_count: u32 = if (post_version >= 3)
            std.mem.readInt(u32, postings_data[6..10], .little)
        else
            std.mem.readInt(u16, postings_data[6..8], .little);

        const file_table_start: usize = if (post_version >= 3) blk: {
            if (postings_data.len < 51) return null;
            break :blk 51;
        } else if (post_version >= 2) blk: {
            if (postings_data.len < 49) return null;
            break :blk 49;
        } else 8;

        // Parse file table
        var file_paths = try allocator.alloc([]u8, file_count);
        var parsed_files: u32 = 0;
        defer {
            for (0..parsed_files) |i| allocator.free(file_paths[i]);
            allocator.free(file_paths);
        }
        var pos: usize = file_table_start;
        for (0..file_count) |i| {
            if (pos + 2 > postings_data.len) return null;
            const path_len = std.mem.readInt(u16, postings_data[pos..][0..2], .little);
            pos += 2;
            if (pos + path_len > postings_data.len) return null;
            file_paths[i] = try allocator.dupe(u8, postings_data[pos .. pos + path_len]);
            parsed_files += 1;
            pos += path_len;
        }

        // Remaining bytes are DiskPosting entries
        const postings_start = pos;
        const postings_byte_len = postings_data.len - postings_start;
        const posting_size: usize = if (post_version >= 3) @sizeOf(DiskPosting) else @sizeOf(OldDiskPosting);
        if (postings_byte_len % posting_size != 0) return null;
        const total_postings = postings_byte_len / posting_size;

        // Validate lookup header
        if (lookup_data.len < 12) return null;
        if (!std.mem.eql(u8, lookup_data[0..4], &LOOKUP_MAGIC)) return null;
        const lk_version = std.mem.readInt(u16, lookup_data[4..6], .little);
        if (lk_version < 1 or lk_version > FORMAT_VERSION) return null;
        const entry_count = std.mem.readInt(u32, lookup_data[8..12], .little);
        if (lookup_data.len < 12 + entry_count * @sizeOf(LookupEntry)) return null;

        // Build in-memory index
        var result = TrigramIndex.init(allocator);
        result.owns_paths = true;
        errdefer result.deinit();

        // Allocate stable path strings owned by the index
        var stable_paths = try allocator.alloc([]const u8, file_count);
        defer allocator.free(stable_paths);
        for (0..file_count) |i| {
            const duped = try allocator.dupe(u8, file_paths[i]);
            errdefer allocator.free(duped);
            stable_paths[i] = duped;
            try result.file_trigrams.put(duped, .{});
        }

        // Parse lookup entries and populate index + file_trigrams
        for (0..entry_count) |e| {
            const entry_off = 12 + e * @sizeOf(LookupEntry);
            const raw = lookup_data[entry_off..][0..@sizeOf(LookupEntry)];
            const entry: *align(1) const LookupEntry = @ptrCast(raw.ptr);

            const tri: Trigram = @intCast(entry.trigram);
            const p_off = entry.offset;
            const p_count = entry.count;

            if (@as(u64, p_off) + @as(u64, p_count) > @as(u64, total_postings)) return error.InvalidData;

            var file_set = std.StringHashMap(PostingMask).init(allocator);
            errdefer file_set.deinit();

            for (0..p_count) |pi| {
                const pb_off = postings_start + (p_off + pi) * posting_size;
                const raw_posting = postings_data[pb_off..][0..posting_size];
                const file_id: u32 = if (post_version >= 3)
                    std.mem.readInt(u32, raw_posting[0..4], .little)
                else
                    std.mem.readInt(u16, raw_posting[0..2], .little);
                const next_mask = raw_posting[if (post_version >= 3) 4 else 2];
                const loc_mask = raw_posting[if (post_version >= 3) 5 else 3];

                if (file_id >= file_count) return error.InvalidData;
                const path = stable_paths[file_id];

                const gop = try file_set.getOrPut(path);
                if (!gop.found_existing) {
                    gop.value_ptr.* = PostingMask{};
                }
                gop.value_ptr.next_mask |= next_mask;
                gop.value_ptr.loc_mask |= loc_mask;

                // Track trigram in file_trigrams
                if (result.file_trigrams.getPtr(path)) |tri_list| {
                    var found = false;
                    for (tri_list.items) |existing| {
                        if (existing == tri) { found = true; break; }
                    }
                    if (!found) try tri_list.append(allocator, tri);
                }
            }

            try result.index.put(tri, file_set);
        }

        return result;
    }

    /// Returns the number of indexed files (for staleness checks).
    pub fn fileCount(self: *TrigramIndex) u32 {
        return @intCast(self.file_trigrams.count());
    }

    /// Header info that can be read without loading the full index.
    pub const DiskHeader = struct {
        file_count: u32,
        git_head: ?[40]u8,
    };

    /// Read just the postings file header — fast, no full file load.
    /// Returns null if the file doesn't exist or has an unrecognised format.
    pub fn readDiskHeader(dir_path: []const u8, allocator: std.mem.Allocator) !?DiskHeader {
        const postings_path = try std.fmt.allocPrint(allocator, "{s}/trigram.postings", .{dir_path});
        defer allocator.free(postings_path);

        const file = std.fs.cwd().openFile(postings_path, .{}) catch return null;
        defer file.close();

        var buf: [51]u8 = undefined;
        const n = file.readAll(&buf) catch return null;
        if (n < 8) return null;
        if (!std.mem.eql(u8, buf[0..4], &POSTINGS_MAGIC)) return null;
        const version = std.mem.readInt(u16, buf[4..6], .little);
        if (version < 1 or version > FORMAT_VERSION) return null;
        const file_count: u32 = if (version >= 3)
            std.mem.readInt(u32, buf[6..10], .little)
        else
            std.mem.readInt(u16, buf[6..8], .little);

        var git_head: ?[40]u8 = null;
        if (version >= 3 and n >= 51) {
            const head_len = buf[10];
            if (head_len == 40) {
                var head: [40]u8 = undefined;
                @memcpy(&head, buf[11..51]);
                git_head = head;
            }
        } else if (version >= 2 and n >= 49) {
            const head_len = buf[8];
            if (head_len == 40) {
                var head: [40]u8 = undefined;
                @memcpy(&head, buf[9..49]);
                git_head = head;
            }
        }
        return DiskHeader{ .file_count = file_count, .git_head = git_head };
    }

    /// Read the git HEAD stored in the disk index header.
    /// Returns null if no git HEAD is stored or the file doesn't exist.
    pub fn readGitHead(dir_path: []const u8, allocator: std.mem.Allocator) !?[40]u8 {
        const header = try readDiskHeader(dir_path, allocator) orelse return null;
        return header.git_head;
    }

};


// ── Regex decomposition ─────────────────────────────────────

pub const RegexQuery = struct {
    and_trigrams: []Trigram,
    or_groups: [][]Trigram,
    allocator: std.mem.Allocator,

    pub fn deinit(self: *RegexQuery) void {
        self.allocator.free(self.and_trigrams);
        for (self.or_groups) |group| {
            self.allocator.free(group);
        }
        self.allocator.free(self.or_groups);
    }
};

/// Parse a regex pattern and extract literal segments that yield trigrams.
/// Handles: . \s \w \d * + ? | [...] \ (escapes)
/// Literal runs >= 3 chars produce AND trigrams.
/// Alternations (foo|bar) produce OR groups.
pub fn decomposeRegex(pattern: []const u8, allocator: std.mem.Allocator) !RegexQuery {
    // First check if this is an alternation at the top level
    // We need to respect grouping: only split on | outside of [...] and (...)
    var top_pipes: std.ArrayList(usize) = .{};
    defer top_pipes.deinit(allocator);

    {
        var depth: usize = 0;
        var in_bracket = false;
        var i: usize = 0;
        while (i < pattern.len) {
            const c = pattern[i];
            if (c == '\\' and i + 1 < pattern.len) {
                i += 2;
                continue;
            }
            if (c == '[') { in_bracket = true; i += 1; continue; }
            if (c == ']') { in_bracket = false; i += 1; continue; }
            if (in_bracket) { i += 1; continue; }
            if (c == '(') { depth += 1; i += 1; continue; }
            if (c == ')') { if (depth > 0) depth -= 1; i += 1; continue; }
            if (c == '|' and depth == 0) {
                try top_pipes.append(allocator, i);
            }
            i += 1;
        }
    }

    if (top_pipes.items.len > 0) {
        // Top-level alternation: merge all branch trigrams into a single OR group.
        // A file matching ANY branch's trigrams is a valid candidate.
        var all_tris: std.ArrayList(Trigram) = .{};
        errdefer all_tris.deinit(allocator);

        var start: usize = 0;
        for (top_pipes.items) |pipe_pos| {
            const branch = pattern[start..pipe_pos];
            const branch_tris = try extractLiteralTrigrams(branch, allocator);
            defer allocator.free(branch_tris);
            for (branch_tris) |tri| {
                try all_tris.append(allocator, tri);
            }
            start = pipe_pos + 1;
        }
        // Last branch
        const last_branch = pattern[start..];
        const last_tris = try extractLiteralTrigrams(last_branch, allocator);
        defer allocator.free(last_tris);
        for (last_tris) |tri| {
            try all_tris.append(allocator, tri);
        }

        const empty_and = try allocator.alloc(Trigram, 0);
        var or_groups: std.ArrayList([]Trigram) = .{};
        errdefer or_groups.deinit(allocator);
        if (all_tris.items.len > 0) {
            try or_groups.append(allocator, try all_tris.toOwnedSlice(allocator));
        }
        return RegexQuery{
            .and_trigrams = empty_and,
            .or_groups = try or_groups.toOwnedSlice(allocator),
            .allocator = allocator,
        };
    }

    // No top-level alternation: extract trigrams from literal segments
    const and_tris = try extractLiteralTrigrams(pattern, allocator);
    const empty_or = try allocator.alloc([]Trigram, 0);
    return RegexQuery{
        .and_trigrams = and_tris,
        .or_groups = empty_or,
        .allocator = allocator,
    };
}

/// Extract trigrams from literal runs in a regex fragment (no top-level |).
fn extractLiteralTrigrams(pattern: []const u8, allocator: std.mem.Allocator) ![]Trigram {
    var literals: std.ArrayList(u8) = .{};
    defer literals.deinit(allocator);

    var trigrams_list: std.ArrayList(Trigram) = .{};
    errdefer trigrams_list.deinit(allocator);

    // Deduplicate trigrams
    var seen = std.AutoHashMap(Trigram, void).init(allocator);
    defer seen.deinit();

    var i: usize = 0;
    while (i < pattern.len) {
        const c = pattern[i];

        // Escape sequences
        if (c == '\\' and i + 1 < pattern.len) {
            const next = pattern[i + 1];
            switch (next) {
                's', 'S', 'w', 'W', 'd', 'D', 'b', 'B' => {
                    // Character class — breaks literal chain
                    try flushLiterals(allocator, &literals, &trigrams_list, &seen);
                    i += 2;
                    // If followed by quantifier, skip it too
                    if (i < pattern.len and isQuantifier(pattern[i])) i += 1;
                    continue;
                },
                else => {
                    // Escaped literal char (e.g. \. \( \) \\ etc.)
                    try literals.append(allocator, next);
                    i += 2;
                    // Check for quantifier after escaped char
                    if (i < pattern.len and isQuantifier(pattern[i])) {
                        // Quantifier on single char — pop it and flush
                        if (literals.items.len > 0) {
                            _ = literals.pop();
                        }
                        try flushLiterals(allocator, &literals, &trigrams_list, &seen);
                        i += 1;
                    }
                    continue;
                },
            }
        }

        // Character class [...]
        if (c == '[') {
            try flushLiterals(allocator, &literals, &trigrams_list, &seen);
            // Skip to closing ]
            i += 1;
            if (i < pattern.len and pattern[i] == '^') i += 1;
            if (i < pattern.len and pattern[i] == ']') i += 1; // literal ] at start
            while (i < pattern.len and pattern[i] != ']') : (i += 1) {}
            if (i < pattern.len) i += 1; // skip ]
            // Skip quantifier after class
            if (i < pattern.len and isQuantifier(pattern[i])) i += 1;
            continue;
        }

        // Grouping parens — just skip them, process contents
        if (c == '(' or c == ')') {
            try flushLiterals(allocator, &literals, &trigrams_list, &seen);
            i += 1;
            continue;
        }

        // Anchors
        if (c == '^' or c == '$') {
            try flushLiterals(allocator, &literals, &trigrams_list, &seen);
            i += 1;
            continue;
        }

        // Dot — any char, breaks chain
        if (c == '.') {
            try flushLiterals(allocator, &literals, &trigrams_list, &seen);
            i += 1;
            if (i < pattern.len and isQuantifier(pattern[i])) i += 1;
            continue;
        }

        // Quantifiers on previous char
        if (isQuantifier(c)) {
            // Remove last literal (it's now optional/repeated)
            if (literals.items.len > 0) {
                _ = literals.pop();
            }
            try flushLiterals(allocator, &literals, &trigrams_list, &seen);
            // If it's a brace quantifier {n}, {n,m}, {n,}, skip to closing }
            if (c == '{') {
                i += 1;
                while (i < pattern.len and pattern[i] != '}') : (i += 1) {}
                if (i < pattern.len) i += 1; // skip '}'
            } else {
                i += 1;
            }
            continue;
        }

        // Plain literal character
        try literals.append(allocator, c);
        i += 1;
    }

    // Flush remaining literals
    try flushLiterals(allocator, &literals, &trigrams_list, &seen);

    return trigrams_list.toOwnedSlice(allocator);
}

fn isQuantifier(c: u8) bool {
    return c == '*' or c == '+' or c == '?' or c == '{';
}

/// Flush a run of literal characters into trigrams (if >= 3 chars).
fn flushLiterals(
    allocator: std.mem.Allocator,
    literals: *std.ArrayList(u8),
    trigrams_list: *std.ArrayList(Trigram),
    seen: *std.AutoHashMap(Trigram, void),
) !void {
    if (literals.items.len >= 3) {
        for (0..literals.items.len - 2) |j| {
            const tri = packTrigram(
                normalizeChar(literals.items[j]),
                normalizeChar(literals.items[j + 1]),
                normalizeChar(literals.items[j + 2]),
            );
            const gop = try seen.getOrPut(tri);
            if (!gop.found_existing) {
                try trigrams_list.append(allocator, tri);
            }
        }
    }
    literals.clearRetainingCapacity();
}


// ── Tokenizer ───────────────────────────────────────────────

pub const WordTokenizer = struct {
    buf: []const u8,
    pos: usize = 0,

    pub fn next(self: *WordTokenizer) ?[]const u8 {
        // Skip non-word chars
        while (self.pos < self.buf.len and !isWordChar(self.buf[self.pos])) {
            self.pos += 1;
        }
        if (self.pos >= self.buf.len) return null;

        const start = self.pos;
        while (self.pos < self.buf.len and isWordChar(self.buf[self.pos])) {
            self.pos += 1;
        }
        return self.buf[start..self.pos];
    }
};

fn isWordChar(c: u8) bool {
    return std.ascii.isAlphanumeric(c) or c == '_';
}

pub fn normalizeChar(c: u8) u8 {
    // Lowercase for case-insensitive trigram matching
    return if (c >= 'A' and c <= 'Z') c + 32 else c;
}

// ── Sparse N-gram index ───────────────────────────────────────────────────────

pub const MAX_NGRAM_LEN: usize = 16;

/// Comptime character-pair frequency table for source code.
/// Common pairs → LOW weight (they stay interior to n-grams).
/// Rare pairs   → HIGH weight (they become n-gram boundaries).
/// All unspecified pairs default to 0xFE00 (rare = high weight).
pub const default_pair_freq: [256][256]u16 = blk: {

    var table: [256][256]u16 = .{.{0xFE00} ** 256} ** 256;
    // English bigrams (lowercase) — common in identifiers and prose
    table['t']['h'] = 0x1000; table['h']['e'] = 0x1000;
    table['i']['n'] = 0x1000; table['e']['r'] = 0x1000;
    table['a']['n'] = 0x1000; table['r']['e'] = 0x1000;
    table['o']['n'] = 0x1000; table['e']['n'] = 0x1000;
    table['s']['t'] = 0x1000; table['e']['s'] = 0x1000;
    table['a']['t'] = 0x1000; table['i']['o'] = 0x1000;
    table['t']['e'] = 0x1000; table['o']['r'] = 0x1000;
    table['t']['i'] = 0x1000; table['a']['r'] = 0x1000;
    table['a']['l'] = 0x1000; table['l']['e'] = 0x1000;
    table['n']['t'] = 0x1000; table['e']['d'] = 0x1000;
    table['n']['d'] = 0x1000; table['o']['u'] = 0x1000;
    table['e']['a'] = 0x1000; table['f']['o'] = 0x1000;
    // Common code keyword fragments
    table['f']['n'] = 0x1000; table['i']['f'] = 0x1000;
    table['r']['n'] = 0x1000; table['t']['u'] = 0x1000;
    table['p']['u'] = 0x1000; table['b']['l'] = 0x1000;
    table['c']['o'] = 0x1000; table['n']['s'] = 0x1000;
    table['t']['r'] = 0x1000; table['u']['e'] = 0x1000;
    // Common operator / punctuation pairs
    table['('][')'] = 0x0800; table['{']['}'] = 0x0800;
    table['['][']'] = 0x0800; table['/']['/'] = 0x0800;
    table['-']['>'] = 0x0800; table['=']['>'] = 0x0800;
    table[':'][':'] = 0x0800; table['!']['='] = 0x0800;
    table['=']['='] = 0x0800; table['<']['='] = 0x0800;
    table['>']['='] = 0x0800; table['&']['&'] = 0x0800;
    table['|']['|'] = 0x0800;
    // Whitespace / structural pairs
    table[' '][' '] = 0x0800; table['\t'][' '] = 0x0800;
    table[' ']['('] = 0x0800; table[' ']['{'] = 0x0800;
    table[';'][' '] = 0x0800; table[':'][' '] = 0x0800;
    table['='][' '] = 0x0800; table[' ']['='] = 0x0800;
    table[','][' '] = 0x0800; table['.']['.'] = 0x0800;
    table['\n'][' '] = 0x0800; table['\n']['\t'] = 0x0800;
    break :blk table;
};

/// Active frequency table — points to the comptime default or a runtime
/// per-project table.  Swap only before indexing starts (not thread-safe).
pub var active_pair_freq: *const [256][256]u16 = &default_pair_freq;
var loaded_freq_table: [256][256]u16 = undefined;


/// Deterministic weight for a character pair, used to place content-defined
/// boundaries between n-grams.  Frequency-weighted: common source-code pairs
/// get LOW weight (they stay interior to n-grams); rare pairs get HIGH weight
/// (they become boundaries).  A small hash jitter (0-255) breaks ties
/// deterministically between pairs in the same frequency tier.
pub fn pairWeight(a: u8, b: u8) u16 {
    const freq_weight = active_pair_freq[a][b];
    const pair = [2]u8{ a, b };
    const jitter: u16 = @truncate(std.hash.Wyhash.hash(0, &pair) & 0xFF);
    return freq_weight +| jitter;
}

/// Swap in a custom frequency table.  Call before indexing; not thread-safe.
pub fn setFrequencyTable(table: *const [256][256]u16) void {
    loaded_freq_table = table.*;
    active_pair_freq = &loaded_freq_table;
}

/// Revert to the built-in comptime frequency table.
pub fn resetFrequencyTable() void {
    active_pair_freq = &default_pair_freq;
}

/// Build a per-project frequency table by counting byte-pair occurrences in
/// `content`, then inverting counts to weights (common → low, rare → high).
pub fn buildFrequencyTable(content: []const u8) [256][256]u16 {
    var counts: [256][256]u64 = .{.{0} ** 256} ** 256;
    if (content.len >= 2) {
        for (0..content.len - 1) |i| {
            counts[content[i]][content[i + 1]] += 1;
        }
    }
    return finishFrequencyTable(&counts);
}

/// Build a frequency table by streaming over multiple content slices.
/// Zero extra memory — counts pairs within each slice, skipping cross-slice
/// boundaries (negligible loss for large corpora).
pub fn buildFrequencyTableFromSlices(slices: []const []const u8) [256][256]u16 {
    var counts: [256][256]u64 = .{.{0} ** 256} ** 256;
    for (slices) |content| {
        if (content.len < 2) continue;
        for (0..content.len - 1) |i| {
            counts[content[i]][content[i + 1]] += 1;
        }
    }
    return finishFrequencyTable(&counts);
}

/// Build a frequency table by streaming over a StringHashMap of content.
/// Iterates file-by-file — no concatenation, zero extra memory.
pub fn buildFrequencyTableFromMap(contents: *const std.StringHashMap([]const u8)) [256][256]u16 {
    var counts: [256][256]u64 = .{.{0} ** 256} ** 256;
    var iter = contents.valueIterator();
    while (iter.next()) |content_ptr| {
        const content = content_ptr.*;
        if (content.len < 2) continue;
        for (0..content.len - 1) |i| {
            counts[content[i]][content[i + 1]] += 1;
        }
    }
    return finishFrequencyTable(&counts);
}

fn finishFrequencyTable(counts: *const [256][256]u64) [256][256]u16 {
    var max_count: u64 = 1;
    for (counts) |row| {
        for (row) |c| {
            if (c > max_count) max_count = c;
        }
    }
    // Invert: count 0 → 0xFE00 (rare, high); max_count → 0x1000 (common, low).
    var table: [256][256]u16 = .{.{0xFE00} ** 256} ** 256;
    for (0..256) |a| {
        for (0..256) |b| {
            const c = counts[a][b];
            if (c == 0) continue;
            const span: u64 = 0xFE00 - 0x1000;
            const w: u64 = 0xFE00 - (c * span / max_count);
            table[a][b] = @intCast(@min(w, 0xFE00));
        }
    }
    return table;
}

/// Persist a frequency table as a raw binary blob to `<dir_path>/pair_freq.bin`.
/// Uses tmp+rename for atomic writes.
pub fn writeFrequencyTable(table: *const [256][256]u16, dir_path: []const u8) !void {
    var dir = try std.fs.cwd().openDir(dir_path, .{});
    defer dir.close();
    {
        const tmp = try dir.createFile("pair_freq.bin.tmp", .{});
        defer tmp.close();
        var row_buf: [256 * 2]u8 = undefined;
        for (table) |row| {
            for (row, 0..) |val, j| {
                std.mem.writeInt(u16, row_buf[j * 2 ..][0..2], val, .little);
            }
            try tmp.writeAll(&row_buf);
        }
    }
    try dir.rename("pair_freq.bin.tmp", "pair_freq.bin");
}

/// Load a frequency table from `<dir_path>/pair_freq.bin`.
/// Returns null if the file does not exist or has the wrong size.
/// Caller owns the returned allocation.
pub fn readFrequencyTable(dir_path: []const u8, allocator: std.mem.Allocator) !?*[256][256]u16 {
    const path = try std.fmt.allocPrint(allocator, "{s}/pair_freq.bin", .{dir_path});
    defer allocator.free(path);
    const file = std.fs.cwd().openFile(path, .{}) catch |err| switch (err) {
        error.FileNotFound => return null,
        else => return err,
    };
    defer file.close();
    const expected_size = 256 * 256 * @sizeOf(u16);
    const stat = try file.stat();
    if (stat.size != expected_size) return null;
    const result = try allocator.create([256][256]u16);
    errdefer allocator.destroy(result);
    var row_buf: [256 * 2]u8 = undefined;
    for (result) |*row| {
        const n = try file.readAll(&row_buf);
        if (n != row_buf.len) {
            allocator.destroy(result);
            return null;
        }
        for (row, 0..) |*val, j| {
            val.* = std.mem.readInt(u16, row_buf[j * 2 ..][0..2], .little);
        }
    }
    return result;
}

/// A single sparse n-gram extracted from a string.
pub const SparseNgram = struct {
    hash: u64,  // Wyhash of the normalized (lowercased) n-gram bytes
    pos: usize, // byte offset in the source string
    len: usize, // byte length of the n-gram
};

fn makeNgram(content: []const u8, pos: usize, len: usize) SparseNgram {
    var buf: [MAX_NGRAM_LEN]u8 = undefined;
    for (0..len) |k| buf[k] = normalizeChar(content[pos + k]);
    return .{
        .hash = std.hash.Wyhash.hash(0, buf[0..len]),
        .pos = pos,
        .len = len,
    };
}

/// Extract sparse n-grams from `content` using content-defined boundaries.
///
/// Boundaries are placed at strict local maxima of pairWeight over the
/// normalized character pairs.  N-grams span consecutive boundaries; spans
/// wider than MAX_NGRAM_LEN are force-split into MAX_NGRAM_LEN chunks.
/// Minimum n-gram length is 3 (same as a trigram).
///
/// Caller owns the returned slice.
pub fn extractSparseNgrams(content: []const u8, allocator: std.mem.Allocator) ![]SparseNgram {
    const MIN_LEN = 3;
    if (content.len < MIN_LEN) return try allocator.alloc(SparseNgram, 0);

    const pair_count = content.len - 1;

    // Compute pair weights.
    const weights = try allocator.alloc(u16, pair_count);
    defer allocator.free(weights);
    for (0..pair_count) |i| {
        weights[i] = pairWeight(normalizeChar(content[i]), normalizeChar(content[i + 1]));
    }

    // Collect boundary pair-positions: always include 0 and pair_count-1,
    // plus any interior strict local maximum.
    var bounds: std.ArrayList(usize) = .{};
    defer bounds.deinit(allocator);

    try bounds.append(allocator, 0);
    if (pair_count >= 3) {
        for (1..pair_count - 1) |i| {
            if (weights[i] > weights[i - 1] and weights[i] > weights[i + 1]) {
                try bounds.append(allocator, i);
            }
        }
    }
    try bounds.append(allocator, pair_count - 1);

    // Emit n-grams spanning consecutive boundary positions.
    // N-gram for boundary pair at position p covers content[p .. p+2].
    var result: std.ArrayList(SparseNgram) = .{};
    errdefer result.deinit(allocator);

    var b: usize = 0;
    while (b + 1 < bounds.items.len) : (b += 1) {
        const start = bounds.items[b];
        const end_pair = bounds.items[b + 1];
        // The right-hand boundary pair covers content[end_pair .. end_pair+2].
        const ngram_end = end_pair + 2;
        const ngram_len = ngram_end - start;

        if (ngram_len < MIN_LEN) continue;

        if (ngram_len <= MAX_NGRAM_LEN) {
            try result.append(allocator, makeNgram(content, start, ngram_len));
        } else {
            // Force-split into MAX_NGRAM_LEN-sized chunks.
            var off = start;
            while (off + MAX_NGRAM_LEN <= ngram_end) {
                try result.append(allocator, makeNgram(content, off, MAX_NGRAM_LEN));
                off += MAX_NGRAM_LEN;
            }
            const rem = ngram_end - off;
            if (rem >= MIN_LEN) {
                try result.append(allocator, makeNgram(content, off, rem));
            } else if (rem > 0) {
                // Tail is too short for its own ngram.  Overlap with the
                // previous chunk by backing up to ngram_end - MIN_LEN so
                // every byte in the span is covered.
                try result.append(allocator, makeNgram(content, ngram_end - MIN_LEN, MIN_LEN));
            }

        }
    }

    return result.toOwnedSlice(allocator);
}

/// Build the covering set of n-gram hashes for a query using a sliding window.
/// Extracts every substring of the query with length in [3, MAX_NGRAM_LEN] so
/// that file boundary-based n-grams overlapping the query are matched regardless
/// of where content-defined boundaries fall in the indexed file.
/// Caller owns the returned slice.
pub fn buildCoveringSet(query: []const u8, allocator: std.mem.Allocator) ![]SparseNgram {
    const MIN_LEN = 3;
    if (query.len < MIN_LEN) return try allocator.alloc(SparseNgram, 0);

    var result: std.ArrayList(SparseNgram) = .{};
    errdefer result.deinit(allocator);

    // Slide a window of every length [MIN_LEN, MAX_NGRAM_LEN] across the query.
    // This avoids boundary-misalignment false negatives when a query substring
    // appears in the indexed file as a content-defined boundary n-gram.
    var len: usize = MIN_LEN;
    while (len <= @min(MAX_NGRAM_LEN, query.len)) : (len += 1) {
        var pos: usize = 0;
        while (pos + len <= query.len) : (pos += 1) {
            try result.append(allocator, makeNgram(query, pos, len));
        }
    }

    return result.toOwnedSlice(allocator);
}

/// In-memory sparse n-gram index.  Mirrors the TrigramIndex API so it can
/// be used as a drop-in acceleration layer alongside the trigram index.
pub const SparseNgramIndex = struct {
    /// ngram hash → set of file paths that contain the n-gram
    index: std.AutoHashMap(u64, std.StringHashMap(void)),
    /// path → list of ngram hashes contributed (for cleanup on re-index)
    file_ngrams: std.StringHashMap(std.ArrayList(u64)),
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) SparseNgramIndex {
        return .{
            .index = std.AutoHashMap(u64, std.StringHashMap(void)).init(allocator),
            .file_ngrams = std.StringHashMap(std.ArrayList(u64)).init(allocator),
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *SparseNgramIndex) void {
        var iter = self.index.iterator();
        while (iter.next()) |entry| {
            entry.value_ptr.deinit();
        }
        self.index.deinit();

        var fn_iter = self.file_ngrams.iterator();
        while (fn_iter.next()) |entry| {
            entry.value_ptr.deinit(self.allocator);
        }
        self.file_ngrams.deinit();
    }

    pub fn removeFile(self: *SparseNgramIndex, path: []const u8) void {
        const ngrams = self.file_ngrams.getPtr(path) orelse return;
        for (ngrams.items) |hash| {
            if (self.index.getPtr(hash)) |file_set| {
                _ = file_set.remove(path);
                if (file_set.count() == 0) {
                    file_set.deinit();
                    _ = self.index.remove(hash);
                }
            }
        }
        ngrams.deinit(self.allocator);
        _ = self.file_ngrams.remove(path);
    }

    pub fn indexFile(self: *SparseNgramIndex, path: []const u8, content: []const u8) !void {
        self.removeFile(path);

        const ngrams = try extractSparseNgrams(content, self.allocator);
        defer self.allocator.free(ngrams);

        // Deduplicate hashes so the cleanup list stays compact.
        var seen = std.AutoHashMap(u64, void).init(self.allocator);
        defer seen.deinit();

        for (ngrams) |ng| {
            const gop = try self.index.getOrPut(ng.hash);
            if (!gop.found_existing) {
                gop.value_ptr.* = std.StringHashMap(void).init(self.allocator);
            }
            _ = try gop.value_ptr.getOrPut(path);
            _ = try seen.getOrPut(ng.hash);
        }

        var hash_list: std.ArrayList(u64) = .{};
        errdefer hash_list.deinit(self.allocator);
        var seen_iter = seen.keyIterator();
        while (seen_iter.next()) |h| {
            try hash_list.append(self.allocator, h.*);
        }
        try self.file_ngrams.put(path, hash_list);
    }

    /// Find candidate files that may contain the query string.
    /// Uses the sliding-window covering set from buildCoveringSet and returns
    /// the UNION of all matching posting lists — a superset of true matches,
    /// to be verified by content search.  Returns null when the query is too
    /// short.  Caller must free the returned slice.
    pub fn candidates(self: *SparseNgramIndex, query: []const u8, allocator: std.mem.Allocator) ?[]const []const u8 {
        const ngrams = buildCoveringSet(query, allocator) catch return null;
        defer allocator.free(ngrams);

        if (ngrams.len == 0) return null;

        // Union posting sets for all sliding-window n-gram hashes.
        // A file is a candidate if it shares any substring with the query.
        var seen_files = std.StringHashMap(void).init(allocator);
        defer seen_files.deinit();

        for (ngrams) |ng| {
            const file_set = self.index.getPtr(ng.hash) orelse continue;
            var it = file_set.keyIterator();
            while (it.next()) |path_ptr| {
                seen_files.put(path_ptr.*, {}) catch return null;
            }
        }

        if (seen_files.count() == 0) {
            return allocator.alloc([]const u8, 0) catch null;
        }

        var result: std.ArrayList([]const u8) = .{};
        errdefer result.deinit(allocator);
        result.ensureTotalCapacity(allocator, seen_files.count()) catch return null;

        var file_it = seen_files.keyIterator();
        while (file_it.next()) |path_ptr| {
            result.appendAssumeCapacity(path_ptr.*);
        }

        return result.toOwnedSlice(allocator) catch {
            result.deinit(allocator);
            return null;
        };
    }

    pub fn fileCount(self: *SparseNgramIndex) u32 {
        return @intCast(self.file_ngrams.count());
    }
};

