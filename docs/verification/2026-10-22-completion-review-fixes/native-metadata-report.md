# Native metadata fixture implementation

Owned changes only: tools/tests/metadata_probe_fixture.c (new), tools/tests/smoke_tui_metadata_pty.py, tools/tests/test_metadata_fixture.py. Read canonical AGENTS.md and both requested investigation/fix reports. Preserved the prior batched cancel submission, ordinary-control setup policy, quit sequence, exact probe/live parent and leaf/group assertions, queued-marker observation, Esc/join130/discard, closure before cleanup, and no-generation assertion. No shipped Rust, dependencies, libtest, production deadlines, retries, Cargo, shared target, real profiles, services, agents, commits, or pushes were used.

MetadataFixture now compiles a test-only POSIX C helper into scratch/bin/abi in setup before TUI launch. CC must be an absolute path, defaults to /usr/bin/cc, and missing compiler or unsuccessful compilation fails hard. Setup compiler execution is bounded; a scratch native-compiler-proof.json retains command, compiler version and source SHA-256. No compiler runs inside the version probe.

Exact --version/version runs native code only. Startup control returns the original immediate version. Metadata control reads a bounded line sidecar, emits rename-published bounded JSON with real PID/PPID/PGID/parent PGID and resolved selected path, nonce, and actual argv. JSON strings escape quotes, backslashes and controls. Held metadata forks a real leaf; SIGTERM handlers only set sig_atomic_t flags. The leaf inherits the already installed handler, writes its own real identity event, and sends a readiness byte; parent waits at most one second for readiness. Both hold loops have a 20 second ceiling and release checks. Parent terminates/waits the leaf outside handlers for normal release, hold expiry and cancellation, then records terminated and exits130 on cancellation. Forked leaf keeps the actual original argv, which its event reports rather than inventing a private command line.

All non-version requests exec the retained generic Python ABI stub with explicit interpreter and script paths, -S, and unchanged original arguments. The generic script lives at generation/abi so its actual backend name remains abi. A positive complete invocation records the original queued marker and extra arguments, ensuring the no-generation-contact test cannot pass merely because native selection drops generation calls.

## RED/GREEN evidence

Command: PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/tests -p test_metadata_fixture.py -v

Before implementation, the poison-interpreter test patched sys.executable during fixture construction and invoked the exact selected --version binary. Old shebang execution wrote the poison marker and failed the real owner/leaf readiness requirement. Full output: native-metadata-red.log.

After implementation all nine focused tests pass. Full final output: native-metadata-green.log. Poison remains unused; real owner and leaf have matching group and actual parentage, owner returns130, leaf is reaped and group closes before release. Tests additionally cover actual generation forwarding, unchanged startup version, compiler failure/missing compiler hard failures, bounded sidecar marker rejection, ambient Python site-hook isolation, and all prior ordering/setup tests.

Native PTY control/cancel/quit and independent review remain controller-owned. Earlier controller trials had three passing cohorts then a missing-event fourth trial; its cause remains unattributed. This removes Python startup from the exact metadata version path, but does not establish Python caused that prior failure or immunity to arbitrary OS scheduler suspension beyond the real 500 ms budget.
