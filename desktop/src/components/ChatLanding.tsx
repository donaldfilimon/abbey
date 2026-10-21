import type { V4Route } from "../ipc/generated";
import type { SurfaceId } from "../surfaces";
import { Icon } from "./Icon";
export function ChatLanding({
  draft,
  setDraft,
  navigate,
  routes,
  routeId,
  onRoute,
  routeStatus,
  onRetryRoutes,
  canSend,
  sending,
  sendBlocked,
  onSend,
}: {
  draft: string;
  setDraft: (value: string) => void;
  navigate: (id: SurfaceId) => void;
  routes: V4Route[];
  routeId: string | null;
  onRoute: (routeId: string) => void;
  routeStatus: "loading" | "ready" | "unavailable" | "error";
  onRetryRoutes: () => void;
  canSend: boolean;
  sending: boolean;
  sendBlocked: boolean;
  onSend: () => void;
}) {
  return (
    <div className="chat-landing">
      <div className="welcome">
        <div className="welcome-mark">
          <Icon name="spark" />
        </div>
        <p className="eyebrow">A LITTLE SPACE TO THINK</p>
        <h1>Hello, I’m Abbey.</h1>
        <p className="welcome-copy">
          Your ideas, memory, and work.
          <br />
          Together in one quiet place.
        </p>
      </div>
      <div className="suggestions">
        {(
          [
            {
              id: "memory",
              title: "Find a little context",
              copy: "Explore your saved memory.",
              icon: "memory",
            },
            {
              id: "models",
              title: "Meet your models",
              copy: "Inspect the available inventory.",
              icon: "models",
            },
            {
              id: "runs",
              title: "Follow your work",
              copy: "Look up a task and its events.",
              icon: "runs",
            },
          ] as const
        ).map((item) => (
          <button
            className="suggestion"
            key={item.id}
            onClick={() => navigate(item.id)}
          >
            <Icon name={item.icon} />
            <strong>{item.title}</strong>
            <span>{item.copy}</span>
            <Icon name="chevron" className="suggestion-arrow" />
          </button>
        ))}
      </div>
      <DraftComposer draft={draft} setDraft={setDraft} routes={routes} routeId={routeId} onRoute={onRoute} routeStatus={routeStatus} onRetryRoutes={onRetryRoutes} canSend={canSend} sending={sending} sendBlocked={sendBlocked} onSend={onSend} />
    </div>
  );
}

export function DraftComposer({
  draft,
  setDraft,
  routes,
  routeId,
  onRoute,
  routeStatus,
  onRetryRoutes,
  canSend,
  sending,
  sendBlocked,
  onSend,
}: {
  draft: string;
  setDraft: (value: string) => void;
  routes: V4Route[];
  routeId: string | null;
  onRoute: (routeId: string) => void;
  routeStatus: "loading" | "ready" | "unavailable" | "error";
  onRetryRoutes: () => void;
  canSend: boolean;
  sending: boolean;
  sendBlocked: boolean;
  onSend: () => void;
}) {
  const ready = canSend && draft.trim().length > 0 && !sending;
  return <>
      <div className="composer">
        <label className="sr-only" htmlFor="chat-draft">
          Message draft
        </label>
        <textarea
          id="chat-draft"
          placeholder="Make room for an idea…"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && ready) {
              event.preventDefault();
              onSend();
            }
          }}
          rows={3}
          maxLength={8192}
          disabled={sending}
        />
        <div className="composer-footer">
          <label className="composer-route">
            <span className="sr-only">Conversation route</span>
            <select
              value={routeId ?? ""}
              disabled={routes.length === 0 || routeStatus !== "ready" || sending}
              onChange={(event) => onRoute(event.target.value)}
            >
              {routes.length === 0 ? <option value="">No configured route</option> : routes.map(route => (
                <option key={route.route_id} value={route.route_id} disabled={!route.available}>{route.available ? route.label : `${route.label} · unavailable`}</option>
              ))}
            </select>
          </label>
          <button
            className="send-button"
            disabled={!ready}
            aria-label={ready ? "Send message" : "Send message unavailable"}
            aria-describedby="chat-availability"
            onClick={onSend}
          >
            <Icon name="arrow" />
          </button>
        </div>
      </div>
      <p className="composer-note" id="chat-availability">
        {routeStatus === "error"
          ? <>Conversation routes could not be loaded, so sending stays off. <button type="button" onClick={onRetryRoutes}>Retry routes</button></>
          : routeStatus === "loading"
            ? "Loading configured conversation routes…"
            : routeStatus === "unavailable"
              ? "This daemon has not granted route access. Sending stays off. Drafts stay in this window."
              : routes.length === 0
                ? <>This daemon has no configured conversation routes, so sending stays off. Drafts stay in this window. <button type="button" onClick={onRetryRoutes}>Refresh routes</button></>
                : routes.every(route => !route.available)
                  ? <>Configured routes are currently unavailable. <button type="button" onClick={onRetryRoutes}>Refresh routes</button></>
                  : sendBlocked
                    ? "This conversation has a submission with an unknown outcome. Refresh or inspect its history before continuing."
          : sending
            ? "Sending once. The reply appears as the daemon publishes it."
            : "Cmd or Ctrl and Enter sends once. Drafts stay in this window."}
      </p>
  </>;
}
