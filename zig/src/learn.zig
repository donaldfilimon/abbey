//! Self-learning from corrections, preferences, and routes into memory
//! layers, plus train_candidate curation (port of Rust `learn.rs`). This is
//! curation only: LoRA / fine-tuning is Proposed and refused.
const std = @import("std");
const Ctx = @import("ctx.zig").Ctx;
const mem = @import("memory/store.zig");
const Record = mem.Record;
const newrec = @import("memory/new.zig");
const rec_json = @import("memory/record.zig");
const route_log = @import("route_log.zig");
const fsx = @import("util/fsx.zig");
const bin = @import("edition.zig").id.binary_name;

pub const Error = mem.Error || route_log.ReadError || std.Io.Writer.Error || error{Usage};

pub const Env = struct {
    ctx: Ctx,
    arena: std.mem.Allocator,
    state_dir: []const u8,
    cwd: []const u8,

    fn store(self: Env) error{OutOfMemory}!mem.Store {
        return mem.open(self.ctx.gpa, self.ctx.io, self.arena, self.state_dir);
    }
};

pub fn captureCorrection(e: Env, summary: []const u8, detail: []const u8, as_train: bool) Error![]const u8 {
    const s = try e.store();
    var r = try newrec.stm(e.ctx, e.arena, summary, detail);
    r.origin = "user";
    r.source_type = "correction";
    r.retention = if (as_train) "train_candidate" else "ltm";
    r.tags = if (as_train) &.{ "ltm", "correction", "self-learn", "train_candidate" } else &.{ "ltm", "correction", "self-learn" };
    r.confidence = 0.95;
    r.provenance = try std.fmt.allocPrint(e.arena, "user correction @ {s}", .{e.cwd});
    try s.store(e.arena, r);
    return r.id;
}

pub fn learnPreference(e: Env, preference: []const u8) Error![]const u8 {
    const s = try e.store();
    var end: usize = 0;
    var chars: usize = 0;
    while (end < preference.len and chars < 80) : (chars += 1) {
        end += @min(std.unicode.utf8ByteSequenceLength(preference[end]) catch 1, preference.len - end);
    }
    var r = try newrec.stm(e.ctx, e.arena, try std.fmt.allocPrint(e.arena, "preference: {s}", .{preference[0..end]}), preference);
    r.origin = "user";
    r.source_type = "preference";
    r.retention = "ltm";
    r.tags = &.{ "ltm", "preference", "self-learn" };
    r.confidence = 0.99;
    r.provenance = "abbey learn preference";
    try s.store(e.arena, r);
    return r.id;
}

fn routeActivityPayload(arena: std.mem.Allocator, r: *const route_log.Record) error{OutOfMemory}![]const u8 {
    return std.fmt.allocPrint(arena, "{s}\nreason={s}\nconfidence={d:.2}\nalternate={s}\nfallback={s}", .{
        r.cwd, r.reason, r.confidence, r.alternate orelse "-", r.fallback orelse "-",
    });
}

/// Promote the recent route tail into activity. Idempotent: provenance
/// `route.jsonl @ <ts>` is stable per route.
pub fn learnFromRoutes(e: Env, n: usize) Error!usize {
    const s = try e.store();
    const routes = try route_log.recent(e.arena, e.ctx.io, e.state_dir, n);
    var already: std.StringHashMapUnmanaged(void) = .empty;
    for (try s.filterWith(e.arena, .{ .source_type = "route" }, 10_000)) |r| try already.put(e.arena, r.provenance, {});
    var stored: usize = 0;
    for (routes) |*r| {
        const provenance = try std.fmt.allocPrint(e.arena, "route.jsonl @ {s}", .{r.ts});
        if (already.contains(provenance)) continue;
        var m = try newrec.stm(e.ctx, e.arena, try std.fmt.allocPrint(e.arena, "route {s}/{s} \u{2192} {s}", .{ r.persona, r.role, r.model }), try routeActivityPayload(e.arena, r));
        m.source_type = "route";
        m.retention = "activity";
        m.tags = try e.arena.dupe([]const u8, &.{ "activity", "self-learn", r.role });
        m.confidence = r.confidence;
        m.provenance = provenance;
        try s.store(e.arena, m);
        try already.put(e.arena, provenance, {});
        stored += 1;
    }
    return stored;
}

pub fn digest(e: Env) Error![]const u8 {
    const s = try e.store();
    const report = try s.reflect(e.arena);
    var promoted: usize = 0;
    for (try s.filter(e.arena, "activity", "self-learn", 100)) |r| {
        if (r.confidence >= 0.8 and !contains(report.low_confidence, r.id)) {
            s.promote(e.arena, r.id, "ltm") catch {};
            promoted += 1;
        }
    }
    return std.fmt.allocPrint(e.arena, "digest: promoted={d} low_confidence={d} dups={d} superseded={d}", .{ promoted, report.low_confidence.len, report.duplicate_summaries.len, report.superseded.len });
}

