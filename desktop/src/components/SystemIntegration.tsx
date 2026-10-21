// System integration — what this app does with the host operating system.
//
// This is a *transparency* surface, not a control surface. It states four
// facts, and every one of them is a fact about the shipped build:
//
//   1. Login startup — this app registers no login item, launch agent, or
//      startup service, and it must stay unregistered.
//   2. Background service — this app installs and starts no service and
//      spawns no helper process. The daemon is a separate owner-only process.
//   3. OS permissions in use — the exact permissions the shipped capability
//      manifest declares, and what each one can reach.
//   4. Closing the window — does not cancel daemon-owned work.
//
// The permission list is read from the build's own compiled-in manifest
// through `app_system_integration`, so it cannot drift away from what the app
// actually declares. Nothing in this file registers, installs, starts, or
// requests anything: there is no button here, and there is no command behind
// one that could.

import { useEffect, useState } from "react";
import { appSystemIntegration, toIpcError } from "../ipc/client";
import type { IpcError, SystemIntegration } from "../ipc/generated";
import { ErrorCard } from "../views/ErrorCard";

/** What each declared permission can reach, in this build. */
const PERMISSION_DETAIL: Record<string, string> = {
  "core:default":
    "Windows, events, and app metadata for this window. No filesystem, shell, process, " +
    "or opener access.",
  "updater:default":
    "The pinned, signature-verified updater: it can fetch and replace this app bundle from " +
    "the endpoint pinned in the Tauri config, and only after the artifact verifies against " +
    "the pinned Ed25519 key. It is the only permission here with a network or write effect, " +
    "and it only runs from an explicit user action.",
};

/** The label for a permission this build does not explain. */
const UNEXPLAINED_PERMISSION =
  "This build declares a permission this window does not explain. Read " +
  "src-tauri/capabilities/default.json before trusting it.";

export function describePermission(identifier: string): string {
  return PERMISSION_DETAIL[identifier] ?? UNEXPLAINED_PERMISSION;
}

interface Props {
  /** Injected for tests; the live panel reads the facts itself. */
  integration?: SystemIntegration | null;
  error?: IpcError | null;
}

export function SystemIntegrationPanel({ integration: injected, error: injectedError }: Props = {}) {
  const [integration, setIntegration] = useState<SystemIntegration | null>(injected ?? null);
  const [error, setError] = useState<IpcError | null>(injectedError ?? null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (injected !== undefined || injectedError !== undefined) return;
    setLoading(true);
    void appSystemIntegration()
      .then((result) => setIntegration(result))
      .catch((cause: unknown) => setError(toIpcError(cause)))
      .finally(() => setLoading(false));
  }, []);

  return (
    <section className="card settings-section" aria-label="System integration">
      <div>
        <h2>System integration</h2>
        <p className="muted">
          What this app does with your operating system. Everything below is read from
          this build&apos;s own capability manifest — nothing here registers, installs,
          or starts anything.
        </p>
      </div>
      {error !== null && (
        <div>
          <p className="muted">
            The host-integration facts could not be read. Nothing is claimed in their
            place.
          </p>
          <ErrorCard error={error} />
        </div>
      )}
      {error === null && integration === null && (
        <p className="muted" role="status">
          {loading ? "Reading this build's host integration…" : "Host integration unavailable."}
        </p>
      )}
      {error === null && integration !== null && (
        <div>
          <dl className="kv">
            <dt>Login startup</dt>
            <dd>
              {integration.login_startup.state === "not_registered"
                ? "Not registered"
                : integration.login_startup.state}
            </dd>
            <dt>Background service</dt>
            <dd>
              {integration.background_service.state === "separate_process"
                ? "Not installed or started by this app"
                : integration.background_service.state}
            </dd>
            <dt>Closing the window</dt>
            <dd>
              {integration.window_close.state === "daemon_owned"
                ? "Daemon-owned work continues"
                : integration.window_close.state}
            </dd>
          </dl>
          <p className="claim-note">{integration.login_startup.detail}</p>
          <p className="claim-note">{integration.background_service.detail}</p>
          <p className="claim-note">{integration.window_close.detail}</p>

          <h3>OS permissions in use</h3>
          {integration.declared_permissions.length === 0 ? (
            <p className="claim-note">
              This build declares no permissions. Nothing in this window can reach the
              filesystem, a shell, a process, or the OS opener.
            </p>
          ) : (
            <>
              <p className="claim-note">
                These are the exact permissions this build declares:
              </p>
              <ul>
                {integration.declared_permissions.map((permission) => (
                  <li key={permission}>
                    <code>{permission}</code> — {describePermission(permission)}
                  </li>
                ))}
              </ul>
            </>
          )}
          <p className="claim-note">
            {integration.execution_capable_permissions.length === 0
              ? "No declared permission can reach the filesystem, a shell, a process, or " +
                "the OS opener. There is no shell, filesystem, process, or opener plugin in " +
                "this client at all."
              : `These declared permissions could execute: ${integration.execution_capable_permissions.join(", ")}.`}
          </p>
        </div>
      )}
    </section>
  );
}
