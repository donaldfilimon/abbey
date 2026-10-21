import { MessageContent } from "./MessageContent";
import { useState } from "react";
import { ChatLanding, DraftComposer } from "../components/ChatLanding";
import type { SurfaceId } from "../surfaces";
import type { Conversations, MutationKind } from "./controller";
import { ConversationDialog } from "./ConversationDialog";
const availability = {
  loading:"Connecting conversation history…", ready:"Conversation history connected", "read-only":"Read-only access. This daemon hasn’t granted conversation changes.",
  "no-daemon":"Conversation history needs a configured Abbey daemon. Local core reads remain available.", "no-grant":"This daemon hasn’t granted conversation history access.", offline:"Conversation connection unavailable. Existing content and drafts are kept.",
};
export function ConversationWorkspace({controller, state, navigate}: {controller:Conversations; state:ReturnType<Conversations["snapshot"]>; navigate:(id:SurfaceId) => void}) {
  const [dialog, setDialog] = useState<MutationKind | null>(null);
  const busy = state.mutation !== null;
  const stopping = state.stop?.runId === state.activeRunId ? state.stop : null;
  const stopLabel = stopping ? {sending:"Requesting Stop…", requested:"Stopping…", unknown:"Stop outcome unknown", rejected:"Stop rejected", settled:"Stopped"}[stopping.phase] : state.messages.some(message => message.run_id === state.activeRunId && message.state === "cancel_requested") ? "Stopping…" : "Stop";
  const showNotice = state.note !== null && state.note !== availability[state.access];
  return <div className={`conversation-workspace ${state.selectedId ? "has-conversation" : ""}`}>
    <div className="conversation-access" role="status"><span>{availability[state.access]}</span><button disabled={state.access === "loading"} onClick={() => void controller.connect()}>Reconnect history</button></div>
    {showNotice && <div className="conversation-notice" role="status">{state.note}{state.mutation?.phase === "unknown" && <><button onClick={() => void controller.reconcile()}>Check current state</button>{state.mutation.kind === "create" && <p>A lost create response cannot be matched reliably by title. The outcome stays unknown in this window; inspect history without repeating the create.</p>}</>}</div>}
    <div className="conversation-toolbar">
      <div><h1>{state.selected?.title ?? (state.selectedId ? "Conversation" : "A little space to think")}</h1>{state.selected && <p className="muted">{state.selected.pinned ? "Pinned · " : ""}{state.selected.archived ? "Archived · " : ""}{state.selected.message_count} messages</p>}</div>
      <div className="conversation-actions">
        {!state.selectedId ? <button disabled={!state.canWrite || busy} onClick={() => setDialog("create")}>Create conversation</button> : <>
          <button disabled={!state.canRead || state.reading} onClick={() => void controller.select(state.selectedId!)}>Refresh conversation</button>
          <button disabled={!state.canWrite || busy || !state.selected} onClick={() => setDialog("rename")}>Rename</button>
          <button disabled={!state.canWrite || busy || !state.selected || state.selected.archived} onClick={() => void controller.mutate("archive")}>{state.selected?.archived ? "Archived" : "Archive"}</button>
          <button disabled={!state.canWrite || !state.canUpdate || busy || !state.selected || state.reading} onClick={() => void controller.updateMetadata({title:null,archived:null,pinned:!state.selected!.pinned})}>{state.selected?.pinned ? "Unpin" : "Pin"}</button>
          {state.selected?.archived && <button disabled={!state.canWrite || !state.canUpdate || busy || state.reading} onClick={() => void controller.updateMetadata({title:null,archived:false,pinned:null})}>Unarchive</button>}
          <button className="danger-button" disabled={!state.canWrite || busy || !state.selected} onClick={() => setDialog("delete")}>Delete</button>
        </>}
      </div>
    </div>
    {state.selectedId ? <>
      {state.canCancel && state.activeRunId && <button className="stop-button" disabled={!state.canRead || state.reading || (state.stop?.runId === state.activeRunId) || state.messages.some(message => message.run_id === state.activeRunId && message.state === "cancel_requested")} onClick={() => void controller.stop(state.selectedId, state.activeRunId)}>{stopLabel}</button>}
      {state.stop && <p className="conversation-notice" role="status">{state.stop.note}</p>}
      {state.retries.filter(attempt => attempt.conversationId === state.selectedId).map(attempt => <p className="conversation-notice" role="status" key={attempt.originalRunId}>Retry of attempt {attempt.originalRunId} · route {attempt.routeId} · {attempt.phase}{attempt.runId ? ` · new attempt ${attempt.runId}` : ""}. Relationship recorded in this window only.</p>)}
      <section className="message-history" aria-label="Authorized conversation messages" aria-busy={state.reading}>
        {state.messages.map(message => <article className={`message message-${message.role}`} key={message.sequence}><h2>{message.role === "user" ? "You" : message.role === "assistant" ? "Abbey" : "System"}</h2>{message.role === "assistant" && message.state !== "succeeded" && <p className="message-state" role="status">{message.state === "running" ? "Replying…" : message.state === "queued" ? "Waiting to reply…" : message.state === "starting" ? "Starting reply…" : message.state === "cancel_requested" ? "Stopping reply…" : message.state === "failed" ? message.text ? "Reply stopped early" : "Reply failed" : message.state === "cancelled" ? "Reply cancelled" : "Reply interrupted"}</p>}<MessageContent text={message.text} complete={message.text_complete && message.text_bytes <= new TextEncoder().encode(message.text).length}/>{state.messages.filter(current => current.run_id === message.run_id).at(-1)?.sequence === message.sequence && ["failed", "interrupted", "cancelled"].includes(message.state) && <button disabled={!controller.canRetry(message.conversation_id, message.run_id)} onClick={() => void controller.retry(message.conversation_id, message.run_id)}>Retry with selected route</button>}{message.role === "assistant" && message.text_bytes > new TextEncoder().encode(message.text).length && <button onClick={() => void controller.loadContent(message)}>Load complete reply</button>}</article>)}
        {!state.messages.length && <div className="history-placeholder">{state.reading ? "Reading this conversation…" : "No stored messages on this page."}</div>}
        {state.moreMessages && <button disabled={state.reading} onClick={() => void controller.moreMessages()}>{state.reading ? "Reading…" : "More messages"}</button>}
        {state.messages.length >= 256 && <p className="muted">This window shows up to 256 messages. Refresh to read from the beginning.</p>}
      </section>
      <DraftComposer draft={state.draft} setDraft={controller.draft} routes={state.routes} routeId={state.routeId} onRoute={controller.chooseRoute} routeStatus={state.routeStatus} onRetryRoutes={() => void controller.refreshRoutes()} canSend={state.canWrite && !state.retries.some(attempt => attempt.phase === "unknown" && attempt.conversationId === state.selectedId) && state.mutation === null && state.routeStatus === "ready" && state.routes.some(route => route.available && route.route_id === state.routeId) && state.sendUnknownFor !== state.selectedId} sendBlocked={state.selectedId !== null && state.sendUnknownFor === state.selectedId} sending={state.sending} onSend={() => void controller.send()}/>
    </> : <ChatLanding draft={state.draft} setDraft={controller.draft} navigate={navigate} routes={state.routes} routeId={state.routeId} onRoute={controller.chooseRoute} routeStatus={state.routeStatus} onRetryRoutes={() => void controller.refreshRoutes()} canSend={state.canWrite && !state.retries.some(attempt => attempt.phase === "unknown" && attempt.conversationId === state.selectedId) && state.mutation === null && state.routeStatus === "ready" && state.routes.some(route => route.available && route.route_id === state.routeId)} sendBlocked={false} sending={state.sending} onSend={() => void controller.send()}/>}
    <p className="conversation-limits">Message text is bounded by the daemon. {!state.canUpdate ? "Pinning and unarchiving need conversation-update authority. " : ""}Full transcript export is unavailable on this bridge.</p>
    {dialog && <ConversationDialog key={`${dialog}-${state.selectedId}`} kind={dialog} title={dialog === "create" ? "" : state.selected?.title ?? "Conversation"} close={() => setDialog(null)} submit={value => {setDialog(null); void controller.mutate(dialog, value);}}/>}
  </div>;
}
