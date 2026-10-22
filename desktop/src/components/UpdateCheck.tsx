// The single updater trigger surface. Deliberately small: the check is always
// user-initiated, every download is verified by the updater plugin against the
// pinned Ed25519 public key in tauri.conf.json before it installs, and nothing
// here can install without an explicit button press. The endpoint and key are
// pinned in Rust-side config, not in this file.
import { useCallback, useState, type ReactNode } from "react";
import {
  check as pluginCheck,
  type DownloadEvent,
} from "@tauri-apps/plugin-updater";

/**
 * The part of the updater plugin's `Update` this surface uses.
 *
 * Declared here so the surface can be rendered against a stand-in update —
 * the plugin's own class carries a live backend resource that a render test
 * has no business holding.
 */
export interface UpdateCandidate {
  readonly version: string;
  readonly body?: string;
  downloadAndInstall(onEvent?: (event: DownloadEvent) => void): Promise<void>;
}

/** How this surface asks the plugin whether a newer signed build exists. */
export type UpdateChecker = () => Promise<UpdateCandidate | null>;

export type Phase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: UpdateCandidate }
  | { kind: "downloading"; bytes: number | null }
  | { kind: "installed" }
  | { kind: "failed"; message: string };

/**
 * The status line and the one control that can start work.
 *
 * Split out from the container so each phase can be rendered on its own:
 * the surface is the only place a check may be triggered, so what it shows
 * in every phase is the property worth proving.
 */
export function UpdateStatus({
  phase,
  onCheck,
  onInstall,
}: {
  phase: Phase;
  onCheck: () => void;
  onInstall: (update: UpdateCandidate) => void;
}) {
  let status: ReactNode;
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
              onClick={() => onInstall(phase.update)}
            >
              Download {phase.update.version} and install
            </button>
          ) : (
            <button
              type="button"
              disabled={phase.kind === "checking" || phase.kind === "downloading"}
              onClick={onCheck}
            >
              Check for updates
            </button>
          )}
        </div>
      </div>
    </>
  );
}

interface Props {
  /**
   * Injected for tests; the live surface calls the updater plugin itself.
   *
   * The seam exists so the trigger can be rendered without a plugin
   * backend — it does not add a second way to update: the button is still
   * the only thing that calls this.
   */
  check?: UpdateChecker;
}

export function UpdateCheck({ check: injectedCheck }: Props = {}) {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const check = injectedCheck ?? pluginCheck;

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
  }, [check]);

  const install = useCallback(async (update: UpdateCandidate) => {
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

  return (
    <UpdateStatus
      phase={phase}
      onCheck={() => void checkForUpdate()}
      onInstall={(update) => void install(update)}
    />
  );
}
