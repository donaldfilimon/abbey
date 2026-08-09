import type { BundleIdentity, ConnectionInfo, RuntimeStatus } from "../ipc/generated";
import { ALL_CAPABILITIES } from "../ipc/generated.samples";
import { PERSONAL_EDITION, SAFE_EDITION } from "../ipc/generated.editions";

interface Props {
  status: RuntimeStatus;
  connection: ConnectionInfo;
  identity: BundleIdentity | null;
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

export function DoctorView({ status, connection, identity }: Props) {
  const granted = status.capabilities.capabilities;
  const editionRow = status.edition === "personal" ? PERSONAL_EDITION : SAFE_EDITION;
  const mismatched =
    identity !== null && identity.bundle_id !== identity.configured_bundle_id;

  return (
    <>
      <h1>Doctor</h1>
      <p className="subtitle">
        Everything below is read from the application core through{" "}
        <code>app_status</code>, <code>app_connection</code>, and{" "}
        <code>app_bundle_identity</code>. Nothing is cached or inferred.
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

      <h2>Granted capabilities</h2>
      <div className="card">
        <p className="muted" style={{ marginTop: 0 }}>
          The read-only application core can grant {ALL_CAPABILITIES.length}{" "}
          capabilities. Views are enabled from this list at runtime, not from a
          hardcoded switch.
        </p>
        <dl className="kv">
          {ALL_CAPABILITIES.map((capability) => (
            <div key={capability} style={{ display: "contents" }}>
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
    </>
  );
}
