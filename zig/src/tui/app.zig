//! TUI state machine (port of Rust `tui/app.rs` + `tui/keys.rs` + `tabs.rs`).
//!
//! Pure: keys arrive as decoded `Key` values, never bytes, and everything
//! that touches disk or resolves a binary goes through the injected `Host`.
//! The executor backend lives in the caller-owned `AgentConfig` that
//! `agent` points at: Ctrl-B mutates that live value, and the event loop
//! hands the same pointer to `actions.runAgent`. Nothing here re-reads
//! ABBEY_BACKEND or re-runs backend selection.
const std = @import("std");
const Backend = @import("../agent/backend.zig").Backend;
const AgentConfig = @import("../agent/argv.zig").AgentConfig;
const theme = @import("theme.zig");

pub const Tab = enum {
    home,
    chats,
    personas,
    memory,
    skills,
    models,
    doctor,

    pub const all = [_]Tab{ .home, .chats, .personas, .memory, .skills, .models, .doctor };

    pub fn title(self: Tab) []const u8 {
        return switch (self) {
            .home => "Home",
            .chats => "Chats",
            .personas => "Personas",
            .memory => "Memory",
            .skills => "Skills",
            .models => "Models",
            .doctor => "Doctor",
        };
    }

    pub fn index(self: Tab) usize {
        return @backingInt(self);
    }

    pub fn next(self: Tab) Tab {
        return all[(self.index() + 1) % all.len];
    }

    pub fn prev(self: Tab) Tab {
        return all[(self.index() + all.len - 1) % all.len];
    }
};

pub const Focus = enum {
    prompt,
    panel,

    pub fn toggle(self: Focus) Focus {
        return if (self == .prompt) .panel else .prompt;
    }

    pub fn label(self: Focus) []const u8 {
        return @tagName(self);
    }
};

/// The Rust SlashSuggest overlay depends on the slash catalog and command
/// prediction, both Proposed here, so only these two overlays exist.
pub const Overlay = enum { none, palette, help };

pub const Pending = enum { none, run_resume, run_fresh };

pub const Key = union(enum) {
    /// Printable codepoint without Ctrl.
    char: u21,
    /// Ctrl plus a lowercase ASCII letter.
    ctrl: u8,
    enter,
    esc,
    backspace,
    delete,
    tab,
    backtab,
    up,
    down,
    left,
    right,
    home,
    end,
    page_up,
    page_down,
    f1,
};

pub const Panel = enum { doctor, personas, memory, skills, models };

/// Owned list of display lines; `reset` drops the previous generation.
pub const Lines = struct {
    arena: std.heap.ArenaAllocator,
    items: []const []const u8 = &.{},

    pub fn init(gpa: std.mem.Allocator) Lines {
        return .{ .arena = .init(gpa) };
    }

    pub fn deinit(self: *Lines) void {
        self.arena.deinit();
    }

    /// Start a new generation; returns the allocator its strings live in.
    pub fn reset(self: *Lines) std.mem.Allocator {
        _ = self.arena.reset(.retain_capacity);
        self.items = &.{};
        return self.arena.allocator();
    }
};

/// Everything the state machine needs from the outside world.
pub const Host = struct {
    ud: ?*anyopaque,
    /// Executor binary for `backend`, or null when it does not resolve.
    /// The slice must outlive the App.
    resolve: *const fn (?*anyopaque, Backend) ?[]const u8,
    /// Refill one panel for the LIVE `app.agent`. Doctor also refills
    /// `app.history` and `app.routes`.
    refresh: *const fn (?*anyopaque, *App, Panel) void,
    /// Persist `id` as the active chat; false on failure.
    save_chat: *const fn (?*anyopaque, *App, []const u8) bool,
    /// Resolve and persist a model id; the resolved id (outliving the App)
    /// or null on failure.
    select_model: *const fn (?*anyopaque, *App, []const u8) ?[]const u8,
    save_theme: *const fn (?*anyopaque, theme.Id) void,
};

