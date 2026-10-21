import { useEffect, useState } from "react";
import type { BundleIdentity, ConnectionInfo, EffectiveModel, RuntimeStatus } from "../ipc/generated";
import { appEffectiveModel } from "../ipc/client";
import {
  ALL_APP_CAPABILITIES,
  DESKTOP_READ_CAPABILITIES,
} from "../ipc/generated.samples";
import { PERSONAL_EDITION, SAFE_EDITION } from "../ipc/generated.editions";
import { UpdateCheck } from "../components/UpdateCheck";

interface Props {
  status: RuntimeStatus;
  connection: ConnectionInfo;
  identity: BundleIdentity | null;
  /**
   * The route the Chat surface has selected, if any.
   *
   * The effective model is "the model a turn would use right now", and a
   * turn runs on the *selected* route. Without this the Doctor would report
   * the runtime's first available route, which is not the route the Chat
   * surface submits turns on once a route has been chosen.
   */
  routeId?: string | null;
  /** Injected for tests; the live view reads the effective model itself. */
  effectiveModel?: EffectiveModel | null;
  effectiveError?: string | null;
}

const CONNECTION_LABEL = {
  daemon: "abbeyd (authenticated Unix socket)",
  in_process: "linked application core (no daemon configured)",
} as const;

const BEARER_LABEL = {
  inline_env: "inline environment variable",
  token_file: "owner-only token file",
  conflicting: "both sources set — fail-closed",
} as const;

