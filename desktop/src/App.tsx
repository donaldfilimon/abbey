import { useEffect, useState } from "react";
import type { IpcError, V3Capability, RuntimeStatus } from "./ipc/generated";
import {
  ALL_APP_CAPABILITIES,
  DESKTOP_READ_CAPABILITIES,
} from "./ipc/generated.samples";
import { surfaceById, type Surface, type SurfaceId } from "./surfaces";
import { ClaimsView } from "./views/ClaimsView";
import { DoctorView } from "./views/DoctorView";
import { ErrorCard } from "./views/ErrorCard";
import { MemoryView } from "./views/MemoryView";
import { ModelsView } from "./views/ModelsView";
import { RoutesView } from "./views/RoutesView";
import { RunsView } from "./views/RunsView";
import { ToolsView } from "./views/ToolsView";
import { UnavailableView } from "./views/UnavailableView";
import { useBootstrap } from "./hooks/useBootstrap";
import { usePreferences, type Appearance } from "./hooks/usePreferences";
import { ConversationWorkspace } from "./conversations/Workspace";
import { History } from "./conversations/History";
import { useConversations } from "./conversations/useConversations";
import { CommandPalette } from "./components/CommandPalette";
import { Icon } from "./components/Icon";
import { PresentationSettings } from "./components/PresentationSettings";
import { SystemIntegrationPanel } from "./components/SystemIntegration";
const PRIMARY: SurfaceId[] = [
  "chat",
  "runs",
  "automations",
  "memory",
  "models",
  "tools",
];
const ADVANCED: SurfaceId[] = ["routes", "claims", "training", "cluster"];
// `DESKTOP_READ_CAPABILITIES` mixes protocol-v1/v2 `AppCapability` reads with
// protocol-v3 `V3Capability` reads. `ALL_APP_CAPABILITIES` is the exhaustive
// v1/v2 set, so anything in the former but not the latter is a v3 grant. This
// keeps `available()` derived from the generated capability lists instead of
// a hardcoded pair that silently misses a new v3 read (e.g. `ReadClaimsById`).
const V3_GATED_CAPABILITIES = new Set<string>(
  DESKTOP_READ_CAPABILITIES.filter(
    (capability) =>
      !(ALL_APP_CAPABILITIES as readonly string[]).includes(capability),
  ),
);

/**
 * Whether the window may show this surface.
 *
 * Exported so the grant rule is testable directly: it is the one place
 * that decides whether a capability the daemon refused still shows a view.
 * A surface whose capability is v3-gated needs the v3 grant; every other
 * surface needs the protocol-v1 status grant; and a surface that requires
 * nothing is never available.
 */
export function available(
  surface: Surface,
  status: RuntimeStatus | null,
  v3Grants: V3Capability[],
): boolean {
  if (surface.requires === null) return false;
  if (V3_GATED_CAPABILITIES.has(surface.requires)) {
    return (v3Grants as readonly string[]).includes(surface.requires);
  }
  if (status === null) return false;
  return (status.capabilities.capabilities as readonly string[]).includes(
    surface.requires,
  );
}

/**
 * Whether the window may show the surface it is on.
 *
 * The Chat surface is the one exception, and it is a real one: its
 * authority comes from the separate v4 conversation session, not from the
 * protocol-v1 status capability set, so `available()` — which only knows
 * about the v1/v2/v3 grants — can never answer it. The Chat surface is
 * therefore answered by that session's own read authority.
 */
export function grantedFor(
  active: SurfaceId,
  surface: Surface,
  status: RuntimeStatus | null,
  v3Grants: V3Capability[],
  chatCanRead: boolean,
): boolean {
  if (active === "chat") return chatCanRead;
  return available(surface, status, v3Grants);
}

/**
 * The connection banner.
 *
 * The window says whether it is still connecting or whether the runtime
 * could not be reached, and offers the one action that can change that.
 */
