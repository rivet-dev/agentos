const std = @import("std");

/// ANSI style constants. Obtain an instance via `style(use_color)`.
pub const Style = struct {
    reset: []const u8,
    bold: []const u8,
    dim: []const u8,
    red: []const u8,
    green: []const u8,
    yellow: []const u8,
    blue: []const u8,
    magenta: []const u8,
    cyan: []const u8,
    bright_green: []const u8,
    orange: []const u8,

    /// ANSI color for a Language @tagName (e.g. "zig", "go_lang", "typescript").
    pub fn langColor(self: Style, lang: []const u8) []const u8 {
        if (std.mem.eql(u8, lang, "zig")) return self.yellow;
        if (std.mem.eql(u8, lang, "typescript")) return self.blue;
        if (std.mem.eql(u8, lang, "javascript")) return self.yellow;
        if (std.mem.eql(u8, lang, "go_lang")) return self.cyan;
        if (std.mem.eql(u8, lang, "rust")) return self.orange;
        if (std.mem.eql(u8, lang, "python")) return self.blue;
        if (std.mem.eql(u8, lang, "c") or std.mem.eql(u8, lang, "cpp")) return self.blue;
        if (std.mem.eql(u8, lang, "markdown")) return self.dim;
        if (std.mem.eql(u8, lang, "json") or std.mem.eql(u8, lang, "yaml")) return self.dim;
        return self.dim; // unknown
    }

    /// ANSI color for a SymbolKind @tagName (e.g. "function", "struct_def").
    pub fn kindColor(self: Style, kind: []const u8) []const u8 {
        if (std.mem.eql(u8, kind, "function")) return self.blue;
        if (std.mem.eql(u8, kind, "method")) return self.blue;
        if (std.mem.eql(u8, kind, "struct_def")) return self.yellow;
        if (std.mem.eql(u8, kind, "enum_def")) return self.yellow;
        if (std.mem.eql(u8, kind, "union_def")) return self.yellow;
        if (std.mem.eql(u8, kind, "trait_def")) return self.magenta;
        if (std.mem.eql(u8, kind, "impl_block")) return self.cyan;
        if (std.mem.eql(u8, kind, "type_alias")) return self.yellow;
        if (std.mem.eql(u8, kind, "macro_def")) return self.orange;
        if (std.mem.eql(u8, kind, "test_decl")) return self.green;
        if (std.mem.eql(u8, kind, "import")) return self.dim;
        if (std.mem.eql(u8, kind, "comment_block")) return self.dim;
        return self.cyan; // constant, variable
    }
};

pub const on = Style{
    .reset = "\x1b[0m",
    .bold = "\x1b[1m",
    .dim = "\x1b[2m",
    .red = "\x1b[31m",
    .green = "\x1b[32m",
    .yellow = "\x1b[33m",
    .blue = "\x1b[34m",
    .magenta = "\x1b[35m",
    .cyan = "\x1b[36m",
    .bright_green = "\x1b[92m",
    .orange = "\x1b[38;5;208m",
};

pub const off = Style{
    .reset = "",
    .bold = "",
    .dim = "",
    .red = "",
    .green = "",
    .yellow = "",
    .blue = "",
    .magenta = "",
    .cyan = "",
    .bright_green = "",
    .orange = "",
};

/// Return `on` or `off` depending on `color`.
pub fn style(color: bool) Style {
    return if (color) on else off;
}

/// Format nanoseconds as a human-readable duration.
/// Fast ops (<10ms) get an ⚡ prefix.
pub fn formatDuration(buf: []u8, ns: i128) []const u8 {
    const abs_ns: u128 = if (ns < 0) @intCast(-ns) else @intCast(ns);
    if (abs_ns < 1_000) {
        return std.fmt.bufPrint(buf, "\xe2\x9a\xa1 {d}ns", .{abs_ns}) catch "";
    } else if (abs_ns < 1_000_000) {
        const us = @as(f64, @floatFromInt(abs_ns)) / 1_000.0;
        return std.fmt.bufPrint(buf, "\xe2\x9a\xa1 {d:.1}\xc2\xb5s", .{us}) catch "";
    } else if (abs_ns < 10_000_000) {
        const ms = @as(f64, @floatFromInt(abs_ns)) / 1_000_000.0;
        return std.fmt.bufPrint(buf, "\xe2\x9a\xa1 {d:.1}ms", .{ms}) catch "";
    } else if (abs_ns < 1_000_000_000) {
        const ms = @as(f64, @floatFromInt(abs_ns)) / 1_000_000.0;
        return std.fmt.bufPrint(buf, "{d:.1}ms", .{ms}) catch "";
    } else {
        const sc = @as(f64, @floatFromInt(abs_ns)) / 1_000_000_000.0;
        return std.fmt.bufPrint(buf, "{d:.1}s", .{sc}) catch "";
    }
}

/// Return a color scaled to elapsed duration.
pub fn durationColor(s: Style, ns: i128) []const u8 {
    const abs_ns: u128 = if (ns < 0) @intCast(-ns) else @intCast(ns);
    if (abs_ns < 10_000_000) return s.cyan;    // <10ms  → cyan ⚡
    if (abs_ns < 100_000_000) return s.green;  // <100ms → green
    if (abs_ns < 1_000_000_000) return s.blue; // <1s    → blue
    return s.yellow;                            // 1s+    → yellow
}