fn contains(list: []const []const u8, id: []const u8) bool {
    for (list) |x| if (std.mem.eql(u8, x, id)) return true;
    return false;
}

pub const Curation = struct { total: usize, with_provenance: usize, high_confidence: usize, ready: usize };

pub fn curation(e: Env) Error!Curation {
    const s = try e.store();
    var c: Curation = .{ .total = 0, .with_provenance = 0, .high_confidence = 0, .ready = 0 };
    for (try s.filter(e.arena, "train_candidate", null, 10_000)) |r| {
        c.total += 1;
        const prov = std.mem.trim(u8, r.provenance, &std.ascii.whitespace).len != 0;
        if (prov) c.with_provenance += 1;
        if (r.confidence >= 0.9) c.high_confidence += 1;
        if (prov and r.confidence >= 0.9) c.ready += 1;
    }
    return c;
}

fn storeExists(e: Env) error{OutOfMemory}!bool {
    return fsx.exists(e.ctx.io, try mem.pathFor(e.arena, e.state_dir));
}

pub fn printUsage(w: *std.Io.Writer) std.Io.Writer.Error!void {
    try w.print(
        \\usage:
        \\   {0s} learn                    # status + train_candidate summary
        \\   {0s} learn review [n]         # list candidates (provenance gate)
        \\   {0s} learn stats              # curation counts
        \\   {0s} learn train <text>       # add train_candidate with provenance
        \\   {0s} learn correction <text>  # LTM correction
        \\   {0s} learn preference <text>  # LTM standing directive
        \\   {0s} learn routes [n]         # route.jsonl -> activity
        \\   {0s} learn improve [n] [--apply] # propose; apply additive steps only
        \\   {0s} learn digest|export ...
        \\note:  LoRA / fine-tune is Proposed but unavailable; curation only
        \\
    , .{bin});
}

/// Read-only: never creates the store.
pub fn status(e: Env) Error!void {
    const w = e.ctx.out;
    try w.print("{s} learn: self-learn + train_candidate curation (not a LoRA runner)\n\n", .{bin});
    try w.print("store: {s}\n", .{try mem.pathFor(e.arena, e.state_dir)});
    if (!try storeExists(e)) {
        try w.writeAll("(empty: capture corrections / run learn routes)\n\n");
        return printUsage(w);
    }
    const s = try e.store();
    for ([_][]const u8{ "stm", "ltm", "activity", "train_candidate" }) |layer| {
        try w.print("  {s:<16} self-learn={d}\n", .{ layer, (try s.filter(e.arena, layer, "self-learn", 500)).len });
    }
    const rep = try s.reflect(e.arena);
    try w.print("reflect: low={d} dups={d} superseded={d}\n", .{ rep.low_confidence.len, rep.duplicate_summaries.len, rep.superseded.len });
    const c = try curation(e);
    try w.print("train_candidate: total={d} prov_ok={d} missing={d} high_conf={d} ready={d}\n\n", .{ c.total, c.with_provenance, c.total - c.with_provenance, c.high_confidence, c.ready });
    try w.print("curate:  {0s} learn review . {0s} learn stats . {0s} learn export\nrefuse:  {0s} learn lora\n", .{bin});
}

pub fn review(e: Env, limit: usize) Error!void {
    const w = e.ctx.out;
    try w.print("{s} learn review: train_candidate curation (no weight updates)\n\n", .{bin});
    if (!try storeExists(e)) return w.print("(no store yet: `{s} learn train <text>` or `{s} learn correction ...`)\n", .{ bin, bin });
    const rows = try (try e.store()).filter(e.arena, "train_candidate", null, limit);
    if (rows.len == 0) return w.print("(no train_candidate records: `{s} learn train <text>`)\n", .{bin});
    var missing: usize = 0;
    var ready: usize = 0;
    for (rows) |r| {
        const prov = std.mem.trim(u8, r.provenance, &std.ascii.whitespace).len != 0;
        if (!prov) missing += 1;
        const is_ready = prov and r.confidence >= 0.9;
        if (is_ready) ready += 1;
        var end: usize = 0;
        var chars: usize = 0;
        while (end < r.payload.len and chars < 120) : (chars += 1) end += @min(std.unicode.utf8ByteSequenceLength(r.payload[end]) catch 1, r.payload.len - end);
        try w.print("{s}\tconf={d:.2}\tprov={s}\tready={s}\t{s}\n", .{ r.id, r.confidence, if (prov) "ok" else "MISSING", if (is_ready) "yes" else "no", r.payload[0..end] });
    }
    try w.print("\nreview: {d} candidate(s); {d} missing provenance; {d} ready (prov+conf>=0.9)\nnext:   {s} learn stats . {s} learn export train_candidate\nproposed: LoRA pipeline unavailable\n", .{ rows.len, missing, ready, bin, bin });
}

