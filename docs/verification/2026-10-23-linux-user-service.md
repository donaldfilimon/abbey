# Linux user-service bootstrap source qualification

This slice adds an explicit Linux-only controller for already-installed edition-matched CLI/daemon binaries. Install creates private credentials and an exact user unit without enabling or starting it. Start/status use the existing authenticated CLI status; stop/restart/uninstall retain credentials and runtime data. No desktop discovery or login startup is implemented.

Focused tests first exposed missing implementation, reload failure publication, eager CLI state initialization, and current protocol-v2 status rejection. The resulting controller restores prior unit bytes/mode or removes a newly published unit on reload failure; reports rollback failures; confines identity probes to temporary HOME/XDG and both edition config/state roots; and accepts only exact integer protocol/schema pairs (1,2) and (2,3), alongside exact ready edition. It refuses unsafe paths, unrelated units, permissive existing state and unsupported platforms rather than silently adopting them.

All 21 focused Python tests pass. Independent scoped reviews passed spec compliance and quality after each correction. Earlier fixture subprocess timeouts are retained as failures with unestablished cause; explicit current-Python -S fixture shebangs replace an indirect Apple launcher. Production three-second/64-KiB subprocess limits remain unchanged.

A macOS scratch Unix-daemon check using exact recorded CLI/daemon hashes passed authenticated current-v2 readiness, wrong-bearer refusal, offline refusal and credential/state retention on uninstall. The service manager was a no-effect fixture; this proves the controller/status contract, not systemd or Ubuntu behavior. Initial scratch checks found the eager identity defect and version mismatch. A separate long macOS temporary path exceeded SUN_LEN; a shorter isolated /tmp path corrected that harness precondition. Failed attempts remain retained.

Root/desktop gates are pending for this integrated slice. Source/mock/macOS evidence cannot close native Ubuntu ARM64 service, native CI or desktop discovery acceptance. Windows named pipes/Job Objects and all three-platform milestone proof remain open. Logs, reviews, scoped diffs, actual status and bounded native-controller proof are in the adjacent directory.

The integrated platform-tooling snapshot subsequently passed root (546.36 s)
then desktop (25.88 s) serially, and all four additional WDBX/personal/accelerator
feature combinations (96.81/98.63/96.59/100.15 s), with no source hash/mode drift.
Exact snapshot and receipts are retained under platform-tooling in the completion
review-fixes receipt. This acceptance applies to that recorded source, not later
in-progress credential-reader/tooling changes, and supplies no native Linux or
Windows execution. Root retained upstream WDBX and claims timeout PIPE warnings;
the latter has a separately reviewed fix for the next source snapshot.
