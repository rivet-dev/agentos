const std = @import("std");
const Store = @import("store.zig").Store;
const idx = @import("index.zig");
const WordIndex = idx.WordIndex;
const TrigramIndex = idx.TrigramIndex;
const SparseNgramIndex = idx.SparseNgramIndex;


pub const SymbolKind = enum(u8) {
    function,
    struct_def,
    enum_def,
    union_def,
    constant,
    variable,
    import,
    test_decl,
    comment_block,
    trait_def,
    impl_block,
    type_alias,
    macro_def,
    method,
};

pub const Symbol = struct {
    name: []const u8,
    kind: SymbolKind,
    line_start: u32,
    line_end: u32,
    detail: ?[]const u8 = null,
};

pub const FileOutline = struct {
    path: []const u8,
    language: Language,
    line_count: u32,
    byte_size: u64,
    symbols: std.ArrayList(Symbol) = .{},
    imports: std.ArrayList([]const u8) = .{},
    allocator: std.mem.Allocator,
    owns_path: bool = false,

    pub fn init(allocator: std.mem.Allocator, path: []const u8) FileOutline {
        return .{
            .path = path,
            .language = detectLanguage(path),
            .line_count = 0,
            .byte_size = 0,
            .allocator = allocator,
        };
    }
    pub fn deinit(self: *FileOutline) void {
        if (self.owns_path) self.allocator.free(self.path);
        for (self.symbols.items) |sym| {
            self.allocator.free(sym.name);
            if (sym.detail) |d| self.allocator.free(d);
        }
        self.symbols.deinit(self.allocator);
        for (self.imports.items) |imp| self.allocator.free(imp);
        self.imports.deinit(self.allocator);
    }
};

pub const Language = enum(u8) {
    zig,
    c,
    cpp,
    python,
    javascript,
    typescript,
    rust,
    go_lang,
    markdown,
    json,
    yaml,
    unknown,
};

pub fn detectLanguage(path: []const u8) Language {
    if (std.mem.endsWith(u8, path, ".zig")) return .zig;
    if (std.mem.endsWith(u8, path, ".c") or std.mem.endsWith(u8, path, ".h")) return .c;
    if (std.mem.endsWith(u8, path, ".cpp") or std.mem.endsWith(u8, path, ".hpp")) return .cpp;
    if (std.mem.endsWith(u8, path, ".py")) return .python;
    if (std.mem.endsWith(u8, path, ".js") or std.mem.endsWith(u8, path, ".jsx")) return .javascript;
    if (std.mem.endsWith(u8, path, ".ts") or std.mem.endsWith(u8, path, ".tsx")) return .typescript;
    if (std.mem.endsWith(u8, path, ".rs")) return .rust;
    if (std.mem.endsWith(u8, path, ".go")) return .go_lang;
    if (std.mem.endsWith(u8, path, ".md")) return .markdown;
    if (std.mem.endsWith(u8, path, ".json")) return .json;
    if (std.mem.endsWith(u8, path, ".yaml") or std.mem.endsWith(u8, path, ".yml")) return .yaml;
    return .unknown;
}

pub const SymbolResult = struct {
    path: []const u8,
    symbol: Symbol,
};

pub const SearchResult = struct {
    path: []const u8,
    line_num: u32,
    line_text: []const u8,
};