export function ConnectionBanner({
  loading,
  error,
  onRetry,
}: {
  loading: boolean;
  error: IpcError | null;
  onRetry: () => void;
}) {
  return (
    <div
      className={`connection-banner ${error ? "connection-error" : ""}`}
      role="status"
    >
      <span className="status-dot" />
      <span>
        {loading
          ? "Connecting to your Abbey runtime…"
          : "The runtime couldn’t be reached. Check that Abbey is running, then retry."}
      </span>
      {error && (
        <>
          <ErrorCard error={error} />
          <button onClick={onRetry}>
            <Icon name="refresh" />
            Retry connection
          </button>
        </>
      )}
    </div>
  );
}

/**
 * The empty state for a surface the connected runtime has not unlocked.
 *
 * It names the grant that is missing instead of showing an empty page, so
 * a reader can tell "not granted" from "nothing to show".
 */
export function EmptyState({
  surface,
  loading,
  connected,
  onRetry,
}: {
  surface: Surface;
  loading: boolean;
  connected: boolean;
  onRetry: () => void;
}) {
  return (
    <div className="empty-state">
      <div className="empty-icon">
        <Icon name={surface.id} />
      </div>
      <h1>{surface.label}</h1>
      <p>
        {loading
          ? "Checking what’s available in your runtime…"
          : connected
            ? "This view needs a capability your connected runtime hasn’t granted."
            : "Connect to your Abbey runtime to explore this view."}
      </p>
      <button onClick={onRetry} disabled={loading}>
        {loading ? "Checking connection…" : "Retry connection"}
      </button>
      {connected && (
        <details>
          <summary>Capability details</summary>
          <p>
            Required runtime grant: <code>{surface.requires}</code>
          </p>
        </details>
      )}
    </div>
  );
}

