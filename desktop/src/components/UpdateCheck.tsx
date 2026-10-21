// The single updater trigger surface. Deliberately small: the check is always
// user-initiated, every download is verified by the updater plugin against the
// pinned Ed25519 public key in tauri.conf.json before it installs, and nothing
// here can install without an explicit button press. The endpoint and key are
// pinned in Rust-side config, not in this file.
import { useCallback, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";

type Phase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; bytes: number | null }
  | { kind: "installed" }
  | { kind: "failed"; message: string };

export function UpdateCheck() {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });

  const checkForUpdate = useCallback(async () => {
    setPhase({ kind: "checking" });
    try {
      const update = await check();
      if (update === null) {
        setPhase({ kind: "current" });
      } else {
        setPhase({ kind: "available", update });
      }
    } catch (error) {
      setPhase({
        kind: "failed",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, []);

  const install = useCallback(async (update: Update) => {
    setPhase({ kind: "downloading", bytes: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Progress") {
          setPhase((current) =>
            current.kind === "downloading"
              ? { kind: "downloading", bytes: (current.bytes ?? 0) + event.data.chunkLength }
              : current,
          );
        }
      });
      // The new version is on disk; without the process plugin there is no
      // in-app relaunch, so the user finishes by quitting and reopening.
      setPhase({ kind: "installed" });
    } catch (error) {
      setPhase({
        kind: "failed",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, []);

  let status: React.ReactNode;
  switch (phase.kind) {
    case "checking":
      status = "Checking the pinned release endpoint…";
      break;
    case "current":
      status = "Abbey is up to date.";
      break;
    case "available":
      status = `Abbey ${phase.update.version} is available${phase.update.body ? ` — ${phase.update.body}` : ""}`;
      break;
    case "downloading":
      status = `Downloading and verifying${phase.bytes !== null ? ` (${(phase.bytes / 1048576).toFixed(1)} MiB)` : ""}…`;
      break;
    case "installed":
      status = "Installed. Quit and reopen Abbey to finish the update.";
      break;
    case "failed":
      status = `Update check failed: ${phase.message}`;
      break;
    default:
      status = "Not checked since launch.";
  }

  return (
    <>
      <h2>Updates</h2>
      <div className="card">
        <dl className="kv">
          <div className="kv-row">
            <dt>Source</dt>
            <dd>
              <code>plugins.updater.endpoints</code> in <code>tauri.conf.json</code>{" "}
              — downloads are verified against the pinned Ed25519 public key
              before install.
            </dd>
          </div>
          <div className="kv-row">
            <dt>Policy</dt>
            <dd>Checking and installing are always explicit actions here; nothing is automatic.</dd>
          </div>
          <div className="kv-row">
            <dt>Status</dt>
            <dd role="status">{status}</dd>
          </div>
        </dl>
        <div className="update-actions">
          {phase.kind === "available" ? (
            <button
              type="button"
              disabled={false}
              onClick={() => void install(phase.update)}
            >
              Download {phase.update.version} and install
            </button>
          ) : (
            <button
              type="button"
              disabled={phase.kind === "checking" || phase.kind === "downloading"}
              onClick={() => void checkForUpdate()}
            >
              Check for updates
            </button>
          )}
        </div>
      </div>
    </>
  );
}
