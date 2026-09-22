//! Help text, one block per P1 command. Pinned byte for byte by
//! `tests/golden/help/*.txt` (see `cli/help_test.zig` and the gate).
const std = @import("std");
const Command = @import("args.zig").Command;
const bin = @import("../edition.zig").id.binary_name;

pub const version = "0.1.0-p1";

const globals =
    \\Global options:
    \\  -m, --model <MODEL>        Model id or alias (fable, opus, local, ...) [env: ABBEY_MODEL]
    \\      --mode <MODE>          Execution mode [possible values: ask, plan]
    \\      --plan                 Plan mode shorthand
    \\      --ask                  Ask mode shorthand
    \\  -p, --print                Headless print (capture, then emit)
    \\      --output-format <FMT>  Output format with --print
    \\  -f, --force                Force / always-approve [aliases: --yolo, --always-approve]
    \\      --add-dir <DIR>        Extra workspace root (repeatable)
    \\      --sandbox <MODE>       Sandbox: enabled|disabled
    \\      --debug                Forward --debug to cursor-style backends
    \\      --approve-mcps         Forward --approve-mcps to cursor-style backends
    \\      --max-turns <N>        Forward --max-turns to cursor-style backends
    \\  -h, --help                 Print help
    \\  -V, --version              Print version
    \\
;

pub const top = "Abbey (Zig): agent CLI over subprocess executors (P1 CLI core)\n\n" ++
    "Usage: " ++ bin ++ " [OPTIONS] <COMMAND> [ARGS]...\n\n" ++
    \\Commands:
    \\  ask      Read-only Q&A through the canonical hybrid path (route-logged)
    \\  print    Headless single-shot capture; no route record [aliases: p, e, exec]
    \\  commit   Conventional commit message for the staged diff (headless capture)
    \\  doctor   Paths, backend resolution, model, chat, memory [aliases: which, info]
    \\  learn    Self-learn: status|correction|train|preference|routes|digest|export|review|stats|improve
    \\  routes   Recent hybrid routing records
    \\  config   Show config (`--init` scaffolds config.toml if absent)
    \\  claims   Capability ledger: Current / Partial / Proposed / Out of scope [aliases: roadmap, scope]
    \\  memory   Memory store: search <query> | similar <query>
    \\  wdbx     Bridge to the `abi wdbx` CLI (query gets --json and Abbey's store)
    \\  edition  Compiled edition identity and namespaces
    \\  version  Print version
    \\  help     Print this help or the help of a command
    \\
    \\
++ globals ++
    \\
    \\The TUI, daemon, and MCP server are later phases (see `claims`).
    \\
;

fn cmd(comptime usage: []const u8, comptime about: []const u8, comptime body: []const u8) []const u8 {
    return about ++ "\n\nUsage: " ++ bin ++ " " ++ usage ++ "\n" ++ body ++ "\n" ++ globals;
}

pub fn forCommand(c: Command) []const u8 {
    return switch (c) {
        .ask => cmd("ask [OPTIONS] [PROMPT]...", "Read-only Q&A (Claude ask): Gemma role, ask mode, through the canonical hybrid path",
            \\
            \\Every ask appends one record to <state>/route.jsonl and one activity memory.
            \\
        ),
        .print => cmd("print [OPTIONS] [PROMPT]...", "Headless single-shot (Claude -p, Codex exec, Grok --single)",
            \\
            \\A headless bypass: resumes only the live backend's chat and writes no
            \\route record and no activity memory.
            \\
        ),
        .commit => cmd("commit [OPTIONS]", "Conventional commit message for the staged diff (Claude /commit)",
            \\
            \\Reads `git diff --cached` (at most 3000 lines) and captures one headless
            \\run. Nothing is committed; the message is printed.
            \\
        ),
        .doctor => cmd("doctor [OPTIONS]", "Paths, backend resolution, model, chat, memory (Codex doctor)", ""),
        .learn => cmd("learn [OPTIONS] [ARGS]...", "Self-learn: correction|train|preference|routes|digest|export|review|stats|improve",
            \\
            \\Curation only: LoRA / fine-tune is Proposed and `learn lora` exits 2.
            \\
        ),
        .routes => cmd("routes [OPTIONS] [N]", "Recent hybrid routing records (default 10)", ""),
        .config => cmd("config [OPTIONS] [--init]", "Show config; --init writes an annotated config.toml when none exists", ""),
        .claims => cmd("claims [OPTIONS] [--markdown]", "Capability ledger: Current / Partial / Proposed / Out of scope", ""),
        .memory => cmd("memory [OPTIONS] <search|similar> <QUERY>...", "Memory store: keyword search or lexical feature-hash similarity",
            \\
            \\Similarity is a deterministic n-gram feature hash, not a learned embedding.
            \\
        ),
        .wdbx => cmd("wdbx [OPTIONS] [ARGS]...", "Bridge to `abi wdbx` (query gets --json and Abbey's store base path)",
            \\
            \\`abi` paths are BASE paths: Abbey's <state>/wdbx/ directory is
            \\<state>/wdbx/wdbx to `abi`.
            \\
        ),
        .edition => cmd("edition [OPTIONS]", "Compiled edition identity and state/config namespaces", ""),
        .version => cmd("version", "Print version", ""),
        .help => top,
    };
}
