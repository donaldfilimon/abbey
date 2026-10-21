import { useEffect, useRef, useState } from "react";
import type { MutationKind } from "./controller";
export function ConversationDialog({kind, title, close, submit}: {kind:MutationKind; title:string; close:() => void; submit:(value:string) => void}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [value, setValue] = useState(title);
  const deleting = kind === "delete";
  useEffect(() => {const dialog = ref.current!; dialog.showModal(); return () => dialog.close();}, []);
  return <dialog ref={ref} className="conversation-dialog" aria-labelledby="conversation-dialog-title" onCancel={close} onClose={close}>
    <form onSubmit={event => {event.preventDefault(); submit(value.trim());}}>
      <p className="eyebrow">YOUR CONVERSATIONS</p>
      <h2 id="conversation-dialog-title">{deleting ? "Delete conversation?" : kind === "rename" ? "Rename conversation" : "New conversation"}</h2>
      {deleting ? <p>Delete “{title}” and its stored transcript? This cannot be undone. Your other conversations remain available.</p> : <><label htmlFor="conversation-title">Conversation title</label><input id="conversation-title" autoFocus required maxLength={256} value={value} onChange={event => setValue(event.target.value)}/></>}
      <div className="dialog-actions"><button type="button" autoFocus={deleting} onClick={close}>Cancel</button><button className={deleting ? "danger-button" : "primary-button"} disabled={!deleting && !value.trim()}>{deleting ? "Delete conversation" : kind === "rename" ? "Save title" : "Create conversation"}</button></div>
    </form>
  </dialog>;
}