export function DoctorView({
  status,
  connection,
  identity,
  routeId = null,
  effectiveModel: injectedModel,
  effectiveError: injectedError,
}: Props) {
  const granted = status.capabilities.capabilities;
  const editionRow = status.edition === "personal" ? PERSONAL_EDITION : SAFE_EDITION;
  const mismatched =
    identity !== null && identity.bundle_id !== identity.configured_bundle_id;

  const [effectiveModel, setEffectiveModel] = useState<EffectiveModel | null>(
    injectedModel ?? null,
  );
  const [effectiveError, setEffectiveError] = useState<string | null>(injectedError ?? null);
  useEffect(() => {
    if (injectedModel !== undefined || injectedError !== undefined) return;
    let cancelled = false;
    // Ask about the *selected* route, so the answer is the model a turn
    // would use right now. With no selection this is the runtime's default
    // available route, which is what a turn uses when none is chosen.
    appEffectiveModel(routeId, null, null)
      .then((value) => {
        if (!cancelled) setEffectiveModel(value);
      })
      .catch((error) => {
        if (!cancelled) setEffectiveError(String(error?.message ?? error));
      });
    return () => {
      cancelled = true;
    };
  }, [injectedModel, injectedError, routeId]);

  return (
    <>
      <h1>Doctor</h1>
      <p className="subtitle">
        Everything below is read from the runtime through{" "}
        <code>app_status</code>, <code>app_connection</code>,{" "}
        <code>app_bundle_identity</code>, and <code>app_effective_model</code>. Nothing
        is cached; the effective model is resolved by Abbey&apos;s published spawn-path
        rules, not guessed.
      </p>

      <h2>Runtime</h2>
      <div className="card">
        <dl className="kv">
          <dt>State</dt>
          <dd>{status.state}</dd>
          <dt>Edition</dt>
          <dd>
            {editionRow.product_name} <span className="muted">({status.edition})</span>
          </dd>
          <dt>Protocol version</dt>
          <dd className="mono">{status.protocol_version}</dd>
          <dt>Schema version</dt>
          <dd className="mono">{status.schema_version}</dd>
        </dl>
      </div>

      <h2>Build stamp</h2>
      <div className="card">
        <dl className="kv">
          <dt>Version</dt>
          <dd className="mono">{status.version}</dd>
          <dt>Git</dt>
          <dd className="mono">{status.build_git}</dd>
          <dt>Target</dt>
          <dd className="mono">{status.build_target}</dd>
        </dl>
      </div>

      <h2>Effective model</h2>
      <div className="card">
        {effectiveError !== null ? (
          <p className="muted">{effectiveError}</p>
        ) : effectiveModel === null ? (
          <p className="muted">Resolving…</p>
        ) : (
          <dl className="kv">
            <dt>Backend</dt>
            <dd className="mono">{effectiveModel.backend}</dd>
            <dt>Resolved</dt>
            <dd className="mono">{effectiveModel.resolved}</dd>
            <dt>Live</dt>
            <dd className="mono">{effectiveModel.raw === "" ? "(unset)" : effectiveModel.raw}</dd>
            <dt>Source</dt>
            <dd className="mono">{effectiveModel.source}</dd>
          </dl>
        )}
        <p className="claim-note">
          The model a turn would use now, taken from the runtime&apos;s own published
          route{routeId ? " — the route the Chat surface has selected" : " — the default available route, because no route is selected"} — and
          resolved with Abbey&apos;s shared spawn-path rules, the same resolution{" "}
          <code>abbey doctor</code> and the TUI doctor panel render, so the surfaces
          cannot disagree. Read-only: it reports; it does not act.
        </p>
      </div>

      <h2>Granted capabilities</h2>
      <div className="card">
        <p className="muted no-top-margin">
          The responding runtime granted {granted.length} capabilities. This
          desktop recognizes {DESKTOP_READ_CAPABILITIES.length} read capabilities across status,
          claims, routes, task events, memory, model inventory, and the tool
          inventory. Each view requires its corresponding runtime grant.
        </p>
        <dl className="kv">
          {ALL_APP_CAPABILITIES.map((capability) => (
            <div key={capability} className="kv-row">
              <dt className="mono">{capability}</dt>
              <dd>{granted.includes(capability) ? "granted" : "not granted"}</dd>
            </div>
          ))}
        </dl>
      </div>

      <h2>Connection</h2>
      <div className="card">
        <dl className="kv">
          <dt>Source</dt>
          <dd>{CONNECTION_LABEL[connection.source]}</dd>
          <dt>Socket path</dt>
          <dd className="mono">{connection.socket_path ?? "—"}</dd>
          <dt>Bearer configured</dt>
          <dd>{connection.bearer_configured ? "yes" : "no"}</dd>
          <dt>Bearer source</dt>
          <dd>
            {connection.bearer_source === null
              ? "—"
              : BEARER_LABEL[connection.bearer_source]}
          </dd>
        </dl>
        <p className="claim-note">{connection.detail}</p>
        <p className="claim-note">
          The bearer value is never sent to this window. Only whether one is
          configured, and from which kind of source, crosses IPC.
        </p>
      </div>

      <h2>Bundle identity</h2>
      <div className={mismatched ? "card error" : "card"}>
        {identity === null ? (
          <p className="muted">Not available.</p>
        ) : (
          <>
            <dl className="kv">
              <dt>Product name</dt>
              <dd>{identity.product_name}</dd>
              <dt>Bundle identifier (edition table)</dt>
              <dd className="mono">{identity.bundle_id}</dd>
              <dt>Bundle identifier (packaged)</dt>
              <dd className="mono">{identity.configured_bundle_id}</dd>
              <dt>CLI binary</dt>
              <dd className="mono">{identity.binary_name}</dd>
              <dt>Daemon binary</dt>
              <dd className="mono">{identity.daemon_binary_name}</dd>
            </dl>
            <p className="claim-note">
              {mismatched
                ? "The packaged identifier does not match the compiled edition."
                : "The safe and personal editions never share a bundle identifier: " +
                  `${SAFE_EDITION.bundle_id} and ${PERSONAL_EDITION.bundle_id} are both ` +
                  "written into the Tauri configs from abbey::edition by desktop/codegen."}
            </p>
          </>
        )}
      </div>

      <UpdateCheck />
    </>
  );
}
