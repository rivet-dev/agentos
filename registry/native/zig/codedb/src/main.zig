const std = @import("std");
const Explorer = @import("vendor/explore.zig").Explorer;

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
    "target",
    ".gradle",
    ".idea",
    ".vs",
    "vendor",
    "Pods",
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

const skip_extensions = [_][]const u8{
    ".png",  ".jpg",  ".jpeg", ".gif",  ".bmp",   ".ico",  ".icns", ".webp",
    ".svg",  ".ttf",  ".otf",  ".woff", ".woff2", ".eot",
    ".zip",  ".tar",  ".gz",   ".bz2",  ".xz",    ".7z",   ".rar",
    ".pdf",  ".doc",  ".docx", ".xls",  ".xlsx",  ".pptx",
    ".mp3",  ".mp4",  ".wav",  ".avi",  ".mov",   ".flv",  ".ogg",  ".webm",
    ".exe",  ".dll",  ".so",   ".dylib", ".o",    ".a",    ".lib",
    ".wasm", ".pyc",  ".pyo",  ".class",
    ".db",   ".sqlite", ".sqlite3",
    ".lock", ".sum",
};

fn shouldSkipDir(name: []const u8) bool {
    for (skip_dirs) |skip| {
        if (std.mem.eql(u8, name, skip)) return true;
    }
    return false;
}

fn shouldSkipFile(path: []const u8) bool {
    for (skip_extensions) |ext| {
        if (std.mem.endsWith(u8, path, ext)) return true;
    }
    return false;
}

fn indexFile(explorer: *Explorer, full_path: []const u8, rel_path: []const u8, allocator: std.mem.Allocator) !void {
    if (shouldSkipFile(rel_path)) return;

    const file = std.fs.cwd().openFile(full_path, .{ .mode = .read_only }) catch |err| {
        std.debug.print("openFile failed {s}: {s}\n", .{ rel_path, @errorName(err) });
        return err;
    };
    defer file.close();

    const stat = file.stat() catch |err| {
        std.debug.print("stat failed {s}: {s}\n", .{ rel_path, @errorName(err) });
        return err;
    };
    if (stat.size > 512 * 1024) return;

    const content = file.readToEndAlloc(allocator, 512 * 1024) catch |err| {
        std.debug.print("read failed {s}: {s}\n", .{ rel_path, @errorName(err) });
        return err;
    };
    defer allocator.free(content);

    const check_len = @min(content.len, 512);
    for (content[0..check_len]) |c| {
        if (c == 0) return;
    }

    try explorer.indexFile(rel_path, content);
}

fn scanDir(explorer: *Explorer, dir_path: []const u8, prefix: []const u8, allocator: std.mem.Allocator) !void {
    var dir = try std.fs.cwd().openDir(dir_path, .{ .iterate = true });
    defer dir.close();

    var iter = dir.iterate();
    while (try iter.next()) |entry| {
        switch (entry.kind) {
            .directory => {
                if (shouldSkipDir(entry.name)) continue;

                const next_prefix = if (prefix.len == 0)
                    try allocator.dupe(u8, entry.name)
                else
                    try std.fmt.allocPrint(allocator, "{s}/{s}", .{ prefix, entry.name });
                defer allocator.free(next_prefix);

                const next_dir_path = try std.fmt.allocPrint(allocator, "{s}/{s}", .{ dir_path, entry.name });
                defer allocator.free(next_dir_path);

                try scanDir(explorer, next_dir_path, next_prefix, allocator);
            },
            .file => {
                const rel_path = if (prefix.len == 0)
                    try allocator.dupe(u8, entry.name)
                else
                    try std.fmt.allocPrint(allocator, "{s}/{s}", .{ prefix, entry.name });
                defer allocator.free(rel_path);

                const full_path = try std.fmt.allocPrint(allocator, "{s}/{s}", .{ dir_path, entry.name });
                defer allocator.free(full_path);

                try indexFile(explorer, full_path, rel_path, allocator);
            },
            else => {},
        }
    }
}

fn scanProject(explorer: *Explorer, root: []const u8, allocator: std.mem.Allocator) !void {
    var dir = std.fs.cwd().openDir(root, .{ .iterate = true }) catch |err| {
        std.debug.print("openDir failed {s}: {s}\n", .{ root, @errorName(err) });
        return err;
    };
    dir.close();
    try scanDir(explorer, root, "", allocator);
}

fn usage() !void {
    const stderr = std.fs.File.stderr();
    try stderr.writeAll(
        \\usage:
        \\  codedb tree <root>
        \\  codedb outline <root> <path>
        \\  codedb find <root> <symbol>
        \\  codedb search <root> <query> [max]
        \\  codedb word <root> <word>
        \\  codedb deps <root> <path>
        \\  codedb read <root> <path>
        \\
    );
}