pub const Explorer = struct {
    outlines: std.StringHashMap(FileOutline),
    dep_graph: std.StringHashMap(std.ArrayList([]const u8)),
    contents: std.StringHashMap([]const u8),
    word_index: WordIndex,
    trigram_index: TrigramIndex,
    sparse_ngram_index: SparseNgramIndex,
    allocator: std.mem.Allocator,
    mu: std.Thread.RwLock = .{},

    pub fn init(allocator: std.mem.Allocator) Explorer {
        return .{
            .outlines = std.StringHashMap(FileOutline).init(allocator),
            .dep_graph = std.StringHashMap(std.ArrayList([]const u8)).init(allocator),
            .contents = std.StringHashMap([]const u8).init(allocator),
            .word_index = WordIndex.init(allocator),
            .trigram_index = TrigramIndex.init(allocator),
            .sparse_ngram_index = SparseNgramIndex.init(allocator),
            .allocator = allocator,
        };
    }


    pub fn deinit(self: *Explorer) void {
        var iter = self.outlines.iterator();
        while (iter.next()) |entry| {
            self.allocator.free(entry.key_ptr.*);
            entry.value_ptr.deinit();
        }
        self.outlines.deinit();

        var dep_iter = self.dep_graph.iterator();
        while (dep_iter.next()) |entry| {
            entry.value_ptr.deinit(self.allocator);
        }
        self.dep_graph.deinit();

        var content_iter = self.contents.iterator();
        while (content_iter.next()) |entry| {
            self.allocator.free(entry.value_ptr.*);
        }
        self.contents.deinit();

        self.word_index.deinit();
        self.trigram_index.deinit();
        self.sparse_ngram_index.deinit();
    }

    pub fn indexFile(self: *Explorer, path: []const u8, content: []const u8) !void {
        return self.indexFileInner(path, content, true, false);
    }

    /// Fast path: index outline + content storage only, skip word/trigram indexes.
    pub fn indexFileOutlineOnly(self: *Explorer, path: []const u8, content: []const u8) !void {
        return self.indexFileInner(path, content, false, false);
    }

    /// Index outline + word index but skip trigram construction (used when trigram is loaded from disk cache).
    pub fn indexFileSkipTrigram(self: *Explorer, path: []const u8, content: []const u8) !void {
        return self.indexFileInner(path, content, true, true);
    }


fn indexFileInner(self: *Explorer, path: []const u8, content: []const u8, full_index: bool, skip_trigram: bool) !void {
    // Parse outline outside the global explorer write lock.
    // This keeps HTTP/MCP readers from being blocked on line-by-line parsing.
    var outline = FileOutline.init(self.allocator, path);
    errdefer outline.deinit();
    outline.byte_size = content.len;

    var line_num: u32 = 0;
    var prev_line_trimmed: []const u8 = "";
    var lines = std.mem.splitScalar(u8, content, '\n');
    while (lines.next()) |line| {
        line_num += 1;
        const trimmed = std.mem.trim(u8, line, " \t");

        if (outline.language == .zig) {
            try self.parseZigLine(trimmed, line_num, &outline);
        } else if (outline.language == .python) {
            try self.parsePythonLine(trimmed, line_num, &outline);
        } else if (outline.language == .typescript or outline.language == .javascript) {
            try self.parseTsLine(trimmed, line_num, &outline);
        } else if (outline.language == .rust) {
            try self.parseRustLine(trimmed, line_num, &outline, prev_line_trimmed);
        }

        prev_line_trimmed = trimmed;
    }
    outline.line_count = line_num;

    self.mu.lock();
    defer self.mu.unlock();

    // Reuse existing key if file was already indexed, else dupe.
    const outline_gop = try self.outlines.getOrPut(path);
    const is_new = !outline_gop.found_existing;
    var prior_outline: ?FileOutline = if (outline_gop.found_existing)
        outline_gop.value_ptr.*
    else
        null;
    const stable_path = if (outline_gop.found_existing) blk: {
        break :blk outline_gop.key_ptr.*;
    } else blk: {
        const duped = try self.allocator.dupe(u8, path);
        outline_gop.key_ptr.* = duped;
        break :blk duped;
    };
    // If we added a new entry but later fail, remove it so the map stays consistent.
    errdefer if (is_new) {
        _ = self.outlines.remove(stable_path);
        self.allocator.free(stable_path);
    };

    // Ensure outline path uses the stable map key.
    outline.path = stable_path;

    const duped_content = try self.allocator.dupe(u8, content);
    errdefer self.allocator.free(duped_content);
    const content_gop = try self.contents.getOrPut(stable_path);
    var prior_content: ?[]const u8 = null;
    if (content_gop.found_existing) {
        prior_content = content_gop.value_ptr.*;
    } else {
        content_gop.key_ptr.* = stable_path;
    }
    content_gop.value_ptr.* = duped_content;
    errdefer {
        if (content_gop.found_existing) {
            content_gop.value_ptr.* = prior_content.?;
        } else {
            _ = self.contents.remove(stable_path);
        }
    }

    // Build search indexes.
    if (full_index) {
        try self.word_index.indexFile(stable_path, content);
        if (!skip_trigram) {
            try self.trigram_index.indexFile(stable_path, content);
            try self.sparse_ngram_index.indexFile(stable_path, content);
        }
    }


    try self.rebuildDepsFor(stable_path, &outline);

    outline_gop.value_ptr.* = outline;
    if (prior_content) |old_content| {
        self.allocator.free(old_content);
    }
    if (prior_outline) |*old_outline| {
        old_outline.deinit();
    }
}
    /// Rebuild trigram index from the stored file contents.
    /// Used after a cache hit to populate trigrams when they were skipped during the fast scan.
    pub fn rebuildTrigrams(self: *Explorer) !void {
        self.mu.lock();
        defer self.mu.unlock();
        var iter = self.contents.iterator();
        while (iter.next()) |entry| {
            try self.trigram_index.indexFile(entry.key_ptr.*, entry.value_ptr.*);
            try self.sparse_ngram_index.indexFile(entry.key_ptr.*, entry.value_ptr.*);
        }
    }


    pub fn removeFile(self: *Explorer, path: []const u8) void {
        self.mu.lock();
        defer self.mu.unlock();
        if (self.dep_graph.getPtr(path)) |deps| {
            deps.deinit(self.allocator);
            _ = self.dep_graph.remove(path);
        }
        if (self.contents.getPtr(path)) |content| {
            self.allocator.free(content.*);
            _ = self.contents.remove(path);
        }
        self.word_index.removeFile(path);
        self.trigram_index.removeFile(path);
        self.sparse_ngram_index.removeFile(path);

        if (self.outlines.fetchRemove(path)) |kv| {
            var outline = kv.value;
            outline.deinit();
            self.allocator.free(kv.key);
        }
    }

    pub fn getOutline(self: *Explorer, path: []const u8, allocator: std.mem.Allocator) !?FileOutline {
        self.mu.lockShared();
        defer self.mu.unlockShared();

        const outline = self.outlines.getPtr(path) orelse return null;
        return try cloneOutline(outline, allocator);
    }

    /// Return a caller-owned copy of cached file content.
pub fn getContent(self: *Explorer, path: []const u8, allocator: std.mem.Allocator) !?[]u8 {
    self.mu.lockShared();
    defer self.mu.unlockShared();

    const content = self.contents.get(path) orelse return null;
    return try allocator.dupe(u8, content);
}

fn cloneOutline(src: *const FileOutline, allocator: std.mem.Allocator) !FileOutline {
    const copied_path = try allocator.dupe(u8, src.path);
    // No errdefer here: dst.deinit() below handles freeing copied_path via owns_path.

    var dst = FileOutline.init(allocator, copied_path);
    dst.owns_path = true;
    errdefer dst.deinit();
    dst.line_count = src.line_count;
    dst.byte_size = src.byte_size;
    for (src.symbols.items) |sym| {
        const copied_name = try allocator.dupe(u8, sym.name);
        errdefer allocator.free(copied_name);

        const copied_detail = if (sym.detail) |d| blk: {
            const detail = try allocator.dupe(u8, d);
            break :blk detail;
        } else null;
        errdefer if (copied_detail) |d| allocator.free(d);

        try dst.symbols.append(allocator, .{
            .name = copied_name,
            .kind = sym.kind,
            .line_start = sym.line_start,
            .line_end = sym.line_end,
            .detail = copied_detail,
        });
    }
    for (src.imports.items) |imp| {
        const copied_import = try allocator.dupe(u8, imp);
        errdefer allocator.free(copied_import);
        try dst.imports.append(allocator, copied_import);
    }

    return dst;
}

pub fn getTree(self: *Explorer, allocator: std.mem.Allocator, use_color: bool) ![]u8 {
    const s = @import("style.zig").style(use_color);

    self.mu.lockShared();
    defer self.mu.unlockShared();

    var buf: std.ArrayList(u8) = .{};
    errdefer buf.deinit(allocator);
    const writer = buf.writer(allocator);

    var paths: std.ArrayList([]const u8) = .{};
    defer paths.deinit(allocator);

    var iter = self.outlines.iterator();
    while (iter.next()) |entry| {
        try paths.append(allocator, entry.key_ptr.*);
    }

    std.mem.sort([]const u8, paths.items, {}, struct {
        fn lessThan(_: void, a: []const u8, b: []const u8) bool {
            return std.mem.order(u8, a, b) == .lt;
        }
    }.lessThan);

    var seen_dirs = std.StringHashMap(void).init(allocator);
    defer seen_dirs.deinit();

    for (paths.items) |path| {
        const outline = self.outlines.get(path) orelse continue;

        // Emit directory nodes we haven't seen yet
        var prefix_end: usize = 0;
        while (std.mem.indexOfScalarPos(u8, path, prefix_end, '/')) |sep| {
            const dir = path[0 .. sep + 1];
            if (!seen_dirs.contains(dir)) {
                try seen_dirs.put(dir, {});
                const depth = std.mem.count(u8, dir[0..sep], "/");
                for (0..depth) |_| try writer.writeAll("  ");
                const dir_name = path[if (depth > 0) std.mem.lastIndexOfScalar(u8, dir[0..sep], '/').? + 1 else 0 .. sep];
                try writer.print("{s}{s}/{s}\n", .{ s.bold, dir_name, s.reset });
            }
            prefix_end = sep + 1;
        }

        // Emit file leaf
        const depth = std.mem.count(u8, path, "/");
        for (0..depth) |_| try writer.writeAll("  ");
        const basename = if (std.mem.lastIndexOfScalar(u8, path, '/')) |pos| path[pos + 1 ..] else path;
        const lang = @tagName(outline.language);
        try writer.print("{s}  {s}{s}{s}  {s}{d}L  {d} sym{s}\n", .{
            basename,
            s.langColor(lang), lang, s.reset,
            s.dim, outline.line_count, outline.symbols.items.len, s.reset,
        });
    }

    return buf.toOwnedSlice(allocator);
}

    pub fn findSymbol(self: *Explorer, name: []const u8, allocator: std.mem.Allocator) !?struct { path: []const u8, symbol: Symbol } {
        self.mu.lockShared();
        defer self.mu.unlockShared();

        var iter = self.outlines.iterator();
        while (iter.next()) |entry| {
            for (entry.value_ptr.symbols.items) |sym| {
                if (std.mem.eql(u8, sym.name, name)) {
                    return .{
                        .path = try allocator.dupe(u8, entry.key_ptr.*),
                        .symbol = .{
                            .name = try allocator.dupe(u8, sym.name),
                            .kind = sym.kind,
                            .line_start = sym.line_start,
                            .line_end = sym.line_end,
                            .detail = if (sym.detail) |d| try allocator.dupe(u8, d) else null,
                        },
                    };
                }
            }
        }
        return null;
    }

    pub fn findAllSymbols(self: *Explorer, name: []const u8, allocator: std.mem.Allocator) ![]const SymbolResult {
        self.mu.lockShared();
        defer self.mu.unlockShared();

        var result_list: std.ArrayList(SymbolResult) = .{};
        errdefer result_list.deinit(allocator);
        var iter = self.outlines.iterator();
        while (iter.next()) |entry| {
            for (entry.value_ptr.symbols.items) |sym| {
                if (std.mem.eql(u8, sym.name, name)) {
                    try result_list.append(allocator, .{
                        .path = try allocator.dupe(u8, entry.key_ptr.*),
                        .symbol = .{
                            .name = try allocator.dupe(u8, sym.name),
                            .kind = sym.kind,
                            .line_start = sym.line_start,
                            .line_end = sym.line_end,
                            .detail = if (sym.detail) |d| try allocator.dupe(u8, d) else null,
                        },
                    });
                }
            }
        }
        return result_list.toOwnedSlice(allocator);
    }

    pub fn searchContent(self: *Explorer, query: []const u8, allocator: std.mem.Allocator, max_results: usize) ![]const SearchResult {
        self.mu.lockShared();
        defer self.mu.unlockShared();

        var result_list: std.ArrayList(SearchResult) = .{};
        errdefer result_list.deinit(allocator);

        // Sparse n-gram candidates (sliding-window, union semantics).
        const sparse_paths = self.sparse_ngram_index.candidates(query, allocator);
        defer if (sparse_paths) |sp| allocator.free(sp);

        // Trigram candidates — always computed so we can intersect or fall back.
        const candidate_paths = self.trigram_index.candidates(query, allocator);
        defer if (candidate_paths) |cp| allocator.free(cp);

        if (sparse_paths != null and sparse_paths.?.len > 0) {
            // Sparse has candidates: intersect with trigram to narrow results.
            if (candidate_paths != null and candidate_paths.?.len > 0) {
                var sparse_set = std.StringHashMap(void).init(allocator);
                defer sparse_set.deinit();
                for (sparse_paths.?) |p| try sparse_set.put(p, {});
                for (candidate_paths.?) |path| {
                    if (!sparse_set.contains(path)) continue;
                    const content = self.contents.get(path) orelse continue;
                    try searchInContent(path, content, query, allocator, max_results, &result_list);
                    if (result_list.items.len >= max_results) break;
                }
            } else {
                // No trigram candidates; search sparse candidates directly.
                for (sparse_paths.?) |path| {
                    const content = self.contents.get(path) orelse continue;
                    try searchInContent(path, content, query, allocator, max_results, &result_list);
                    if (result_list.items.len >= max_results) break;
                }
            }
            return result_list.toOwnedSlice(allocator);
        }

        // Sparse returned empty — fall through to trigram or brute force.
        const use_trigram = candidate_paths != null and candidate_paths.?.len > 0;
        if (use_trigram) {
            for (candidate_paths.?) |path| {
                const content = self.contents.get(path) orelse continue;
                try searchInContent(path, content, query, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
        } else {
            // Brute force (short query or no index hits).
            var iter = self.contents.iterator();
            while (iter.next()) |entry| {
                try searchInContent(entry.key_ptr.*, entry.value_ptr.*, query, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
        }

        return result_list.toOwnedSlice(allocator);
    }


    /// Search file contents using a regex pattern with trigram acceleration.
    /// Decomposes the regex to extract literal trigrams for candidate filtering,
    /// then does actual regex matching on candidates.
    pub fn searchContentRegex(self: *Explorer, pattern: []const u8, allocator: std.mem.Allocator, max_results: usize) ![]const SearchResult {
        self.mu.lockShared();
        defer self.mu.unlockShared();

        var result_list: std.ArrayList(SearchResult) = .{};
        errdefer result_list.deinit(allocator);

        // Decompose regex into trigram query
        var query = idx.decomposeRegex(pattern, self.allocator) catch {
            // If decomposition fails, fall back to brute force
            var iter = self.contents.iterator();
            while (iter.next()) |entry| {
                try searchInContentRegex(entry.key_ptr.*, entry.value_ptr.*, pattern, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
            return result_list.toOwnedSlice(allocator);
        };
        defer query.deinit();

        const candidate_paths = self.trigram_index.candidatesRegex(&query, allocator);
        defer if (candidate_paths) |cp| allocator.free(cp);
        const use_trigram = candidate_paths != null and candidate_paths.?.len > 0;

        if (use_trigram) {
            for (candidate_paths.?) |path| {
                const content = self.contents.get(path) orelse continue;
                try searchInContentRegex(path, content, pattern, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
        } else {
            // Brute force — no useful trigrams extracted
            var iter = self.contents.iterator();
            while (iter.next()) |entry| {
                try searchInContentRegex(entry.key_ptr.*, entry.value_ptr.*, pattern, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
        }

        return result_list.toOwnedSlice(allocator);
    }


    /// Search for a word using the inverted word index. O(1) lookup.
    pub fn searchWord(self: *Explorer, word: []const u8, allocator: std.mem.Allocator) ![]const idx.WordHit {
        self.mu.lockShared();
        defer self.mu.unlockShared();
        return self.word_index.searchDeduped(word, allocator);
    }

pub fn getImportedBy(self: *Explorer, path: []const u8, allocator: std.mem.Allocator) ![]const []const u8 {
    self.mu.lockShared();
    defer self.mu.unlockShared();

    // Extract basename for matching against raw import strings
    // e.g., "src/store.zig" -> "store.zig"
    const basename = if (std.mem.lastIndexOfScalar(u8, path, '/')) |pos| path[pos + 1 ..] else path;

    var result: std.ArrayList([]const u8) = .{};
    errdefer {
        for (result.items) |p| allocator.free(p);
        result.deinit(allocator);
    }

    var iter = self.dep_graph.iterator();
    while (iter.next()) |entry| {
        for (entry.value_ptr.items) |dep| {
            const normalized_dep = if (std.mem.startsWith(u8, dep, "./")) dep[2..] else dep;
            if (std.mem.eql(u8, dep, path) or
                std.mem.eql(u8, dep, basename) or
                std.mem.eql(u8, normalized_dep, path) or
                std.mem.eql(u8, normalized_dep, basename))
            {
                const dep_path = try allocator.dupe(u8, entry.key_ptr.*);
                try result.append(allocator, dep_path);
                break;
            }
        }
    }
    return result.toOwnedSlice(allocator);
}

pub fn getHotFiles(self: *Explorer, store: *Store, allocator: std.mem.Allocator, limit: usize) ![]const []const u8 {
    // Collect stable path copies under explorer lock.
    var path_list: std.ArrayList([]u8) = .{};
    errdefer {
        for (path_list.items) |path| allocator.free(path);
        path_list.deinit(allocator);
    }
    defer path_list.deinit(allocator);
    {
        self.mu.lockShared();
        defer self.mu.unlockShared();
        var iter = self.outlines.iterator();
        while (iter.next()) |kv| {
            const path_copy = try allocator.dupe(u8, kv.key_ptr.*);
            try path_list.append(allocator, path_copy);
        }
    }

    // Query store seqs without holding explorer lock.
    const Entry = struct { path: []u8, seq: u64 };
    var entries: std.ArrayList(Entry) = .{};
    defer entries.deinit(allocator);
    {
        store.mu.lock();
        defer store.mu.unlock();
        for (path_list.items) |path| {
            const seq = store.getLatestSeqUnlocked(path);
            try entries.append(allocator, .{ .path = path, .seq = seq });
        }
    }

    std.mem.sort(Entry, entries.items, {}, struct {
        fn cmp(_: void, a: Entry, b: Entry) bool {
            return a.seq > b.seq;
        }
    }.cmp);

    const count = @min(limit, entries.items.len);
    const paths = try allocator.alloc([]const u8, count);
    for (entries.items[0..count], 0..) |e, i| {
        paths[i] = e.path;
    }
    for (entries.items[count..]) |e| {
        allocator.free(e.path);
    }
    return paths;
}
    // ── Language parsers ──────────────────────────────────────

    fn parseZigLine(self: *Explorer, line: []const u8, line_num: u32, outline: *FileOutline) !void {
        const a = self.allocator;
        if (startsWith(line, "pub fn ") or startsWith(line, "fn ")) {
            const start: usize = if (startsWith(line, "pub fn ")) 7 else 3;
            if (extractIdent(line[start..])) |name| {
                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = .function,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });
            }
        } else if (startsWith(line, "pub const ") or startsWith(line, "const ")) {
            const start: usize = if (startsWith(line, "pub const ")) 10 else 6;
            if (extractIdent(line[start..])) |name| {
                const kind: SymbolKind = if (std.mem.indexOf(u8, line, "struct {") != null)
                    .struct_def
                else if (std.mem.indexOf(u8, line, "enum {") != null)
                    .enum_def
                else if (std.mem.indexOf(u8, line, "union {") != null or
                    std.mem.indexOf(u8, line, "union(enum) {") != null)
                    .union_def
                else if (std.mem.indexOf(u8, line, "@import") != null)
                    .import
                else
                    .constant;

                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = kind,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });

                if (kind == .import) {
                    if (extractStringLiteral(line)) |import_path| {
                        const import_copy = try a.dupe(u8, import_path);
                        errdefer a.free(import_copy);
                        try outline.imports.append(a, import_copy);
                    }
                }
            }
        } else if (startsWith(line, "test ")) {
            const name_copy = try a.dupe(u8, line);
            errdefer a.free(name_copy);
            try outline.symbols.append(a, .{
                .name = name_copy,
                .kind = .test_decl,
                .line_start = line_num,
                .line_end = line_num,
            });
        }
    }

    fn parsePythonLine(self: *Explorer, line: []const u8, line_num: u32, outline: *FileOutline) !void {
        const a = self.allocator;
        if (startsWith(line, "def ")) {
            if (extractIdent(line[4..])) |name| {
                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = .function,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });
            }
        } else if (startsWith(line, "class ")) {
            if (extractIdent(line[6..])) |name| {
                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = .struct_def,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });
            }
        } else if (startsWith(line, "import ") or startsWith(line, "from ")) {
            const symbol_copy = try a.dupe(u8, line);
            errdefer a.free(symbol_copy);
            try outline.symbols.append(a, .{
                .name = symbol_copy,
                .kind = .import,
                .line_start = line_num,
                .line_end = line_num,
            });
            const import_copy = try a.dupe(u8, line);
            errdefer a.free(import_copy);
            try outline.imports.append(a, import_copy);
        }
    }
    fn parseTsLine(self: *Explorer, line: []const u8, line_num: u32, outline: *FileOutline) !void {
        const a = self.allocator;
        if (containsAny(line, &.{ "function ", "const ", "export function ", "export const " })) {
            const kind: SymbolKind = if (std.mem.indexOf(u8, line, "function") != null) .function else .constant;
            const trimmed = skipKeywords(line);
            if (extractIdent(trimmed)) |name| {
                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = kind,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });
            }
        }
        if (containsAny(line, &.{ "import ", "require(" })) {
            const symbol_copy = try a.dupe(u8, line);
            errdefer a.free(symbol_copy);
            try outline.symbols.append(a, .{
                .name = symbol_copy,
                .kind = .import,
                .line_start = line_num,
                .line_end = line_num,
            });
            if (extractStringLiteral(line)) |path| {
                const import_copy = try a.dupe(u8, path);
                errdefer a.free(import_copy);
                try outline.imports.append(a, import_copy);
            }
        }
    }

    fn parseRustLine(self: *Explorer, line: []const u8, line_num: u32, outline: *FileOutline, prev_line: []const u8) !void {
        const a = self.allocator;

        // fn / pub fn / pub(crate) fn / async fn / pub async fn / unsafe fn
        if (containsAny(line, &.{"fn "})) {
            const is_decl = startsWith(line, "fn ") or
                startsWith(line, "pub fn ") or
                startsWith(line, "pub(crate) fn ") or
                startsWith(line, "pub(super) fn ") or
                startsWith(line, "async fn ") or
                startsWith(line, "pub async fn ") or
                startsWith(line, "unsafe fn ") or
                startsWith(line, "pub unsafe fn ") or
                startsWith(line, "pub(crate) async fn ") or
                startsWith(line, "pub(crate) unsafe fn ") or
                startsWith(line, "pub unsafe extern ");
            if (is_decl) {
                if (std.mem.indexOf(u8, line, "fn ")) |fn_pos| {
                    if (extractIdent(line[fn_pos + 3 ..])) |name| {
                        const is_test = std.mem.eql(u8, prev_line, "#[test]") or
                            startsWith(prev_line, "#[tokio::test");
                        const kind: SymbolKind = if (is_test) .test_decl else .function;
                        const name_copy = try a.dupe(u8, name);
                        errdefer a.free(name_copy);
                        const detail_copy = try a.dupe(u8, line);
                        errdefer a.free(detail_copy);
                        try outline.symbols.append(a, .{
                            .name = name_copy,
                            .kind = kind,
                            .line_start = line_num,
                            .line_end = line_num,
                            .detail = detail_copy,
                        });
                    }
                }
            }
        }

        // struct
        if (startsWith(line, "struct ") or startsWith(line, "pub struct ") or startsWith(line, "pub(crate) struct ")) {
            if (std.mem.indexOf(u8, line, "struct ")) |pos| {
                if (extractIdent(line[pos + 7 ..])) |name| {
                    const name_copy = try a.dupe(u8, name);
                    errdefer a.free(name_copy);
                    const detail_copy = try a.dupe(u8, line);
                    errdefer a.free(detail_copy);
                    try outline.symbols.append(a, .{
                        .name = name_copy,
                        .kind = .struct_def,
                        .line_start = line_num,
                        .line_end = line_num,
                        .detail = detail_copy,
                    });
                }
            }
        }

        // enum
        if (startsWith(line, "enum ") or startsWith(line, "pub enum ") or startsWith(line, "pub(crate) enum ")) {
            if (std.mem.indexOf(u8, line, "enum ")) |pos| {
                if (extractIdent(line[pos + 5 ..])) |name| {
                    const name_copy = try a.dupe(u8, name);
                    errdefer a.free(name_copy);
                    const detail_copy = try a.dupe(u8, line);
                    errdefer a.free(detail_copy);
                    try outline.symbols.append(a, .{
                        .name = name_copy,
                        .kind = .enum_def,
                        .line_start = line_num,
                        .line_end = line_num,
                        .detail = detail_copy,
                    });
                }
            }
        }

        // trait
        if (startsWith(line, "trait ") or startsWith(line, "pub trait ") or startsWith(line, "pub(crate) trait ") or startsWith(line, "unsafe trait ") or startsWith(line, "pub unsafe trait ")) {
            if (std.mem.indexOf(u8, line, "trait ")) |pos| {
                if (extractIdent(line[pos + 6 ..])) |name| {
                    const name_copy = try a.dupe(u8, name);
                    errdefer a.free(name_copy);
                    const detail_copy = try a.dupe(u8, line);
                    errdefer a.free(detail_copy);
                    try outline.symbols.append(a, .{
                        .name = name_copy,
                        .kind = .trait_def,
                        .line_start = line_num,
                        .line_end = line_num,
                        .detail = detail_copy,
                    });
                }
            }
        }

        // impl
        if (startsWith(line, "impl ") or startsWith(line, "impl<") or startsWith(line, "unsafe impl ")) {
            const impl_start: usize = if (startsWith(line, "unsafe impl ")) 12 else if (startsWith(line, "impl<")) blk: {
                if (std.mem.indexOf(u8, line, "> ")) |gt| {
                    break :blk gt + 2;
                } else break :blk 5;
            } else 5;
            if (extractIdent(line[impl_start..])) |name| {
                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = .impl_block,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });
            }
        }

        // type alias
        if (startsWith(line, "type ") or startsWith(line, "pub type ") or startsWith(line, "pub(crate) type ")) {
            if (std.mem.indexOf(u8, line, "type ")) |pos| {
                if (extractIdent(line[pos + 5 ..])) |name| {
                    const name_copy = try a.dupe(u8, name);
                    errdefer a.free(name_copy);
                    const detail_copy = try a.dupe(u8, line);
                    errdefer a.free(detail_copy);
                    try outline.symbols.append(a, .{
                        .name = name_copy,
                        .kind = .type_alias,
                        .line_start = line_num,
                        .line_end = line_num,
                        .detail = detail_copy,
                    });
                }
            }
        }

        // const / static
        if (startsWith(line, "const ") or startsWith(line, "pub const ") or startsWith(line, "pub(crate) const ") or
            startsWith(line, "static ") or startsWith(line, "pub static ") or startsWith(line, "pub(crate) static "))
        {
            const keyword = if (std.mem.indexOf(u8, line, "static ")) |_| "static " else "const ";
            if (std.mem.indexOf(u8, line, keyword)) |pos| {
                if (extractIdent(line[pos + keyword.len ..])) |name| {
                    const name_copy = try a.dupe(u8, name);
                    errdefer a.free(name_copy);
                    const detail_copy = try a.dupe(u8, line);
                    errdefer a.free(detail_copy);
                    try outline.symbols.append(a, .{
                        .name = name_copy,
                        .kind = .constant,
                        .line_start = line_num,
                        .line_end = line_num,
                        .detail = detail_copy,
                    });
                }
            }
        }

        // macro_rules!
        if (startsWith(line, "macro_rules!")) {
            if (extractIdent(line[13..])) |name| {
                const name_copy = try a.dupe(u8, name);
                errdefer a.free(name_copy);
                const detail_copy = try a.dupe(u8, line);
                errdefer a.free(detail_copy);
                try outline.symbols.append(a, .{
                    .name = name_copy,
                    .kind = .macro_def,
                    .line_start = line_num,
                    .line_end = line_num,
                    .detail = detail_copy,
                });
            }
        }

        // use / mod
        if (startsWith(line, "use ") or startsWith(line, "pub use ") or startsWith(line, "pub(crate) use ")) {
            const symbol_copy = try a.dupe(u8, line);
            errdefer a.free(symbol_copy);
            try outline.symbols.append(a, .{
                .name = symbol_copy,
                .kind = .import,
                .line_start = line_num,
                .line_end = line_num,
            });
            const import_copy = try a.dupe(u8, line);
            errdefer a.free(import_copy);
            try outline.imports.append(a, import_copy);
        } else if (startsWith(line, "mod ") or startsWith(line, "pub mod ") or startsWith(line, "pub(crate) mod ")) {
            if (std.mem.indexOf(u8, line, "mod ")) |pos| {
                if (extractIdent(line[pos + 4 ..])) |name| {
                    const name_copy = try a.dupe(u8, name);
                    errdefer a.free(name_copy);
                    try outline.symbols.append(a, .{
                        .name = name_copy,
                        .kind = .import,
                        .line_start = line_num,
                        .line_end = line_num,
                    });
                    const import_copy = try a.dupe(u8, name);
                    errdefer a.free(import_copy);
                    try outline.imports.append(a, import_copy);
                }
            }
        }
    }