export function App() {
  const [active, setActive] = useState<SurfaceId>("chat");
  const {state: history, conversations} = useConversations();
  const [palette, setPalette] = useState(false);
  const { boot, error, loading, retry } = useBootstrap();
  const { appearance, collapsed, material, density, setAppearance, setMaterial, setDensity, toggleSidebar, resetPresentation } =
    usePreferences();
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && !event.altKey) {
        if (event.key.toLowerCase() === "k") {
          event.preventDefault();
          setPalette((open) => !open);
        }
        if (event.key.toLowerCase() === "b") {
          event.preventDefault();
          toggleSidebar();
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [toggleSidebar]);
  const surface = surfaceById(active);
  const granted = grantedFor(
    active,
    surface,
    boot?.status ?? null,
    boot?.v3Grants ?? [],
    history.canRead,
  );
  const connectionLabel = loading
    ? "Connecting"
    : boot
      ? boot.connection.source === "daemon"
        ? "abbeyd (authenticated Unix socket)"
        : "Local application core"
      : "Connection unavailable";
  const navigationButton = (id: SurfaceId) => {
    const entry = surfaceById(id);
    return (
      <button
        key={id}
        type="button"
        className="nav-button"
        aria-label={entry.label}
        title={collapsed ? entry.label : undefined}
        aria-current={active === id ? "page" : undefined}
        onClick={() => setActive(id)}
      >
        <Icon name={id} />
        <span className="nav-label">{entry.label}</span>
      </button>
    );
  };
  return (
    <div className={`shell ${collapsed ? "sidebar-collapsed" : ""}`}>
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <Icon name="spark" />
          </span>
          <span className="nav-label">
            abbey<span className="brand-caption">A space for your work</span>
          </span>
        </div>
        <button className="new-chat" aria-label="New conversation draft" onClick={() => {conversations.newDraft(); setActive("chat");}}>
          <Icon name="plus" />
          <span className="nav-label">New conversation</span>
        </button>
        <span id="new-chat-note" className="sr-only">
          Start a new window-session draft. Create conversation metadata in Chat when the daemon grants changes.
        </span>
        <nav aria-label="Main navigation">
          <div className="nav-group">Workspace</div>
          {PRIMARY.map(navigationButton)}
          <div className="nav-group">Advanced</div>
          {ADVANCED.map(navigationButton)}
        </nav>
        <History controller={conversations} state={history} open={() => setActive("chat")} />
        <div className="sidebar-bottom">
          {navigationButton("doctor")}
          <div className="sidebar-status">
            <span className={`status-dot ${boot ? "online" : ""}`} />
            <span className="nav-label">
              {connectionLabel}
              <small>
                {boot
                  ? `${boot.status.version} · ${boot.status.edition}`
                  : "Your workspace stays here"}
              </small>
            </span>
          </div>
        </div>
      </aside>
      <div className="workspace">
        <header className="toolbar">
          <div className="toolbar-leading">
            <button
              className="icon-button"
              onClick={toggleSidebar}
              aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
              title="Toggle sidebar (⌘/Ctrl B)"
            >
              <Icon name="panel" />
            </button>
            <span className="toolbar-divider" />
            <span>{surface.label}</span>
          </div>
          <button
            className="search-trigger"
            onClick={() => setPalette(true)}
            aria-label="Search pages and commands"
          >
            <Icon name="search" />
            <span>Quick navigation</span>
            <kbd>⌘ / Ctrl K</kbd>
          </button>
        </header>
        <main
          className={`content ${active === "chat" ? "chat-content" : ""}`}
          id="main-content"
        >
          {(loading || error !== null) && (
            <ConnectionBanner loading={loading} error={error} onRetry={retry} />
          )}
          {active === "chat" ? (
            <ConversationWorkspace controller={conversations} state={history} navigate={setActive} />
          ) : active === "doctor" ? (
            <>
              <div className="page-heading">
                <p className="eyebrow">MAKE YOURSELF AT HOME</p>
                <h1>Settings</h1>
                <p className="subtitle">
                  A little more you. A little less friction.
                </p>
              </div>
              <section className="card settings-section">
                <div>
                  <h2>Appearance</h2>
                  <p className="muted">
                    Follow your system appearance, or choose a light or dark theme for this window.
                  </p>
                </div>
                <div className="segmented" aria-label="Appearance">
                  {(["system", "light", "dark"] as Appearance[]).map(
                    (value) => (
                      <button
                        key={value}
                        aria-pressed={appearance === value}
                        onClick={() => setAppearance(value)}
                      >
                        {value[0]!.toUpperCase() + value.slice(1)}
                      </button>
                    ),
                  )}
                </div>
              </section>
              <section className="card settings-section">
                <div>
                  <h2>Connection</h2>
                  <p className="muted">{connectionLabel}</p>
                </div>
                <button onClick={retry} disabled={loading}>
                  <Icon name="refresh" />
                  {loading ? "Connecting…" : "Reconnect"}
                </button>
              </section>
              <PresentationSettings material={material} density={density} setMaterial={setMaterial} setDensity={setDensity} resetPresentation={resetPresentation} />
              <SystemIntegrationPanel />
              <details className="card diagnostics">
                <summary>Runtime diagnostics</summary>
                {error && <ErrorCard error={error} />}
                {boot && granted ? (
                  <DoctorView
                    status={boot.status}
                    connection={boot.connection}
                    identity={boot.identity}
                    routeId={history.routeId}
                  />
                ) : (
                  <p className="muted">
                    {loading
                      ? "Waiting for runtime status…"
                      : "Runtime diagnostics are unavailable. Retry the connection above."}
                  </p>
                )}
              </details>
            </>
          ) : surface.requires === null ? (
            <UnavailableView key={active} surface={surface} />
          ) : boot && granted ? (
            <div key={`${active}-${boot.status.version}`} className="data-view">
              {active === "claims" && (
                <ClaimsView
                  claimByIdGranted={boot.v3Grants.includes("read_claims_by_id")}
                />
              )}
              {active === "routes" && <RoutesView />}
              {active === "runs" && <RunsView />}
              {active === "memory" && <MemoryView />}
              {active === "models" && <ModelsView />}
              {active === "tools" && <ToolsView />}
            </div>
          ) : (
            <EmptyState
              surface={surface}
              loading={loading}
              connected={boot !== null}
              onRetry={retry}
            />
          )}
        </main>
        <footer className="workspace-footer">
          <span>Abbey · Your work, thoughtfully connected</span>
          <span>{connectionLabel}</span>
        </footer>
      </div>
      {palette && (
        <CommandPalette close={() => setPalette(false)} navigate={setActive} />
      )}
    </div>
  );
}