fn parseMax(arg: []const u8) usize {
    return std.fmt.parseInt(usize, arg, 10) catch 20;
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    if (args.len < 3) {
        try usage();
        std.process.exit(1);
    }

    const cmd = args[1];
    const root = args[2];

    var explorer = Explorer.init(allocator);
    defer explorer.deinit();

    try scanProject(&explorer, root, allocator);

    if (std.mem.eql(u8, cmd, "tree")) {
        const tree = try explorer.getTree(allocator, false);
        defer allocator.free(tree);
        try std.fs.File.stdout().writeAll(tree);
        return;
    }

    if (std.mem.eql(u8, cmd, "outline")) {
        if (args.len < 4) {
            try usage();
            std.process.exit(1);
        }
        var outline = (try explorer.getOutline(args[3], allocator)) orelse {
            std.process.exit(2);
        };
        defer outline.deinit();

        var stdout_writer = std.fs.File.stdout().writer(&.{});
        const stdout = &stdout_writer.interface;
        try stdout.print("{s} {s} {d}L {d} sym\n", .{
            outline.path,
            @tagName(outline.language),
            outline.line_count,
            outline.symbols.items.len,
        });
        for (outline.symbols.items) |sym| {
            try stdout.print("{d}:{d} {s} {s}\n", .{
                sym.line_start,
                sym.line_end,
                @tagName(sym.kind),
                sym.name,
            });
        }
        return;
    }

    if (std.mem.eql(u8, cmd, "find")) {
        if (args.len < 4) {
            try usage();
            std.process.exit(1);
        }
        const hits = try explorer.findAllSymbols(args[3], allocator);
        defer {
            for (hits) |hit| {
                allocator.free(hit.path);
                allocator.free(hit.symbol.name);
                if (hit.symbol.detail) |detail| allocator.free(detail);
            }
            allocator.free(hits);
        }

        var stdout_writer = std.fs.File.stdout().writer(&.{});
        const stdout = &stdout_writer.interface;
        for (hits) |hit| {
            try stdout.print("{s}:{d}:{d} {s} {s}\n", .{
                hit.path,
                hit.symbol.line_start,
                hit.symbol.line_end,
                @tagName(hit.symbol.kind),
                hit.symbol.name,
            });
        }
        return;
    }

    if (std.mem.eql(u8, cmd, "search")) {
        if (args.len < 4) {
            try usage();
            std.process.exit(1);
        }
        const max_results = if (args.len >= 5) parseMax(args[4]) else 20;
        const hits = try explorer.searchContent(args[3], allocator, max_results);
        defer {
            for (hits) |hit| {
                allocator.free(hit.path);
                allocator.free(hit.line_text);
            }
            allocator.free(hits);
        }

        var stdout_writer = std.fs.File.stdout().writer(&.{});
        const stdout = &stdout_writer.interface;
        for (hits) |hit| {
            try stdout.print("{s}:{d}: {s}\n", .{ hit.path, hit.line_num, hit.line_text });
        }
        return;
    }

    if (std.mem.eql(u8, cmd, "word")) {
        if (args.len < 4) {
            try usage();
            std.process.exit(1);
        }
        const hits = try explorer.searchWord(args[3], allocator);
        defer allocator.free(hits);

        var stdout_writer = std.fs.File.stdout().writer(&.{});
        const stdout = &stdout_writer.interface;
        for (hits) |hit| {
            try stdout.print("{s}:{d}\n", .{ hit.path, hit.line_num });
        }
        return;
    }

    if (std.mem.eql(u8, cmd, "deps")) {
        if (args.len < 4) {
            try usage();
            std.process.exit(1);
        }
        const deps = try explorer.getImportedBy(args[3], allocator);
        defer {
            for (deps) |dep| allocator.free(dep);
            allocator.free(deps);
        }

        var stdout_writer = std.fs.File.stdout().writer(&.{});
        const stdout = &stdout_writer.interface;
        for (deps) |dep| {
            try stdout.print("{s}\n", .{dep});
        }
        return;
    }

    if (std.mem.eql(u8, cmd, "read")) {
        if (args.len < 4) {
            try usage();
            std.process.exit(1);
        }
        const content = (try explorer.getContent(args[3], allocator)) orelse {
            std.process.exit(2);
        };
        defer allocator.free(content);
        try std.fs.File.stdout().writeAll(content);
        return;
    }

    try usage();
    std.process.exit(1);
}