fn rebuildDepsFor(self: *Explorer, path: []const u8, outline: *FileOutline) !void {
    var deps: std.ArrayList([]const u8) = .{};
    errdefer deps.deinit(self.allocator);

    for (outline.imports.items) |imp| {
        try deps.append(self.allocator, imp);
    }

    const gop = try self.dep_graph.getOrPut(path);
    if (gop.found_existing) {
        var old = gop.value_ptr.*;
        gop.value_ptr.* = deps;
        old.deinit(self.allocator);
    } else {
        gop.key_ptr.* = path;
        gop.value_ptr.* = deps;
    }
}

    /// Return the source body for a symbol given its file path and line range.
    /// Caller owns the returned slice.
    pub fn getSymbolBody(self: *Explorer, path: []const u8, line_start: u32, line_end: u32, allocator: std.mem.Allocator) !?[]u8 {
        self.mu.lockShared();
        defer self.mu.unlockShared();
        const content = self.contents.get(path) orelse return null;
        const result = try extractLines(content, line_start, line_end, true, false, .unknown, allocator);
        return result;
    }

    /// Find the smallest enclosing symbol for a given line in a file.
    /// Must be called while holding at least a shared lock.
    fn findEnclosingSymbolLocked(self: *Explorer, path: []const u8, line_num: u32) ?Symbol {
        const outline = self.outlines.getPtr(path) orelse return null;
        var best: ?Symbol = null;
        var best_span: u32 = std.math.maxInt(u32);
        for (outline.symbols.items) |sym| {
            if (sym.line_start <= line_num and sym.line_end >= line_num) {
                const span = sym.line_end - sym.line_start;
                if (span < best_span) {
                    best = sym;
                    best_span = span;
                }
            }
        }
        if (best != null) return best;
        // Fallback: nearest preceding symbol
        var nearest: ?Symbol = null;
        var nearest_dist: u32 = std.math.maxInt(u32);
        for (outline.symbols.items) |sym| {
            if (sym.line_start <= line_num) {
                const dist = line_num - sym.line_start;
                if (dist < nearest_dist) {
                    nearest = sym;
                    nearest_dist = dist;
                }
            }
        }
        return nearest;
    }

    pub const ScopedSearchResult = struct {
        path: []const u8,
        line_num: u32,
        line_text: []const u8,
        scope_name: ?[]const u8 = null,
        scope_kind: ?SymbolKind = null,
        scope_start: u32 = 0,
        scope_end: u32 = 0,
    };

    /// Search content and annotate results with the enclosing symbol scope.
    pub fn searchContentWithScope(self: *Explorer, query: []const u8, allocator: std.mem.Allocator, max_results: usize) ![]const ScopedSearchResult {
        self.mu.lockShared();
        defer self.mu.unlockShared();

        var result_list: std.ArrayList(ScopedSearchResult) = .{};
        errdefer {
            for (result_list.items) |r| {
                allocator.free(r.line_text);
                allocator.free(r.path);
                if (r.scope_name) |n| allocator.free(n);
            }
            result_list.deinit(allocator);
        }

        // Sparse n-gram candidates (sliding-window, union semantics).
        const sparse_paths = self.sparse_ngram_index.candidates(query, allocator);
        defer if (sparse_paths) |sp| allocator.free(sp);

        // Trigram candidates — always computed so we can intersect or fall back.
        const candidate_paths = self.trigram_index.candidates(query, allocator);
        defer if (candidate_paths) |cp| allocator.free(cp);

        if (sparse_paths != null and sparse_paths.?.len > 0) {
            // Sparse has candidates: intersect with trigram to narrow results.
            if (candidate_paths != null and candidate_paths.?.len > 0) {
                var sparse_set = std.StringHashMap(void).init(allocator);
                defer sparse_set.deinit();
                for (sparse_paths.?) |p| try sparse_set.put(p, {});
                for (candidate_paths.?) |path| {
                    if (!sparse_set.contains(path)) continue;
                    const content = self.contents.get(path) orelse continue;
                    try self.searchInContentWithScope(path, content, query, allocator, max_results, &result_list);
                    if (result_list.items.len >= max_results) break;
                }
            } else {
                // No trigram candidates; search sparse candidates directly.
                for (sparse_paths.?) |path| {
                    const content = self.contents.get(path) orelse continue;
                    try self.searchInContentWithScope(path, content, query, allocator, max_results, &result_list);
                    if (result_list.items.len >= max_results) break;
                }
            }
            return result_list.toOwnedSlice(allocator);
        }

        // Sparse returned empty — fall through to trigram or brute force.
        const use_trigram = candidate_paths != null and candidate_paths.?.len > 0;
        if (use_trigram) {
            for (candidate_paths.?) |path| {
                const content = self.contents.get(path) orelse continue;
                try self.searchInContentWithScope(path, content, query, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
        } else {
            var iter = self.contents.iterator();
            while (iter.next()) |entry| {
                try self.searchInContentWithScope(entry.key_ptr.*, entry.value_ptr.*, query, allocator, max_results, &result_list);
                if (result_list.items.len >= max_results) break;
            }
        }

        return result_list.toOwnedSlice(allocator);

    }

    fn searchInContentWithScope(self: *Explorer, path: []const u8, content: []const u8, query: []const u8, allocator: std.mem.Allocator, max_results: usize, result_list: *std.ArrayList(ScopedSearchResult)) !void {
        var line_num: u32 = 0;
        var lines = std.mem.splitScalar(u8, content, '\n');
        while (lines.next()) |line| {
            line_num += 1;
            if (indexOfCaseInsensitive(line, query) != null) {
                const line_text = try allocator.dupe(u8, line);
                errdefer allocator.free(line_text);
                const path_copy = try allocator.dupe(u8, path);
                errdefer allocator.free(path_copy);

                const scope = self.findEnclosingSymbolLocked(path, line_num);
                const scope_name = if (scope) |s| try allocator.dupe(u8, s.name) else null;
                errdefer if (scope_name) |n| allocator.free(n);

                try result_list.append(allocator, .{
                    .path = path_copy,
                    .line_num = line_num,
                    .line_text = line_text,
                    .scope_name = scope_name,
                    .scope_kind = if (scope) |s| s.kind else null,
                    .scope_start = if (scope) |s| s.line_start else 0,
                    .scope_end = if (scope) |s| s.line_end else 0,
                });
                if (result_list.items.len >= max_results) return;
            }
        }
    }
};

/// Extract lines from content string as a range [start..end] (1-indexed, inclusive).
/// When line_numbers is true, prepends "{d:>5} | " prefix. When compact is true,
/// skips comment/blank lines based on language.
pub fn extractLines(content: []const u8, start: u32, end: u32, line_numbers: bool, compact: bool, language: Language, allocator: std.mem.Allocator) ![]u8 {
    var buf: std.ArrayList(u8) = .{};
    errdefer buf.deinit(allocator);
    const w = buf.writer(allocator);

    var line_num: u32 = 0;
    var lines = std.mem.splitScalar(u8, content, '\n');
    while (lines.next()) |line| {
        line_num += 1;
        if (line_num < start) continue;
        if (line_num > end) break;
        if (compact and isCommentOrBlank(line, language)) continue;
        if (line_numbers) {
            try w.print("{d:>5} | {s}\n", .{ line_num, line });
        } else {
            try w.print("{s}\n", .{line});
        }
    }
    return buf.toOwnedSlice(allocator);
}

/// Returns true if a line is blank or a single-line comment for the given language.
pub fn isCommentOrBlank(line: []const u8, language: Language) bool {
    const trimmed = std.mem.trim(u8, line, " \t");
    if (trimmed.len == 0) return true;
    return switch (language) {
        .zig, .rust, .go_lang => std.mem.startsWith(u8, trimmed, "//"),
        .python => std.mem.startsWith(u8, trimmed, "#"),
        .javascript, .typescript, .c, .cpp => std.mem.startsWith(u8, trimmed, "//") or std.mem.startsWith(u8, trimmed, "/*") or std.mem.startsWith(u8, trimmed, "*"),
        else => false,
    };
}

fn searchInContent(path: []const u8, content: []const u8, query: []const u8, allocator: std.mem.Allocator, max_results: usize, result_list: *std.ArrayList(SearchResult)) !void {
    var line_num: u32 = 0;
    var lines = std.mem.splitScalar(u8, content, '\n');
    while (lines.next()) |line| {
        line_num += 1;
        if (indexOfCaseInsensitive(line, query) != null) {
            const line_text = try allocator.dupe(u8, line);
            errdefer allocator.free(line_text);
            const path_copy = try allocator.dupe(u8, path);
            errdefer allocator.free(path_copy);
            try result_list.append(allocator, .{
                .path = path_copy,
                .line_num = line_num,
                .line_text = line_text,
            });
            if (result_list.items.len >= max_results) return;
        }
    }
}

fn searchInContentRegex(path: []const u8, content: []const u8, pattern: []const u8, allocator: std.mem.Allocator, max_results: usize, result_list: *std.ArrayList(SearchResult)) !void {
    var line_num: u32 = 0;
    var lines = std.mem.splitScalar(u8, content, '\n');
    while (lines.next()) |line| {
        line_num += 1;
        if (regexMatch(line, pattern)) {
            const line_text = try allocator.dupe(u8, line);
            errdefer allocator.free(line_text);
            const path_copy = try allocator.dupe(u8, path);
            errdefer allocator.free(path_copy);
            try result_list.append(allocator, .{
                .path = path_copy,
                .line_num = line_num,
                .line_text = line_text,
            });
            if (result_list.items.len >= max_results) return;
        }
    }
}

/// Simple regex matcher — supports: . \s \w \d \S \W \D [chars] [^chars]
/// * + ? ^ $ | () and escaped literals.
/// Uses backtracking. Searches for a match anywhere in the string (unanchored).
pub fn regexMatch(haystack: []const u8, pattern: []const u8) bool {
    // Iterate through top-level | separators to prevent stack overflow with
    // many alternation branches.  No recursion; no fixed-size buffer needed.
    var prev: usize = 0;
    var i: usize = 0;
    var depth: usize = 0;
    var in_bracket = false;
    while (i < pattern.len) {
        const c = pattern[i];
        if (c == '\\' and i + 1 < pattern.len) { i += 2; continue; }
        if (c == '[') { in_bracket = true; i += 1; continue; }
        if (c == ']') { in_bracket = false; i += 1; continue; }
        if (in_bracket) { i += 1; continue; }
        if (c == '(') { depth += 1; i += 1; continue; }
        if (c == ')') { if (depth > 0) depth -= 1; i += 1; continue; }
        if (c == '|' and depth == 0) {
            if (regexMatchSingle(haystack, pattern[prev..i])) return true;
            prev = i + 1;
        }
        i += 1;
    }
    return regexMatchSingle(haystack, pattern[prev..]);
}

fn regexMatchSingle(haystack: []const u8, pattern: []const u8) bool {
    if (pattern.len > 0 and pattern[0] == '^') {
        return matchHere(haystack, pattern[1..], 0);
    }
    // Try match at every position (unanchored search)
    for (0..haystack.len + 1) |start| {
        if (matchHere(haystack, pattern, start)) return true;
    }
    return false;
}

fn matchHere(haystack: []const u8, pattern: []const u8, pos: usize) bool {
    var p: usize = 0;
    var h: usize = pos;

    while (p < pattern.len) {
        // End anchor
        if (pattern[p] == '$' and p + 1 == pattern.len) {
            return h == haystack.len;
        }

        // Alternation handled at top level in regexMatch
        if (pattern[p] == '|') return false;

        // Grouping with parens — handle alternation inside groups
        if (pattern[p] == '(') {
            // Find matching closing paren
            var depth: usize = 1;
            var end = p + 1;
            while (end < pattern.len and depth > 0) {
                if (pattern[end] == '\\' and end + 1 < pattern.len) {
                    end += 2;
                    continue;
                }
                if (pattern[end] == '(') depth += 1;
                if (pattern[end] == ')') depth -= 1;
                if (depth > 0) end += 1;
            }
            // end now points at ')' (or pattern.len if unmatched)
            const group_end = if (end < pattern.len) end else pattern.len;
            const group_content = pattern[p + 1 .. group_end];
            const after_group = if (group_end + 1 <= pattern.len) pattern[group_end + 1 ..] else "";

            // Split group content on top-level | within this group
            var branch_start: usize = 0;
            var d: usize = 0;
            var i: usize = 0;
            while (i < group_content.len) {
                if (group_content[i] == '\\' and i + 1 < group_content.len) {
                    i += 2;
                    continue;
                }
                if (group_content[i] == '(') d += 1;
                if (group_content[i] == ')') { if (d > 0) d -= 1; }
                if (group_content[i] == '|' and d == 0) {
                    // Try this branch
                    if (matchGroupBranch(haystack, group_content[branch_start..i], after_group, h)) return true;
                    branch_start = i + 1;
                }
                i += 1;
            }
            // Try last branch
            return matchGroupBranch(haystack, group_content[branch_start..], after_group, h);
        }

        if (pattern[p] == ')') {
            p += 1;
            continue;
        }

        // Check for quantifier following current element
        const elem_end = elementEnd(pattern, p);
        if (elem_end < pattern.len) {
            const qc = pattern[elem_end];
            if (qc == '*') {
                return matchQuantified(haystack, pattern, p, elem_end, elem_end + 1, 0, h);
            }
            if (qc == '+') {
                return matchQuantified(haystack, pattern, p, elem_end, elem_end + 1, 1, h);
            }
            if (qc == '?') {
                // Try with one match
                if (h < haystack.len and matchElement(haystack[h], pattern, p, elem_end)) {
                    if (matchHere(haystack, pattern[elem_end + 1 ..], h + 1)) return true;
                }
                // Try without
                return matchHere(haystack, pattern[elem_end + 1 ..], h);
            }
            if (qc == '{') {
                // Parse {n}, {n,}, {n,m}
                var qi = elem_end + 1;
                var min_rep: usize = 0;
                while (qi < pattern.len and pattern[qi] >= '0' and pattern[qi] <= '9') {
                    min_rep = min_rep * 10 + (pattern[qi] - '0');
                    qi += 1;
                }
                var max_rep: usize = min_rep; // default {n} = exactly n
                if (qi < pattern.len and pattern[qi] == ',') {
                    qi += 1;
                    if (qi < pattern.len and pattern[qi] >= '0' and pattern[qi] <= '9') {
                        max_rep = 0;
                        while (qi < pattern.len and pattern[qi] >= '0' and pattern[qi] <= '9') {
                            max_rep = max_rep * 10 + (pattern[qi] - '0');
                            qi += 1;
                        }
                    } else {
                        max_rep = 256; // {n,} = at least n, cap at 256
                    }
                }
                if (qi < pattern.len and pattern[qi] == '}') {
                    qi += 1; // skip '}'
                    return matchQuantifiedRange(haystack, pattern, p, elem_end, qi, min_rep, max_rep, h);
                }
                // Malformed {…} — treat as literal
            }
        }

        // No quantifier — must match exactly one char
        if (h >= haystack.len) return false;
        if (!matchElement(haystack[h], pattern, p, elem_end)) return false;
        h += 1;
        p = elem_end;
    }

    return true; // pattern exhausted — match
}

/// Try matching a group branch followed by the rest of the pattern.
fn matchGroupBranch(haystack: []const u8, branch: []const u8, after: []const u8, pos: usize) bool {
    // Concatenate branch + after conceptually by matching branch first,
    // then continuing with after at the new position.
    // matchHere on branch tells us how far it consumes.
    // We need to try every possible consumption length of the branch.
    return matchBranchThenRest(haystack, branch, after, pos);
}

fn matchBranchThenRest(haystack: []const u8, branch: []const u8, rest: []const u8, pos: usize) bool {
    // If branch is empty, just try matching the rest
    if (branch.len == 0) return matchHere(haystack, rest, pos);

    // We need to find how many chars the branch consumes, then match rest.
    // Build a temporary combined pattern: branch + rest
    // This is safe because both are slices of the same original pattern string,
    // but they may not be adjacent. Use a simple approach: match branch, track position.
    var buf: [4096]u8 = undefined;
    if (branch.len + rest.len > buf.len) return false;
    @memcpy(buf[0..branch.len], branch);
    @memcpy(buf[branch.len..branch.len + rest.len], rest);
    return matchHere(haystack, buf[0..branch.len + rest.len], pos);
}

/// Match a quantified element (greedy).
fn matchQuantified(haystack: []const u8, pattern: []const u8, elem_start: usize, elem_end: usize, rest_start: usize, min_count: usize, start_pos: usize) bool {
    // Count max matches
    var count: usize = 0;
    var h = start_pos;
    while (h < haystack.len and matchElement(haystack[h], pattern, elem_start, elem_end)) {
        count += 1;
        h += 1;
    }
    // Greedy: try from max matches down to min
    var c: usize = count + 1;
    while (c > min_count) {
        c -= 1;
        if (matchHere(haystack, pattern[rest_start..], start_pos + c)) return true;
    }
    return false;
}

/// Match a {n,m} quantified element (greedy).
fn matchQuantifiedRange(haystack: []const u8, pattern: []const u8, elem_start: usize, elem_end: usize, rest_start: usize, min_count: usize, max_count: usize, start_pos: usize) bool {
    // Count max matches up to max_count
    var count: usize = 0;
    var h = start_pos;
    while (h < haystack.len and count < max_count and matchElement(haystack[h], pattern, elem_start, elem_end)) {
        count += 1;
        h += 1;
    }
    if (count < min_count) return false;
    // Greedy: try from max matches down to min
    var c: usize = count + 1;
    while (c > min_count) {
        c -= 1;
        if (matchHere(haystack, pattern[rest_start..], start_pos + c)) return true;
    }
    return false;
}

/// Return the index past the current element in the pattern.
fn elementEnd(pattern: []const u8, p: usize) usize {
    if (p >= pattern.len) return p;
    if (pattern[p] == '\\' and p + 1 < pattern.len) return p + 2;
    if (pattern[p] == '[') {
        var i = p + 1;
        if (i < pattern.len and pattern[i] == '^') i += 1;
        if (i < pattern.len and pattern[i] == ']') i += 1;
        while (i < pattern.len and pattern[i] != ']') : (i += 1) {}
        if (i < pattern.len) i += 1;
        return i;
    }
    if (pattern[p] == '.') return p + 1;
    return p + 1;
}

/// Match a single character against a pattern element.
fn matchElement(c: u8, pattern: []const u8, start: usize, end: usize) bool {
    if (start >= end) return false;

    // Dot matches any char
    if (pattern[start] == '.' and end == start + 1) return true;

    // Escape sequences
    if (pattern[start] == '\\' and end == start + 2) {
        return switch (pattern[start + 1]) {
            'd' => std.ascii.isDigit(c),
            'D' => !std.ascii.isDigit(c),
            'w' => std.ascii.isAlphanumeric(c) or c == '_',
            'W' => !(std.ascii.isAlphanumeric(c) or c == '_'),
            's' => c == ' ' or c == '\t' or c == '\n' or c == '\r',
            'S' => !(c == ' ' or c == '\t' or c == '\n' or c == '\r'),
            'b', 'B' => false, // word boundary — not a char match
            else => c == pattern[start + 1],
        };
    }

    // Character class [...]
    if (pattern[start] == '[') {
        var i = start + 1;
        var negate = false;
        if (i < end and pattern[i] == '^') {
            negate = true;
            i += 1;
        }
        var matched = false;
        // Handle literal ] at start of class (e.g. []] or [^]])
        if (i < end and pattern[i] == ']') {
            if (c == ']') matched = true;
            i += 1;
        }
        while (i < end and pattern[i] != ']') {
            // Range: a-z, but only if '-' is not at end of class
            if (i + 2 < end and pattern[i + 1] == '-' and pattern[i + 2] != ']') {
                if (c >= pattern[i] and c <= pattern[i + 2]) matched = true;
                i += 3;
            } else {
                if (c == pattern[i]) matched = true;
                i += 1;
            }
        }
        return if (negate) !matched else matched;
    }

    // Literal
    return c == pattern[start];
}


fn indexOfCaseInsensitive(haystack: []const u8, needle: []const u8) ?usize {
    if (needle.len == 0) return 0;
    if (needle.len > haystack.len) return null;
    for (0..haystack.len - needle.len + 1) |i| {
        var match = true;
        for (0..needle.len) |j| {
            const hc = if (haystack[i + j] >= 'A' and haystack[i + j] <= 'Z') haystack[i + j] + 32 else haystack[i + j];
            const nc = if (needle[j] >= 'A' and needle[j] <= 'Z') needle[j] + 32 else needle[j];
            if (hc != nc) {
                match = false;
                break;
            }
        }
        if (match) return i;
    }
    return null;
}

fn startsWith(haystack: []const u8, needle: []const u8) bool {
    return std.mem.startsWith(u8, haystack, needle);
}

fn extractIdent(s: []const u8) ?[]const u8 {
    var end: usize = 0;
    for (s) |ch| {
        if (std.ascii.isAlphanumeric(ch) or ch == '_') {
            end += 1;
        } else break;
    }
    return if (end > 0) s[0..end] else null;
}

fn extractStringLiteral(s: []const u8) ?[]const u8 {
    const quote_chars = [_]u8{ '"', '\'' };
    for (quote_chars) |q| {
        if (std.mem.indexOfScalar(u8, s, q)) |start_pos| {
            if (std.mem.indexOfScalarPos(u8, s, start_pos + 1, q)) |end_pos| {
                return s[start_pos + 1 .. end_pos];
            }
        }
    }
    return null;
}

fn containsAny(s: []const u8, needles: []const []const u8) bool {
    for (needles) |needle| {
        if (std.mem.indexOf(u8, s, needle) != null) return true;
    }
    return false;
}

fn skipKeywords(s: []const u8) []const u8 {
    const keywords = [_][]const u8{ "export ", "async ", "function ", "const ", "let ", "var " };
    var result = s;
    for (keywords) |kw| {
        if (std.mem.startsWith(u8, result, kw)) {
            result = result[kw.len..];
        }
    }
    return result;
}
