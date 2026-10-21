import {expect, test} from "bun:test";
import {createConversations, type ConversationApi} from "../src/conversations/controller";
import type {RunState, V4Conversation, V4Message, V4SubmitTurn, V4TurnSubmission} from "../src/ipc/generated";
const conversation=(id:string):V4Conversation=>({id,conversation_id:id,sequence:1,title:id,archived:false,pinned:false,message_count:2,created_at_ms:1,updated_at_ms:1});
function fixture(state:RunState="failed", overrides:Partial<ConversationApi>={}) {
  const calls:V4SubmitTurn[]=[];
  const messages:V4Message[]=[{conversation_id:"a",run_id:"old",sequence:1,role:"user",state,text:"  original input  ",text_bytes:18,text_complete:true,created_at_ms:1},{conversation_id:"a",run_id:"old",sequence:2,role:"assistant",state,text:"partial",text_bytes:7,text_complete:true,created_at_ms:1}];
  const api:ConversationApi={grants:async()=>({capabilities:["read_conversations","write_conversations","read_routes"]}),list:async()=>({conversations:[conversation("a")],next_after:null}),search:async()=>({conversations:[],next_after:null}),read:async q=>conversation(q.resource_id),messages:async q=>({conversation_id:q.conversation_id,messages:q.conversation_id==="a"?messages:[],next_after:null}),create:async()=>conversation("c"),rename:async()=>conversation("a"),archive:async()=>conversation("a"),delete:async()=>{},routes:async()=>[{route_id:"local",backend:"abi",label:"Local",available:true}],submit:async q=>{calls.push(q);return {conversation_id:"a",run_id:"new",idempotency_key:q.idempotency_key,user_message_sequence:3,state:"queued",duplicate:false};},...overrides};
  return {store:createConversations(api),calls,messages};
}
test("retry uses exact original input and fresh key once, preserving draft and recording honest relationship",async()=>{
  for(const state of ["failed","interrupted","cancelled"] as const){
    const {store,calls}=fixture(state);await store.connect();await store.select("a");store.draft("next thought");
    await store.retry("a","old");await store.retry("a","old");
    expect(calls).toHaveLength(1);expect(calls[0]?.input).toBe("  original input  ");expect(calls[0]?.idempotency_key).toStartWith("turn-");
    expect(store.snapshot().draft).toBe("next thought");expect(store.snapshot().retries[0]).toMatchObject({originalRunId:"old",runId:"new",phase:"accepted"});store.newDraft();
  }
});
test("retry refuses active/successful, incomplete input, unavailable routes, metadata effects and stale actions",async()=>{
  for(const state of ["queued","running","cancel_requested","succeeded"] as const){const {store,calls}=fixture(state);await store.connect();await store.select("a");await store.retry("a","old");expect(calls).toHaveLength(0);store.newDraft();}
  const {store,calls,messages}=fixture();await store.connect();await store.select("a");messages[0]!.text_complete=false;await store.retry("a","old");expect(calls).toHaveLength(0);messages[0]!.text_complete=true;await store.select("b");await store.retry("a","old");expect(calls).toHaveLength(0);store.newDraft();
});
test("late retry acknowledgement preserves navigation and unknown response never allows replay",async()=>{
  let resolve!:(r:V4TurnSubmission)=>void;const calls:V4SubmitTurn[]=[];
  const {store}=fixture("failed",{submit:q=>{calls.push(q);return new Promise(done=>{resolve=done;});}});await store.connect();await store.select("a");store.draft("A");const pending=store.retry("a","old");await store.retry("a","old");expect(calls).toHaveLength(1);await store.select("b");store.draft("B");resolve({conversation_id:"a",run_id:"new",idempotency_key:calls[0]!.idempotency_key,user_message_sequence:3,state:"queued",duplicate:false});await pending;expect(store.snapshot().selectedId).toBe("b");expect(store.snapshot().draft).toBe("B");store.newDraft();
  let count=0;const unknown=fixture("failed",{submit:async()=>{count++;throw {kind:"transport"};}}).store;await unknown.connect();await unknown.select("a");unknown.draft("keep");await unknown.retry("a","old");await unknown.retry("a","old");expect(count).toBe(1);expect(unknown.snapshot().retries[0]?.phase).toBe("unknown");expect(unknown.snapshot().draft).toBe("keep");unknown.newDraft();
});
test("retry refuses unavailable route, unresolved send/effect, another active run and truncated input",async()=>{
  const unavailable=fixture("failed",{routes:async()=>[{route_id:"local",backend:"abi",label:"Local",available:false}]});await unavailable.store.connect();await unavailable.store.select("a");await unavailable.store.retry("a","old");expect(unavailable.calls).toHaveLength(0);unavailable.store.newDraft();
  const effect=fixture("failed",{rename:async()=>{throw {kind:"transport"};}});await effect.store.connect();await effect.store.select("a");await effect.store.mutate("rename","changed");await effect.store.retry("a","old");expect(effect.calls).toHaveLength(0);effect.store.newDraft();
  const active=fixture();active.messages.push({...active.messages[1]!,run_id:"active",sequence:3,state:"running"});await active.store.connect();await active.store.select("a");await active.store.retry("a","old");expect(active.calls).toHaveLength(0);active.store.newDraft();
  const truncated=fixture();truncated.messages[0]!.text_bytes=999;await truncated.store.connect();await truncated.store.select("a");await truncated.store.retry("a","old");expect(truncated.calls).toHaveLength(0);truncated.store.newDraft();
});
test("uncorrelated retry response stays unknown and blocks ordinary sends without consuming draft",async()=>{
  let calls=0;const {store}=fixture("failed",{submit:async q=>{calls++;return {conversation_id:"wrong",run_id:"new",idempotency_key:q.idempotency_key,user_message_sequence:3,state:"queued",duplicate:false};}});await store.connect();await store.select("a");store.draft("keep");await store.retry("a","old");await store.send();expect(calls).toBe(1);expect(store.snapshot().retries[0]?.phase).toBe("unknown");expect(store.snapshot().draft).toBe("keep");store.newDraft();
});
test("workspace renders explicit retry action and honest window-only lineage",async()=>{
  const {createElement}=await import("react");const {renderToStaticMarkup}=await import("react-dom/server");const {ConversationWorkspace}=await import("../src/conversations/Workspace");
  const {store}=fixture();await store.connect();await store.select("a");
  const render=()=>renderToStaticMarkup(createElement(ConversationWorkspace,{controller:store,state:store.snapshot(),navigate:()=>{}}));
  expect(render()).toContain("Retry with selected route");await store.retry("a","old");const html=render();expect(html).toContain("Retry of attempt old");expect(html).toContain("new attempt new");expect(html).toContain("Relationship recorded in this window only");store.newDraft();
});
test("user-only terminal attempts render exactly one retry action",async()=>{
  const {createElement}=await import("react");const {renderToStaticMarkup}=await import("react-dom/server");const {ConversationWorkspace}=await import("../src/conversations/Workspace");
  const {store,messages}=fixture("cancelled");messages.pop();await store.connect();await store.select("a");const html=renderToStaticMarkup(createElement(ConversationWorkspace,{controller:store,state:store.snapshot(),navigate:()=>{}}));expect(html.match(/Retry with selected route/g)).toHaveLength(1);store.newDraft();
});
test("unknown retry blocks only affected conversation and exact-key lookup reconciles without replay",async()=>{
  let key="",lookups=0,submits=0;let lookup:V4TurnSubmission|null=null;
  const {store}=fixture("failed",{submit:async q=>{submits++;key=q.idempotency_key;throw {kind:"transport"};},lookupTurn:async(cid,k)=>{lookups++;expect(cid).toBe("a");expect(k).toBe(key);return lookup;}});
  await store.connect();await store.select("a");await store.retry("a","old");expect(store.snapshot().retries[0]?.idempotencyKey).toBe(key);await store.select("a");expect(store.snapshot().retries[0]?.phase).toBe("unknown");
  await store.select("b");store.draft("new B");await store.send();expect(submits).toBe(2); // Other conversation is independent.
  lookup={conversation_id:"a",idempotency_key:store.snapshot().retries[0]!.idempotencyKey,run_id:"recovered",user_message_sequence:3,state:"failed",duplicate:false};key=lookup.idempotency_key;
  await store.select("a");expect(store.snapshot().retries[0]).toMatchObject({phase:"accepted",runId:"recovered",state:"failed"});expect(submits).toBe(2);expect(lookups).toBeGreaterThan(0);store.newDraft();
});
test("lookup rejects wrong exact key and conversation, null/error never clears unknown",async()=>{
  let response:V4TurnSubmission|null=null;let key="";const {store}=fixture("failed",{submit:async q=>{key=q.idempotency_key;throw {kind:"transport"};},lookupTurn:async()=>response});await store.connect();await store.select("a");await store.retry("a","old");
  for(const [conversation_id,idempotency_key] of [["wrong",key],["a","wrong"]]){response={conversation_id:conversation_id!,idempotency_key:idempotency_key!,run_id:"new",user_message_sequence:3,state:"failed",duplicate:false};await store.select("a");expect(store.snapshot().retries[0]?.phase).toBe("unknown");}
  response=null;await store.connect();expect(store.snapshot().retries[0]?.phase).toBe("unknown");store.newDraft();
});
test("normal send ambiguity survives independent sends and plaintext cannot prove acceptance",async()=>{
  let result:V4TurnSubmission|null=null;let keyA="",calls=0;
  const {store,messages}=fixture("failed",{submit:async q=>{calls++;if(q.conversation_id==="a"){keyA=q.idempotency_key;throw {kind:"transport"};}return {conversation_id:q.conversation_id,run_id:"b-run",idempotency_key:q.idempotency_key,user_message_sequence:1,state:"queued",duplicate:false};},lookupTurn:async()=>result});
  await store.connect();await store.select("a");store.draft("same");await store.send();messages[0]!.text="same";messages[0]!.created_at_ms=Date.now()+1;await store.select("a");expect(store.snapshot().sendUnknownFor).toBe("a");
  await store.select("b");store.draft("B");await store.send();await store.select("a");expect(store.snapshot().sendUnknownFor).toBe("a");await store.send();expect(calls).toBe(2);
  for(const [cid,key] of [["wrong",keyA],["a","wrong"]]){result={conversation_id:cid!,run_id:"accepted",idempotency_key:key!,user_message_sequence:3,state:"failed",duplicate:false};await store.select("a");expect(store.snapshot().sendUnknownFor).toBe("a");}
  store.draft("same"); // A newly edited identical draft must survive reconciliation.
  result={conversation_id:"a",run_id:"accepted",idempotency_key:keyA,user_message_sequence:3,state:"failed",duplicate:false};await store.select("a");expect(store.snapshot().sendUnknownFor).toBeNull();expect(store.snapshot().draft).toBe("same");expect(calls).toBe(2);store.newDraft();
});
test("late exact lookup cannot clobber another conversation or a newly edited draft",async()=>{
  let key="";let resolve!:(value:V4TurnSubmission|null)=>void;let pending=false;
  const {store}=fixture("failed",{submit:async q=>{key=q.idempotency_key;throw {kind:"transport"};},lookupTurn:async()=>pending ? new Promise(done=>{resolve=done;}) : null});
  await store.connect();await store.select("a");store.draft("A");await store.send();pending=true;const reading=store.select("a");await new Promise(done=>setTimeout(done,0));await store.select("b");store.draft("B");resolve({conversation_id:"a",run_id:"new",idempotency_key:key,user_message_sequence:3,state:"failed",duplicate:false});await reading;expect(store.snapshot().selectedId).toBe("b");expect(store.snapshot().draft).toBe("B");pending=false;await store.select("a");expect(store.snapshot().draft).toBe("");expect(store.snapshot().sendUnknownFor).toBeNull();store.newDraft();
});
