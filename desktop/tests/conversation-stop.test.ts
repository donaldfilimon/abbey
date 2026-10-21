import {expect, test} from "bun:test";
import {createConversations, type ConversationApi} from "../src/conversations/controller";
import type {RunState, V4Conversation, V4Message} from "../src/ipc/generated";
const RUN = "00000000-0000-4000-8000-000000000001";
const OTHER_RUN = "00000000-0000-4000-8000-000000000002";
const conversation = (id:string):V4Conversation => ({id, conversation_id:id, sequence:1, title:id, archived:false, pinned:false, message_count:1, created_at_ms:1, updated_at_ms:1});
const message = (id:string, state:RunState = "running", role:"user" | "assistant" = "assistant"):V4Message => ({conversation_id:id, run_id:id === "a" ? RUN : OTHER_RUN, sequence:1, role, state, text:"kept transcript", text_bytes:15, text_complete:role === "user" || !["queued", "starting", "running", "cancel_requested"].includes(state), created_at_ms:1});
const defer = <T,>() => {let resolve!:(value:T) => void; const promise = new Promise<T>(done => {resolve = done;}); return {promise, resolve};};
function fixture(overrides:Partial<ConversationApi> = {}) {
  const api:ConversationApi = {
    grants:async () => ({capabilities:["read_conversations", "write_conversations", "cancel_turns"]}),
    list:async () => ({conversations:[conversation("a"), conversation("b")], next_after:null}),
    search:async () => ({conversations:[], next_after:null}), read:async q => conversation(q.resource_id),
    messages:async q => ({conversation_id:q.conversation_id, messages:[message(q.conversation_id)], next_after:null}),
    create:async () => conversation("c"), rename:async q => conversation(q.conversation_id), archive:async q => conversation(q.resource_id), delete:async () => {},
    routes:async () => [], submit:async () => {throw Error("Stop must never submit");}, cancel:async () => "cancel_requested",
    ...overrides,
  };
  return createConversations(api);
}
test("double Stop sends exact pair once synchronously and preserves draft/transcript while stopping", async () => {
  const pending=defer<RunState>(); const calls:unknown[]=[];
  const store=fixture({cancel:(c,r) => {calls.push([c,r]); return pending.promise;}});
  await store.connect(); await store.select("a"); store.draft("next thought");
  const first=store.stop(); await store.stop();
  expect(calls).toEqual([["a",RUN]]); expect(store.snapshot().stop?.phase).toBe("sending");
  pending.resolve("cancel_requested"); await first; await store.stop();
  expect(calls.length).toBe(1); expect(store.snapshot().stop?.phase).toBe("requested");
  expect(store.snapshot().draft).toBe("next thought"); expect(store.snapshot().messages[0]?.text).toBe("kept transcript");
  store.newDraft();
});
test("ambiguous Stop reconciles only through reads, including navigation away and back", async () => {
  let calls=0, reads=0; let terminal=false;
  const store=fixture({cancel:async () => {calls++; throw {kind:"transport"};}, messages:async q => {reads++; return {conversation_id:q.conversation_id, messages:[message(q.conversation_id, terminal ? "cancelled" : "running")], next_after:null};}});
  await store.connect(); await store.select("a"); store.draft("A draft"); await store.stop();
  expect(store.snapshot().stop?.phase).toBe("unknown"); const before=reads;
  await store.select("b"); store.draft("B draft"); expect(store.snapshot().stop).toBeNull();
  await store.select("a"); await store.stop();
  expect(store.snapshot().draft).toBe("A draft"); expect(calls).toBe(1); expect(reads).toBeGreaterThan(before);
  terminal=true; await store.select("a");
  expect(store.snapshot().stop?.phase).toBe("settled"); expect(store.snapshot().stop?.note).toContain("cancelled");
  expect(store.snapshot().activeRunId).toBeNull(); expect(calls).toBe(1);
  await store.select("b"); expect(store.snapshot().draft).toBe("B draft"); store.newDraft();
});
test("late Stop acknowledgement cannot select or alter another conversation's draft/transcript/action", async () => {
  const pending=defer<RunState>(); const calls:unknown[]=[];
  const store=fixture({cancel:(c,r) => {calls.push([c,r]); return pending.promise;}});
  await store.connect(); await store.select("a"); store.draft("A draft"); const stopping=store.stop();
  await store.select("b"); store.draft("B draft"); pending.resolve("cancelled"); await stopping;
  expect(calls).toEqual([["a",RUN]]); expect(store.snapshot().selectedId).toBe("b");
  expect(store.snapshot().draft).toBe("B draft"); expect(store.snapshot().stop).toBeNull(); expect(store.snapshot().activeRunId).toBe(OTHER_RUN);
  expect(store.snapshot().messages[0]?.conversation_id).toBe("b");
  await store.select("a"); expect(store.snapshot().draft).toBe("A draft"); expect(store.snapshot().activeRunId).toBeNull(); store.newDraft();
});
test("active queued and complete running previews permit Stop; terminal replies and missing grant refuse it", async () => {
  let calls=0;
  const queued=fixture({messages:async q => ({conversation_id:q.conversation_id, messages:[message(q.conversation_id,"queued","user")], next_after:null}), cancel:async () => {calls++; return "cancelled";}});
  await queued.connect(); await queued.select("a"); expect(queued.snapshot().activeRunId).toBe(RUN); await queued.stop(); expect(calls).toBe(1); queued.newDraft();
  for (const state of ["succeeded", "failed", "cancelled", "interrupted"] as const) {
    const store=fixture({messages:async q => ({conversation_id:q.conversation_id, messages:[message(q.conversation_id,state)], next_after:null}), cancel:async () => {calls++; return "cancelled";}});
    await store.connect(); await store.select("a"); expect(store.snapshot().activeRunId).toBeNull(); await store.stop(); store.newDraft();
  }
  const final=fixture({messages:async q => ({conversation_id:q.conversation_id, messages:[{...message(q.conversation_id), text_complete:true}], next_after:null})});
  await final.connect(); await final.select("a"); expect(final.snapshot().activeRunId).toBe(RUN); final.newDraft();
  const denied=fixture({grants:async () => ({capabilities:["read_conversations"]}), cancel:async () => {calls++; return "cancelled";}});
  await denied.connect(); await denied.select("a"); expect(denied.snapshot().canCancel).toBe(false); await denied.stop(); expect(calls).toBe(1); denied.newDraft();
});
test("completion racing Stop is reported as succeeded and removes active Stop immediately", async () => {
  const store=fixture({cancel:async () => "succeeded"});
  await store.connect(); await store.select("a"); store.draft("keep"); await store.stop();
  expect(store.snapshot().stop?.note).toBe("Reply succeeded."); expect(store.snapshot().activeRunId).toBeNull(); expect(store.snapshot().draft).toBe("keep"); store.newDraft();
});
test("a stale rendered Stop action cannot cancel a newly selected conversation or replacement run", async () => {
  const calls:unknown[]=[];
  const store=fixture({cancel:async (c,r) => {calls.push([c,r]); return "cancelled";}});
  await store.connect(); await store.select("a");
  const old=store.snapshot(); await store.select("b");
  await store.stop(old.selectedId, old.activeRunId);
  expect(calls).toEqual([]); expect(store.snapshot().selectedId).toBe("b"); expect(store.snapshot().stop).toBeNull(); store.newDraft();
});
test("terminal content settles an ambiguous Stop without another mutation or discarding the next draft", async () => {
  let calls=0, terminal=false;
  const store=fixture({
    cancel:async () => {calls++; throw {kind:"protocol"};},
    content:async q => ({conversation_id:q.conversation_id, run_id:q.run_id, after:q.after, next_after:terminal ? null : q.after, events:terminal ? [{conversation_id:q.conversation_id,run_id:q.run_id,sequence:q.after+1,timestamp_ms:1,payload:{kind:"terminal" as const,data:{state:"cancelled" as const}}}] : []}),
  });
  await store.connect(); await store.select("a"); store.draft("next draft"); await store.stop();
  terminal=true; await store.loadContent(store.snapshot().messages[0]!);
  expect(calls).toBe(1); expect(store.snapshot().stop?.phase).toBe("settled"); expect(store.snapshot().activeRunId).toBeNull(); expect(store.snapshot().draft).toBe("next draft"); store.newDraft();
});
test("a captured Stop run cannot be retargeted to a later turn in the same conversation", async () => {
  let run=RUN; const calls:unknown[]=[];
  const store=fixture({messages:async q => ({conversation_id:q.conversation_id, messages:[{...message(q.conversation_id),run_id:run}], next_after:null}),cancel:async (c,r) => {calls.push([c,r]); return "cancelled";}});
  await store.connect(); await store.select("a"); const captured=store.snapshot();
  run=OTHER_RUN; await store.select("a"); await store.stop(captured.selectedId,captured.activeRunId);
  expect(calls).toEqual([]); expect(store.snapshot().activeRunId).toBe(OTHER_RUN); store.newDraft();
});