pub const palette = [_]struct { id: []const u8, label: []const u8, detail: []const u8, action: PaletteAction }{
    .{ .id = "new", .label = "New chat", .detail = "Start a fresh agent session", .action = .new_chat },
    .{ .id = "refresh", .label = "Refresh", .detail = "Reload doctor / personas / memory", .action = .refresh },
    .{ .id = "theme", .label = "Cycle theme", .detail = "ink -> violet -> mono", .action = .cycle_theme },
    .{ .id = "backend", .label = "Cycle backend", .detail = "ollama -> grok -> fm -> abi -> claude -> cursor (skips unresolvable)", .action = .cycle_backend },
    .{ .id = "doctor", .label = "Open Doctor", .detail = "Diagnostics panel", .action = .goto_doctor },
    .{ .id = "quit", .label = "Quit", .detail = "Leave the TUI", .action = .quit },
};

pub const PaletteAction = enum { new_chat, refresh, cycle_theme, cycle_backend, goto_doctor, quit };

pub const help_lines = [_][]const u8{
    "Abbey TUI - keys",
    "",
    "  ` / Ctrl-L     toggle focus  prompt <-> panel",
    "  Tab / S-Tab    next / previous tab",
    "  1-7            jump to tab (panel focus)",
    "  Ctrl-K         command palette",
    "  Ctrl-T         cycle theme (ink / violet / mono)",
    "  Ctrl-B         cycle backend (ollama / grok / fm / abi / claude / cursor)",
    "  F1 / ?         help (empty prompt)",
    "  /              filter lists (panel focus)",
    "  Up/Down        history (prompt) . move (panel)",
    "  Enter          run / select",
    "  Ctrl-N         new chat",
    "  Ctrl-R         refresh",
    "  Esc            close overlay . clear . quit",
    "  Ctrl-Q/C       quit",
    "",
    "Slash commands, prediction, please-fix: Proposed (see claims).",
};

/// Rust `models::alias_table()` rows, the Models tab fallback.
pub const alias_rows = [_][2][]const u8{
    .{ "auto", "auto" },
    .{ "fable", "claude-fable-5-thinking-high" },
    .{ "fable-xhigh", "claude-fable-5-thinking-xhigh" },
    .{ "opus", "claude-opus-5-thinking-high" },
    .{ "opus-fast", "claude-opus-5-thinking-high-fast" },
    .{ "opus48", "claude-opus-4-8-thinking-high" },
    .{ "gpt", "gpt-5.2" },
    .{ "gpt55", "gpt-5.5-high" },
    .{ "sol", "gpt-5.6-sol-high" },
    .{ "terra", "gpt-5.6-terra-medium" },
    .{ "codex", "gpt-5.3-codex" },
};