pub fn stats(e: Env) Error!void {
    const w = e.ctx.out;
    try w.print("{s} learn stats: train_candidate curation counts\n\n", .{bin});
    if (!try storeExists(e)) {
        try w.writeAll("train_candidate: total=0 (no store)\n");
        return w.print("note: export via `{s} learn export train_candidate`; LoRA is Proposed, not implemented\n", .{bin});
    }
    const c = try curation(e);
    try w.print("train_candidate: total={d}\n  with_provenance={d}\n  missing_provenance={d}\n  high_confidence(>=0.9)={d}\n  curation_ready(prov+conf>=0.9)={d}\n", .{ c.total, c.with_provenance, c.total - c.with_provenance, c.high_confidence, c.ready });
    try w.print("\nnext:  {s} learn review . {s} learn export train_candidate\nproposed: LoRA / fine-tune unavailable (`{s} learn lora` exits 2)\n", .{ bin, bin, bin });
}

pub fn exportLayer(e: Env, layer: []const u8) Error!void {
    for (try (try e.store()).filter(e.arena, layer, null, 10_000)) |r| {
        try rec_json.writeJson(e.ctx.out, &r);
        try e.ctx.out.writeByte('\n');
    }
}

/// Standing preferences injected into prompts (empty when none).
pub fn preferenceContext(e: Env, limit: usize) error{OutOfMemory}![]const u8 {
    if (!(storeExists(e) catch false)) return "";
    const s = try e.store();
    const prefs = s.filter(e.arena, "ltm", "preference", limit) catch return "";
    if (prefs.len == 0) return "";
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(e.arena, "Standing user preferences (from Abbey self-learn LTM):\n");
    for (prefs) |p| {
        try out.appendSlice(e.arena, "- ");
        try out.appendSlice(e.arena, p.payload);
        try out.append(e.arena, '\n');
    }
    return out.items;
}

pub const improve = @import("learn_improve.zig");

fn parseN(s: ?[]const u8, default: usize) usize {
    const v = s orelse return default;
    return std.fmt.parseInt(usize, v, 10) catch default;
}

/// `learn` dispatcher. Returns the process exit code.
pub fn dispatch(e: Env, args: []const []const u8) Error!u8 {
    const w = e.ctx.out;
    if (args.len == 0) {
        try status(e);
        return 0;
    }
    const sub = args[0];
    const rest = args[1..];
    const text = try std.mem.join(e.arena, " ", rest);
    const eq = struct {
        fn f(a: []const u8, names: []const []const u8) bool {
            for (names) |n| if (std.mem.eql(u8, a, n)) return true;
            return false;
        }
    }.f;
    if (eq(sub, &.{ "status", "show" })) {
        try status(e);
    } else if (eq(sub, &.{ "help", "-h", "--help" })) {
        try printUsage(w);
    } else if (eq(sub, &.{ "correction", "fix", "train" })) {
        const train = std.mem.eql(u8, sub, "train");
        if (text.len == 0) {
            try e.ctx.err.print("{s}\n", .{if (train) "usage: abbey learn train <curated example with provenance>" else "usage: abbey learn correction <what was wrong / preferred behavior>"});
            return error.Usage;
        }
        try w.print("{s}\n", .{try captureCorrection(e, if (train) "train candidate" else "user correction", text, train)});
    } else if (eq(sub, &.{ "preference", "pref" })) {
        if (text.len == 0) {
            try e.ctx.err.writeAll("usage: abbey learn preference <standing directive>\n");
            return error.Usage;
        }
        try w.print("{s}\n", .{try learnPreference(e, text)});
    } else if (eq(sub, &.{"routes"})) {
        const n = try learnFromRoutes(e, parseN(if (rest.len > 0) rest[0] else null, 20));
        try w.print("learned {d} route records into activity\n", .{n});
    } else if (eq(sub, &.{"digest"})) {
        try w.print("{s}\n", .{try digest(e)});
    } else if (eq(sub, &.{"export"})) {
        try exportLayer(e, if (rest.len > 0) rest[0] else "train_candidate");
    } else if (eq(sub, &.{"review"})) {
        try review(e, parseN(if (rest.len > 0) rest[0] else null, 50));
    } else if (eq(sub, &.{"stats"})) {
        try stats(e);
    } else if (eq(sub, &.{"improve"})) {
        var apply = false;
        var n: usize = 20;
        var found_n = false;
        for (rest) |a| {
            if (std.mem.eql(u8, a, "--apply")) apply = true else if (!found_n) {
                if (std.fmt.parseInt(usize, a, 10)) |v| {
                    n = v;
                    found_n = true;
                } else |_| {}
            }
        }
        _ = try improve.run(e, n, apply);
    } else if (eq(sub, &.{ "lora", "finetune", "fine-tune", "fine_tune" })) {
        try e.ctx.err.writeAll("refused: LoRA / fine-tune training is Proposed and not implemented (curation only; see `claims`)\n");
        return 2;
    } else {
        try e.ctx.err.print("unknown learn subcommand `{s}`\nusage: {s} learn [status|correction|train|preference|routes|digest|export|review|stats|improve]\n(LoRA/fine-tune is Proposed)\n", .{ sub, bin });
        return error.Usage;
    }
    return 0;
}
