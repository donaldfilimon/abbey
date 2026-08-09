import { useEffect, useState } from "react";
import {
  appBundleIdentity,
  appConnection,
  appStatus,
  toIpcError,
} from "./ipc/client";
import type {
  BundleIdentity,
  ConnectionInfo,
  IpcError,
  RuntimeStatus,
} from "./ipc/generated";
import { SURFACES, type Surface, type SurfaceId } from "./surfaces";
import { ClaimsView } from "./views/ClaimsView";
import { DoctorView } from "./views/DoctorView";
import { ErrorCard } from "./views/ErrorCard";
import { UnavailableView } from "./views/UnavailableView";

interface Bootstrap {
  status: RuntimeStatus;
  connection: ConnectionInfo;
  identity: BundleIdentity | null;
}

function available(surface: Surface, status: RuntimeStatus | null): boolean {
  if (surface.requires === null) return false;
  if (status === null) return false;
  return status.capabilities.capabilities.includes(surface.requires);
}

export function App() {
  const [active, setActive] = useState<SurfaceId>("doctor");
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [error, setError] = useState<IpcError | null>(null);

  useEffect(() => {
    let live = true;
    Promise.all([appStatus(), appConnection()])
      .then(async ([status, connection]) => {
        // Bundle identity is informational; a failure here must not blank the
        // whole window.
        const identity = await appBundleIdentity().catch(() => null);
        if (live) setBoot({ status, connection, identity });
      })
      .catch((thrown: unknown) => {
        if (live) setError(toIpcError(thrown));
      });
    return () => {
      live = false;
    };
  }, []);

  const status = boot?.status ?? null;
  const surface = SURFACES.find((entry) => entry.id === active) ?? SURFACES[0]!;
  const surfaceIsAvailable = available(surface, status);

  return (
    <div className="shell">
      <nav className="sidebar">
        <div className="brand">
          Abbey
          <small>
            {status === null ? "connecting…" : `${status.version} · ${status.edition}`}
          </small>
        </div>
        <div className="nav-group">Available</div>
        {SURFACES.filter((entry) => entry.requires !== null).map((entry) => (
          <button
            key={entry.id}
            type="button"
            className="nav-button"
            aria-current={entry.id === active ? "page" : undefined}
            onClick={() => setActive(entry.id)}
          >
            {entry.label}
          </button>
        ))}
        <div className="nav-group">Not yet available</div>
        {SURFACES.filter((entry) => entry.requires === null).map((entry) => (
          <button
            key={entry.id}
            type="button"
            className="nav-button unavailable"
            aria-current={entry.id === active ? "page" : undefined}
            onClick={() => setActive(entry.id)}
          >
            {entry.label}
          </button>
        ))}
      </nav>

      <main className="content">
        {error !== null && <ErrorCard error={error} />}
        {error === null && boot === null && <p className="muted">Connecting to Abbey…</p>}
        {boot !== null && surface.id === "doctor" && surfaceIsAvailable && (
          <DoctorView
            status={boot.status}
            connection={boot.connection}
            identity={boot.identity}
          />
        )}
        {boot !== null && surface.id === "claims" && surfaceIsAvailable && <ClaimsView />}
        {boot !== null && surface.requires !== null && !surfaceIsAvailable && (
          <>
            <h1>{surface.label}</h1>
            <div className="card error">
              <strong>Capability not granted</strong>
              <p className="claim-note">
                This build of Abbey did not grant <code>{surface.requires}</code>, so
                the view has no data source. Availability is read from{" "}
                <code>app_status</code> at runtime, never assumed.
              </p>
            </div>
          </>
        )}
        {surface.requires === null && <UnavailableView surface={surface} />}
      </main>
    </div>
  );
}