pub const App = struct {
    gpa: std.mem.Allocator,
    host: Host,
    /// The live executor configuration, owned by the caller.
    agent: *AgentConfig,
    state_dir: []const u8,
    /// Working directory shown on Home (display only).
    cwd: []const u8 = "",
    /// HOME for `~` shortening (display only).
    home: ?[]const u8 = null,
    /// Active chat id as seen by the LIVE backend; refreshed with doctor
    /// (the slice lives in `doctor`'s arena).
    chat: ?[]const u8 = null,
    /// Long-lived strings this App creates (transcript dirs).
    long: std.heap.ArenaAllocator,

    tab: Tab = .home,
    focus: Focus = .prompt,
    theme_id: theme.Id = .ink,
    overlay: Overlay = .none,
    pending: Pending = .none,
    should_quit: bool = false,
    last_code: ?u8 = null,
    tick: u64 = 0,

    input: std.ArrayList(u8) = .empty,
    cursor: usize = 0,
    input_history: std.ArrayList([]u8) = .empty,
    history_idx: ?usize = null,
    filter: std.ArrayList(u8) = .empty,
    filtering: bool = false,
    overlay_query: std.ArrayList(u8) = .empty,
    overlay_idx: usize = 0,
    list_idx: usize = 0,
    scroll: usize = 0,

    status_buf: [256]u8 = undefined,
    status_len: usize = 0,

    doctor: Lines,
    personas: Lines,
    memory: Lines,
    skills: Lines,
    /// Static per-backend model listing; empty means the alias fallback.
    models: Lines,
    /// `ts  chat  cwd` rows from history.log, newest last.
    history: Lines,
    /// Compact route-audit tail for the Home pane.
    routes: Lines,
    alias: Lines,

    pub fn init(gpa: std.mem.Allocator, host: Host, agent: *AgentConfig, state_dir: []const u8, theme_id: theme.Id) error{OutOfMemory}!App {
        var app: App = .{
            .gpa = gpa,
            .host = host,
            .agent = agent,
            .state_dir = state_dir,
            .long = .init(gpa),
            .theme_id = theme_id,
            .doctor = .init(gpa),
            .personas = .init(gpa),
            .memory = .init(gpa),
            .skills = .init(gpa),
            .models = .init(gpa),
            .history = .init(gpa),
            .routes = .init(gpa),
            .alias = .init(gpa),
        };
        errdefer app.deinit();
        const a = app.alias.reset();
        const rows = try a.alloc([]const u8, alias_rows.len);
        for (alias_rows, rows) |r, *out| out.* = try std.fmt.allocPrint(a, "{s: <12} {s}", .{ r[0], r[1] });
        app.alias.items = rows;
        app.setStatus("Enter run . ` focus . Ctrl-K palette . Ctrl-B backend . ? help", .{});
        return app;
    }

    /// First refresh of every panel (Rust `App::new`).
    pub fn load(self: *App) void {
        for ([_]Panel{ .doctor, .personas, .memory, .skills, .models }) |p| self.host.refresh(self.host.ud, self, p);
    }

    pub fn deinit(self: *App) void {
        for (self.input_history.items) |h| self.gpa.free(h);
        self.input_history.deinit(self.gpa);
        self.input.deinit(self.gpa);
        self.filter.deinit(self.gpa);
        self.overlay_query.deinit(self.gpa);
        inline for (.{ "doctor", "personas", "memory", "skills", "models", "history", "routes", "alias" }) |f| @field(self, f).deinit();
        self.long.deinit();
    }

    pub fn status(self: *const App) []const u8 {
        return self.status_buf[0..self.status_len];
    }

    pub fn setStatus(self: *App, comptime fmt: []const u8, args: anytype) void {
        const s = std.fmt.bufPrint(&self.status_buf, fmt, args) catch blk: {
            // Truncated: keep what fit, on a UTF-8 boundary.
            var n: usize = self.status_buf.len;
            while (n > 0 and (self.status_buf[n - 1] & 0xC0) == 0x80) n -= 1;
            if (n > 0 and self.status_buf[n - 1] >= 0xC0) n -= 1;
            break :blk self.status_buf[0..n];
        };
        self.status_len = s.len;
    }

    fn refresh(self: *App, p: Panel) void {
        self.host.refresh(self.host.ud, self, p);
    }

    pub fn refreshAll(self: *App) void {
        for ([_]Panel{ .doctor, .personas, .memory, .skills }) |p| self.refresh(p);
        self.setStatus("refreshed", .{});
    }

    pub fn cycleTheme(self: *App) void {
        self.theme_id = self.theme_id.cycle();
        self.host.save_theme(self.host.ud, self.theme_id);
        self.setStatus("theme -> {s}", .{self.theme_id.asStr()});
    }

    /// Switch to the next backend whose binary resolves (Rust
    /// `cycle_backend`). Unresolvable backends are skipped with no state
    /// change. The executor path and transcript directory follow the new
    /// backend; the model carries over unchanged, as in Rust.
    pub fn cycleBackend(self: *App) void {
        var next = self.agent.backend;
        // Six backends means at most five alternatives before wrapping.
        for (0..5) |_| {
            next = next.cycleNext();
            const path = self.host.resolve(self.host.ud, next) orelse continue;
            const tdir = std.fs.path.join(self.long.allocator(), &.{ self.state_dir, next.transcriptSubdir() }) catch {
                self.setStatus("backend: out of memory", .{});
                return;
            };
            self.agent.backend = next;
            self.agent.agent_path = path;
            // Transcripts are per-backend; without this an abi turn would
            // land in the directory chosen for the startup backend.
            self.agent.transcript_dir = tdir;
            self.refresh(.models);
            self.refresh(.doctor);
            self.setStatus("backend -> {s}", .{next.label()});
            return;
        }
        self.setStatus("backend: no other executor resolvable on this host", .{});
    }

    // ---- lists ----

    pub fn rawLines(self: *const App) []const []const u8 {
        return switch (self.tab) {
            .home, .chats => self.history.items,
            .personas => self.personas.items,
            .memory => self.memory.items,
            .skills => self.skills.items,
            .models => if (self.models.items.len == 0) self.alias.items else self.models.items,
            .doctor => self.doctor.items,
        };
    }

    fn filterApplies(self: *const App) bool {
        const f = std.mem.trim(u8, self.filter.items, " \t");
        return f.len != 0 and switch (self.tab) {
            .home, .chats, .models, .skills => true,
            else => false,
        };
    }

    pub fn lineMatches(self: *const App, line: []const u8) bool {
        if (!self.filterApplies()) return true;
        return std.ascii.findIgnoreCase(line, std.mem.trim(u8, self.filter.items, " \t")) != null; // std: lib/std/ascii.zig
    }

    pub fn listLen(self: *const App) usize {
        var n: usize = 0;
        for (self.rawLines()) |l| {
            if (self.lineMatches(l)) n += 1;
        }
        return n;
    }

    /// The i-th line after filtering.
    pub fn filteredAt(self: *const App, i: usize) ?[]const u8 {
        var n: usize = 0;
        for (self.rawLines()) |l| if (self.lineMatches(l)) {
            if (n == i) return l;
            n += 1;
        };
        return null;
    }

    pub fn ensureVisible(self: *App, viewport: usize) void {
        const len = self.listLen();
        if (len == 0) {
            self.list_idx = 0;
            self.scroll = 0;
            return;
        }
        if (self.list_idx >= len) self.list_idx = len - 1;
        if (self.list_idx < self.scroll) {
            self.scroll = self.list_idx;
        } else if (viewport > 0 and self.list_idx >= self.scroll + viewport) {
            self.scroll = self.list_idx + 1 - viewport;
        }
    }

    // ---- editor ----

    fn prevBoundary(self: *const App) usize {
        var i = self.cursor;
        while (i > 0) {
            i -= 1;
            if ((self.input.items[i] & 0xC0) != 0x80) break;
        }
        return i;
    }

    fn nextBoundary(self: *const App) usize {
        var i = self.cursor;
        if (i >= self.input.items.len) return i;
        i += 1;
        while (i < self.input.items.len and (self.input.items[i] & 0xC0) == 0x80) i += 1;
        return i;
    }

    fn insertChar(self: *App, cp: u21) void {
        var buf: [4]u8 = undefined;
        const n = std.unicode.utf8Encode(cp, &buf) catch return; // std: lib/std/unicode.zig
        self.input.insertSlice(self.gpa, self.cursor, buf[0..n]) catch {
            self.setStatus("input: out of memory", .{});
            return;
        };
        self.cursor += n;
    }

    fn backspace(self: *App) void {
        if (self.cursor == 0) return;
        const start = self.prevBoundary();
        self.input.replaceRangeAssumeCapacity(start, self.cursor - start, &.{});
        self.cursor = start;
    }

    fn delete(self: *App) void {
        if (self.cursor >= self.input.items.len) return;
        const end = self.nextBoundary();
        self.input.replaceRangeAssumeCapacity(self.cursor, end - self.cursor, &.{});
    }

    fn setInput(self: *App, text: []const u8) void {
        self.input.clearRetainingCapacity();
        self.input.appendSlice(self.gpa, text) catch {};
        self.cursor = self.input.items.len;
    }

    pub fn clearInput(self: *App) void {
        self.input.clearRetainingCapacity();
        self.cursor = 0;
    }

    fn pushHistory(self: *App) void {
        const t = std.mem.trim(u8, self.input.items, &std.ascii.whitespace);
        self.history_idx = null;
        if (t.len == 0) return;
        const items = self.input_history.items;
        if (items.len > 0 and std.mem.eql(u8, items[items.len - 1], t)) return;
        const copy = self.gpa.dupe(u8, t) catch return;
        self.input_history.append(self.gpa, copy) catch {
            self.gpa.free(copy);
            return;
        };
        if (self.input_history.items.len > 100) self.gpa.free(self.input_history.orderedRemove(0));
    }

    fn historyUp(self: *App) void {
        const n = self.input_history.items.len;
        if (n == 0) return;
        const next: usize = if (self.history_idx) |i| (if (i == 0) 0 else i - 1) else n - 1;
        self.history_idx = next;
        self.setInput(self.input_history.items[next]);
    }

    fn historyDown(self: *App) void {
        const i = self.history_idx orelse return;
        if (i + 1 >= self.input_history.items.len) {
            self.history_idx = null;
            self.clearInput();
        } else {
            self.history_idx = i + 1;
            self.setInput(self.input_history.items[i + 1]);
        }
    }

    // ---- actions ----

    fn selectListItem(self: *App) void {
        switch (self.tab) {
            .home, .chats => {
                const line = self.filteredAt(self.list_idx) orelse return;
                var it = std.mem.tokenizeAny(u8, line, " \t");
                _ = it.next() orelse return;
                const chat = it.next() orelse return;
                if (self.host.save_chat(self.host.ud, self, chat)) {
                    self.setStatus("active chat -> {s}", .{chat});
                    self.refresh(.doctor);
                } else self.setStatus("save chat failed: {s}", .{chat});
            },
            .models => {
                const line = self.filteredAt(self.list_idx) orelse return;
                var it = std.mem.tokenizeAny(u8, line, " \t");
                const id = it.next() orelse return;
                if (self.host.select_model(self.host.ud, self, id)) |m| {
                    self.agent.model = m;
                    self.setStatus("model -> {s}", .{m});
                    self.refresh(.doctor);
                } else self.setStatus("model: could not save {s}", .{id});
            },
            .personas => {
                self.refresh(.personas);
                self.setStatus("personas refreshed", .{});
            },
            .memory => {
                self.refresh(.memory);
                self.setStatus("memory refreshed", .{});
            },
            .skills => {
                self.refresh(.skills);
                self.setStatus("skills refreshed", .{});
            },
            .doctor => {},
        }
    }

    pub fn enterTab(self: *App, tab: Tab) void {
        self.tab = tab;
        self.list_idx = 0;
        self.scroll = 0;
        self.filter.clearRetainingCapacity();
        self.filtering = false;
        switch (tab) {
            .models => if (self.models.items.len == 0) self.refresh(.models),
            .doctor, .home, .chats => self.refresh(.doctor),
            .personas => self.refresh(.personas),
            .memory => self.refresh(.memory),
            .skills => self.refresh(.skills),
        }
    }

    fn closeOverlay(self: *App) bool {
        if (self.overlay == .none) return false;
        self.overlay = .none;
        self.overlay_query.clearRetainingCapacity();
        self.overlay_idx = 0;
        return true;
    }

    /// Palette rows matching the query (case-insensitive substring over id,
    /// label, detail), as indexes into `palette`.
    pub fn paletteMatches(self: *const App, out: *[palette.len]usize) []usize {
        const q = std.mem.trim(u8, self.overlay_query.items, " \t");
        var n: usize = 0;
        for (palette, 0..) |it, i| {
            if (q.len == 0 or std.ascii.findIgnoreCase(it.id, q) != null or std.ascii.findIgnoreCase(it.label, q) != null or std.ascii.findIgnoreCase(it.detail, q) != null) {
                out[n] = i;
                n += 1;
            }
        }
        return out[0..n];
    }

    fn applyPalette(self: *App, action: PaletteAction) void {
        _ = self.closeOverlay();
        switch (action) {
            .new_chat => self.pending = .run_fresh,
            .refresh => self.refreshAll(),
            .cycle_theme => self.cycleTheme(),
            .cycle_backend => self.cycleBackend(),
            .goto_doctor => self.enterTab(.doctor),
            .quit => self.should_quit = true,
        }
    }

    fn handlePaletteKey(self: *App, key: Key) void {
        var buf: [palette.len]usize = undefined;
        switch (key) {
            .esc => _ = self.closeOverlay(),
            .enter => {
                const m = self.paletteMatches(&buf);
                if (self.overlay_idx < m.len) self.applyPalette(palette[m[self.overlay_idx]].action);
            },
            .down => {
                const n = self.paletteMatches(&buf).len;
                if (n > 0) self.overlay_idx = @min(self.overlay_idx + 1, n - 1);
            },
            .up => self.overlay_idx -|= 1,
            .backspace => {
                _ = self.overlay_query.pop();
                self.overlay_idx = 0;
            },
            .char => |c| {
                if (c == 'j') {
                    const n = self.paletteMatches(&buf).len;
                    if (n > 0) self.overlay_idx = @min(self.overlay_idx + 1, n - 1);
                    return;
                }
                if (c == 'k') {
                    self.overlay_idx -|= 1;
                    return;
                }
                var e: [4]u8 = undefined;
                const n = std.unicode.utf8Encode(c, &e) catch return;
                self.overlay_query.appendSlice(self.gpa, e[0..n]) catch {};
                self.overlay_idx = 0;
            },
            else => {},
        }
    }

    fn proposed(self: *App, what: []const u8) void {
        self.setStatus("{s}: Proposed in abbey-zig (see claims)", .{what});
    }

    fn submitPrompt(self: *App) void {
        const t = std.mem.trim(u8, self.input.items, &std.ascii.whitespace);
        if (std.mem.startsWith(u8, t, "/")) {
            // Slash dispatch is not ported: say so rather than run it as text.
            self.proposed("slash commands");
            return;
        }
        self.pushHistory();
        self.pending = .run_resume;
    }

    fn listDown(self: *App, by: usize) void {
        const len = self.listLen();
        if (len > 0) self.list_idx = @min(self.list_idx + by, len - 1);
    }

    pub fn handleKey(self: *App, key: Key) void {
        if (key == .ctrl and (key.ctrl == 'q' or key.ctrl == 'c')) {
            self.should_quit = true;
            return;
        }
        switch (self.overlay) {
            .palette => return self.handlePaletteKey(key),
            .help => {
                const close = switch (key) {
                    .esc, .enter => true,
                    .char => |c| c == 'q',
                    else => false,
                };
                if (close) _ = self.closeOverlay();
                return;
            },
            .none => {},
        }

        // Global chords.
        switch (key) {
            .ctrl => |c| switch (c) {
                'k' => {
                    self.overlay = .palette;
                    self.overlay_query.clearRetainingCapacity();
                    self.overlay_idx = 0;
                    return;
                },
                't' => return self.cycleTheme(),
                'b' => return self.cycleBackend(),
                'l' => {
                    self.focus = self.focus.toggle();
                    self.setStatus("focus -> {s}", .{self.focus.label()});
                    return;
                },
                else => {},
            },
            .char => |c| if (c == '`') {
                if (self.focus == .panel) {
                    self.focus = .prompt;
                    self.setStatus("focus -> prompt", .{});
                    return;
                }
                if (self.input.items.len == 0) {
                    self.focus = .panel;
                    self.setStatus("focus -> panel", .{});
                    return;
                }
            },
            .f1 => {
                self.overlay = .help;
                return;
            },
            .tab => return self.enterTab(self.tab.next()),
            .backtab => return self.enterTab(self.tab.prev()),
            else => {},
        }

        if (self.filtering and self.focus == .panel) {
            switch (key) {
                .esc => {
                    self.filtering = false;
                    self.filter.clearRetainingCapacity();
                    self.list_idx = 0;
                    return;
                },
                .enter => {
                    self.filtering = false;
                    return;
                },
                .backspace => {
                    _ = self.filter.pop();
                    self.list_idx = 0;
                    return;
                },
                .char => |c| {
                    var e: [4]u8 = undefined;
                    const n = std.unicode.utf8Encode(c, &e) catch return;
                    self.filter.appendSlice(self.gpa, e[0..n]) catch {};
                    self.list_idx = 0;
                    return;
                },
                else => {},
            }
        }

        if (self.focus == .prompt) {
            switch (key) {
                .char => |c| {
                    if (self.input.items.len == 0 and c == 'q') {
                        self.should_quit = true;
                        return;
                    }
                    if (self.input.items.len == 0 and c == '?') {
                        self.overlay = .help;
                        return;
                    }
                    return self.insertChar(c);
                },
                .esc => {
                    if (self.closeOverlay()) return;
                    if (self.input.items.len != 0) self.clearInput() else self.should_quit = true;
                    return;
                },
                .enter => return self.submitPrompt(),
                .up => return self.historyUp(),
                .down => return self.historyDown(),
                .ctrl => |c| switch (c) {
                    'n' => {
                        self.pending = .run_fresh;
                        return;
                    },
                    'p' => return self.proposed("please-fix"),
                    'r' => return self.refreshAll(),
                    else => return,
                },
                .backspace => return self.backspace(),
                .delete => return self.delete(),
                .left => {
                    self.cursor = self.prevBoundary();
                    return;
                },
                .right => {
                    self.cursor = self.nextBoundary();
                    return;
                },
                .home => {
                    self.cursor = 0;
                    return;
                },
                .end => {
                    self.cursor = self.input.items.len;
                    return;
                },
                else => {},
            }
        }

        // Panel focus.
        switch (key) {
            .esc => {
                if (self.closeOverlay()) return;
                if (self.filter.items.len != 0) {
                    self.filter.clearRetainingCapacity();
                    self.filtering = false;
                    self.list_idx = 0;
                } else {
                    self.focus = .prompt;
                    self.setStatus("focus -> prompt", .{});
                }
            },
            .char => |c| switch (c) {
                '1'...'7' => self.enterTab(Tab.all[@intCast(c - '1')]),
                '/' => {
                    self.filtering = true;
                    self.filter.clearRetainingCapacity();
                    self.setStatus("filter...", .{});
                },
                'j' => self.listDown(1),
                'k' => self.list_idx -|= 1,
                'n' => self.pending = .run_fresh,
                'p' => self.proposed("please-fix"),
                'r' => self.refreshAll(),
                'q' => self.should_quit = true,
                else => {},
            },
            .down => self.listDown(1),
            .up => self.list_idx -|= 1,
            .page_down => self.listDown(10),
            .page_up => self.list_idx -|= 10,
            .home => self.list_idx = 0,
            .end => {
                const len = self.listLen();
                if (len > 0) self.list_idx = len - 1;
            },
            .enter => self.selectListItem(),
            else => {},
        }
    }

    /// KPI chips for the Home strip (Rust `kpi_chips`), written into `buf`.
    pub fn kpiChips(self: *const App, buf: *[7][2][]const u8, last_buf: *[4]u8) []const [2][]const u8 {
        const persona = blk: {
            if (self.personas.items.len > 0) {
                var it = std.mem.tokenizeAny(u8, self.personas.items[0], " \t");
                _ = it.next();
                if (it.next()) |w| break :blk w;
            }
            break :blk "abbey";
        };
        const role = blk: {
            for (self.personas.items) |l| if (std.mem.startsWith(u8, l, "default_role:")) {
                break :blk std.mem.trim(u8, l["default_role:".len..], " \t");
            };
            break :blk "auto";
        };
        const mem = if (self.memory.items.len > 0) self.memory.items[0] else "-";
        const last = if (self.last_code) |c| std.fmt.bufPrint(last_buf, "{d}", .{c}) catch "?" else "-";
        buf.* = .{
            .{ "backend", self.agent.backend.label() },
            .{ "model", self.agent.model },
            .{ "chat", if (self.chat) |c| utf8Prefix(c, 8) else "-" },
            .{ "persona", persona },
            .{ "role", role },
            .{ "last", last },
            .{ "mem", utf8Prefix(mem, 18) },
        };
        return buf;
    }
};

/// Longest prefix of `s` of at most `max` bytes ending on a UTF-8 boundary.
pub fn utf8Prefix(s: []const u8, max: usize) []const u8 {
    if (s.len <= max) return s;
    var n = max;
    while (n > 0 and (s[n] & 0xC0) == 0x80) n -= 1;
    return s[0..n];
}
