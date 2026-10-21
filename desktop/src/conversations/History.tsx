import { useEffect, useId, useRef, useState } from "react";
import type { Conversations } from "./controller";
import { Icon } from "../components/Icon";
export function History({controller, state, open}: {controller:Conversations; state:ReturnType<Conversations["snapshot"]>; open:() => void}) {
  const [query, setQuery] = useState("");
  const [picker, setPicker] = useState(false);
  const ref = useRef<HTMLDialogElement>(null);
  const queryId = useId();
  useEffect(() => {if(picker) ref.current?.showModal();}, [picker]);
  const content = <>
    <form className="history-search" onSubmit={event => {event.preventDefault(); void controller.search(query);}}>
      <label htmlFor={queryId} className="sr-only">Search conversation titles</label>
      <input id={queryId} type="search" placeholder="Search conversations…" value={query} maxLength={256} onChange={event => setQuery(event.target.value)} disabled={!state.canRead}/>
      <button className="icon-button" aria-label="Search conversation titles" disabled={!state.canRead}><Icon name="search"/></button>
    </form>
    <div className="history-heading"><span>Conversations</span><button className="icon-button" aria-label="Refresh conversation history" disabled={!state.canRead || state.listing} onClick={() => void controller.refresh()}><Icon name="refresh"/></button></div>
    {state.items.length ? <ul>{state.items.map(item => <li key={item.id}><button aria-current={state.selectedId === item.id ? "true" : undefined} title={item.title} onClick={() => {open(); void controller.select(item.id); setPicker(false);}}>
      <span className="history-title">{item.title}</span><span className="history-meta">{item.pinned ? "Pinned · " : ""}{item.archived ? "Archived · " : ""}{item.message_count} messages</span>
    </button></li>)}</ul> : <p className="history-empty">{state.listing ? "Reading history…" : state.canRead ? "No conversations found." : "Connect to view history."}</p>}
    {state.more && <button className="history-more" disabled={state.listing} onClick={() => void controller.more()}>{state.listing ? "Reading…" : "More conversations"}</button>}
    {state.items.length >= 256 && <p className="history-empty">Showing 256 conversations. Search titles to narrow the list.</p>}
  </>;
  return <>
    <button className="history-open nav-button" aria-label="Browse conversations" title="Browse conversations" onClick={() => setPicker(true)}><Icon name="search"/><span className="nav-label">Conversation history</span></button>
    {!picker && <section className="conversation-history" aria-label="Conversation history">{content}</section>}
    {picker && <dialog ref={ref} className="conversation-dialog history-picker" aria-label="Conversation history" onCancel={() => setPicker(false)} onClose={() => setPicker(false)}><div className="history-picker-heading"><h2>Conversations</h2><button autoFocus onClick={() => setPicker(false)}>Close</button></div><section className="conversation-history">{content}</section></dialog>}
  </>;
}
