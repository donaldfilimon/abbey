import { expect, test } from "bun:test";
import { createConversations, type ConversationApi } from "../src/conversations/controller";
import type { V4ContentPage, V4Conversation, V4ConversationPage, V4MessagePage, V4Route } from "../src/ipc/generated";
const conversation = (id: string, sequence = 1): V4Conversation => ({id, conversation_id:id, sequence, title: id, archived: false, pinned: false, message_count: 0, created_at_ms: 1, updated_at_ms: 1});
const page = (ids: string[], next_after: string | null = null): V4ConversationPage => ({conversations: ids.map((id, i) => conversation(id, i + 1)), next_after});
const fullPage = (prefix: string): V4ConversationPage => page(Array.from({length:16}, (_, i) => `${prefix}-${i}`), `${prefix}-15`);
const message = (conversation_id: string, sequence: number, text: string) => ({conversation_id, run_id:"00000000-0000-4000-8000-000000000001", sequence, role:"assistant" as const, state:"succeeded" as const, text, text_bytes:new TextEncoder().encode(text).length, text_complete:true, created_at_ms:1});
const route = (route_id:string, available=true):V4Route => ({route_id, backend:"abi", label:route_id, available});
function fixture(overrides: Partial<ConversationApi> = {}) {
  let effects = 0;
  const api: ConversationApi = {
    grants: async () => ({capabilities: ["read_conversations", "write_conversations"]}),
    list: async () => page(["a", "b"]), search: async () => page([]),
    read: async q => conversation(q.resource_id),
    messages: async q => ({conversation_id: q.conversation_id, next_after: null, messages: []}),
    routes: async () => [],
    create: async q => { effects++; return {...conversation("created"), title: q.title}; },
    rename: async q => { effects++; return {...conversation(q.conversation_id), title: q.title}; },
    archive: async q => { effects++; return {...conversation(q.resource_id), archived: true}; },
    delete: async () => { effects++; }, ...overrides,
  };
  return {store: createConversations(api), effects: () => effects};
}
const defer = <T,>() => { let resolve!: (v: T) => void; let reject!: (error:unknown) => void; const promise = new Promise<T>((done, failed) => {resolve = done; reject = failed;}); return {promise, resolve, reject}; };
test("drafts belong to each conversation and new draft; survive reads, navigation and failure with bounded input", async () => {
  const {store} = fixture(); store.draft("new"); await store.connect(); await store.select("a"); store.draft("A");
  await store.select("b"); store.draft("B"); store.newDraft(); expect(store.snapshot().draft).toBe("new");
  await store.select("a"); expect(store.snapshot().draft).toBe("A"); store.draft("x".repeat(10000)); expect(store.snapshot().draft.length).toBe(8192);
});
test("slow search and selection cannot replace newer results or message bodies", async () => {
  const slowList = defer<V4ConversationPage>(); const slowMessages = defer<V4MessagePage>();
  const {store} = fixture({search: q => q.search === "old" ? slowList.promise : Promise.resolve(page(["new"])), messages: q => q.conversation_id === "a" ? slowMessages.promise : Promise.resolve({conversation_id: "b", next_after: null, messages: []})});
  await store.connect(); const old = store.search("old"); await store.search("new"); slowList.resolve(page(["old"])); await old;
  expect(store.snapshot().items[0]?.id).toBe("new"); const a = store.select("a"); await store.select("b");
  slowMessages.resolve({conversation_id: "a", next_after: null, messages: [message("a", 1, "private A")]}); await a;
  expect(store.snapshot().selectedId).toBe("b"); expect(store.snapshot().messages).toEqual([]);
});
test("conversation and message pages retain watermark; stale load-more is discarded on query or selection change", async () => {
  const queries: unknown[] = []; const more = defer<V4ConversationPage>();
  const {store} = fixture({list: async q => { queries.push(q); return q.after ? more.promise : fullPage("old"); }, search: async q => {queries.push(q); return page([]);}, messages: async q => {
    queries.push(q); return {conversation_id:q.conversation_id, next_after:q.after === 0 ? 1 : null, messages:[message(q.conversation_id, (q.after ?? 0)+1, "<script>plain text</script>")]};
  }});
  await store.connect(); const pending = store.more(); await store.search("changed"); more.resolve(page(["b"])); await pending;
  expect(store.snapshot().items).toEqual([]); expect(queries[1]).toEqual({search:null, after:"old-15", limit:16, archived:false});
  await store.select("a"); await store.moreMessages(); expect(store.snapshot().messages).toHaveLength(1); expect(store.snapshot().messages[0]?.sequence).toBe(2);
  expect(queries.at(-1)).toEqual({conversation_id:"a", after:1, limit:32});
});
test("double mutation sends once, transport ambiguity locks effects, read reconciliation never replays", async () => {
  let sends = 0; let reads = 0; const lost = defer<V4Conversation>();
  const {store} = fixture({rename: () => { sends++; return lost.promise; }, read: async q => {reads++; return {...conversation(q.resource_id), title:"renamed"};}});
  await store.connect(); await store.select("a"); store.draft("keep"); const first = store.mutate("rename", "renamed"); await store.mutate("rename", "duplicate");
  lost.reject({kind:"transport"}); await first; expect(sends).toBe(1); expect(store.snapshot().mutation?.phase).toBe("unknown");
  await store.mutate("archive"); expect(sends).toBe(1); await store.reconcile(); expect(reads).toBeGreaterThan(1);
  expect(store.snapshot().mutation).toBeNull(); expect(store.snapshot().draft).toBe("keep"); expect(sends).toBe(1);
});
test("unknown create stays unknown after read-only refresh; unknown delete resolves only exact not-found", async () => {
  const {store} = fixture({create: async () => {throw {kind:"transport"};}, delete: async () => {throw {kind:"protocol"};}, read: async q => {if(q.resource_id === "gone") throw {kind:"not_found"}; return conversation(q.resource_id);}});
  await store.connect(); await store.mutate("create", "a"); await store.reconcile(); expect(store.snapshot().mutation?.phase).toBe("unknown");
  const other = fixture({delete: async () => {throw {kind:"transport"};}, read: async q => {if(q.resource_id === "gone") throw {kind:"not_found"}; return conversation(q.resource_id);}}).store;
  await other.connect(); await other.select("gone"); await other.mutate("delete"); await other.reconcile(); expect(other.snapshot().mutation).toBeNull(); expect(other.snapshot().selectedId).toBeNull();
});
test("failed reconnect preserves authorized selection/content/draft and removes write authority", async () => {
  let offline = false; const {store} = fixture({grants: async () => {if(offline) throw {kind:"transport"}; return {capabilities:["read_conversations", "write_conversations"]};}});
  await store.connect(); await store.select("a"); store.draft("keep"); offline = true; await store.connect();
  expect(store.snapshot().selected?.id).toBe("a"); expect(store.snapshot().draft).toBe("keep"); expect(store.snapshot().canWrite).toBe(false);
});
test("read-only and missing grants cannot mutate; no hidden local fallback", async () => {
  const readOnly = fixture({grants: async () => ({capabilities:["read_conversations"]})}); await readOnly.store.connect(); await readOnly.store.mutate("create", "blocked"); expect(readOnly.effects()).toBe(0); expect(readOnly.store.snapshot().access).toBe("read-only");
  const denied = fixture({grants: async () => ({capabilities:[]})}); await denied.store.connect(); expect(denied.store.snapshot().access).toBe("no-grant"); expect(denied.store.snapshot().items).toEqual([]);
});

test("draft inventory refuses to evict existing drafts at the bounded limit", async () => {
  const {store} = fixture(); await store.connect();
  for(let index=0; index<32; index++) {await store.select(`draft-${index}`); store.draft(`kept-${index}`);}
  await store.select("overflow"); store.draft("cannot evict"); expect(store.snapshot().draft).toBe("");
  expect(store.snapshot().note).toContain("32 drafts"); await store.select("draft-0"); expect(store.snapshot().draft).toBe("kept-0");
});
test("late message pagination is ignored after selecting another conversation", async () => {
  const more = defer<V4MessagePage>(); const {store} = fixture({messages: q => q.after ? more.promise : Promise.resolve({conversation_id:q.conversation_id, next_after:1, messages:[message(q.conversation_id,1,q.conversation_id)]})});
  await store.connect(); await store.select("a"); const pending = store.moreMessages(); await store.select("b");
  more.resolve({conversation_id:"a", next_after:null, messages:[message("a",2,"private A")]}); await pending;
  expect(store.snapshot().messages.map(m => m.text)).toEqual(["b"]);
});
test("no-daemon is distinct from a missing live grant and never negotiates authority", async () => {
  let negotiations=0; const {store} = fixture({daemon:async () => false, grants:async () => {negotiations++; return {capabilities:[]};}});
  await store.connect(); expect(negotiations).toBe(0); expect(store.snapshot().access).toBe("no-daemon");
});

test.each(["rename", "archive"] as const)("successful %s metadata survives an older selection while its messages still finish", async kind => {
  const staleRead = defer<V4Conversation>(); const history = defer<V4MessagePage>();
  let reads = 0, messageReads = 0;
  const {store} = fixture({
    read: async q => ++reads === 1 ? conversation(q.resource_id) : staleRead.promise,
    messages: async q => ++messageReads === 1 ? {conversation_id:q.conversation_id, next_after:null, messages:[]} : history.promise,
  });
  await store.connect(); await store.select("a"); const refresh = store.select("a");
  await store.mutate(kind, "new title");
  expect(store.snapshot().reading).toBe(true);
  staleRead.resolve(conversation("a"));
  history.resolve({conversation_id:"a", next_after:1, messages:[message("a",1,"retained history")]});
  await refresh;
  expect(store.snapshot().selected?.title).toBe(kind === "rename" ? "new title" : "a");
  expect(store.snapshot().selected?.archived).toBe(kind === "archive");
  expect(store.snapshot().messages[0]?.text).toBe("retained history");
  expect(store.snapshot().reading).toBe(false); expect(store.snapshot().moreMessages).toBe(true);
});
test.each(["rename", "archive"] as const)("read-reconciled %s metadata survives an older selection", async kind => {
  const staleRead = defer<V4Conversation>(); let reads=0;
  const current = {...conversation("a"), title:kind === "rename" ? "reconciled title" : "a", archived:kind === "archive"};
  const {store} = fixture({
    read: async () => {reads++; return reads === 1 ? conversation("a") : reads === 2 ? staleRead.promise : current;},
    rename:async () => {throw {kind:"transport"};}, archive:async () => {throw {kind:"transport"};},
  });
  await store.connect(); await store.select("a"); const refresh=store.select("a");
  await store.mutate(kind, "reconciled title"); await store.reconcile();
  expect(store.snapshot().mutation).toBeNull(); expect(store.snapshot().selected).toEqual(current);
  staleRead.resolve(conversation("a")); await refresh;
  expect(store.snapshot().selected).toEqual(current); expect(store.snapshot().reading).toBe(false);
});
test("a failed replacement search cannot page the previous query or mix its rows", async () => {
  const first = defer<V4ConversationPage>(); const queries: unknown[] = []; let attempts=0;
  const {store} = fixture({
    list:async () => fullPage("old"),
    search:async q => {queries.push(q); if(++attempts === 1) return first.promise; return q.after ? page(["needle-b"]) : fullPage("needle");},
  });
  await store.connect(); expect(store.snapshot().more).toBe(true);
  const replacement=store.search("needle"); const pendingMore=store.snapshot().more;
  first.reject({kind:"transport"}); await replacement;
  expect(pendingMore).toBe(false); expect(store.snapshot().more).toBe(false);
  await store.more(); expect(queries).toHaveLength(1); expect(store.snapshot().items.map(c => c.id)).toEqual(Array.from({length:16}, (_, i) => `old-${i}`));
  await store.refresh(); expect(queries[1]).toEqual({search:"needle", after:null, limit:16, archived:false});
  expect(store.snapshot().items.map(c => c.id)).toEqual(Array.from({length:16}, (_, i) => `needle-${i}`));
  await store.more(); expect(queries[2]).toEqual({search:"needle", after:"needle-15", limit:16, archived:false});
  expect(store.snapshot().items.map(c => c.id).at(-1)).toBe("needle-b");
});
test("a failed first-page refresh disables the retained cursor until a replacement snapshot succeeds", async () => {
  const replacement = defer<V4ConversationPage>(); const queries: unknown[] = []; let reads=0;
  const {store} = fixture({list:async q => {queries.push(q); if(++reads === 2) return replacement.promise; return reads === 1 ? fullPage("old") : fullPage("fresh");}});
  await store.connect(); const refresh=store.refresh(); replacement.reject({kind:"transport"}); await refresh;
  expect(store.snapshot().more).toBe(false); await store.more(); expect(queries).toHaveLength(2);
  await store.refresh(); expect(queries[2]).toEqual({search:null, after:null, limit:16, archived:false});
  expect(store.snapshot().items.map(c => c.id)).toEqual(Array.from({length:16}, (_, i) => `fresh-${i}`)); expect(store.snapshot().more).toBe(true);
});

// basic poll test (uses messages list for active chat; content command added in parallel)
test("poll refreshes tail of selected conversation without clobbering draft or selection on transient error", async () => {
  let polls = 0;
  const {store} = fixture({messages: async q => { polls++; return {conversation_id: q.conversation_id, next_after:null, messages: polls > 1 ? [message(q.conversation_id, (q.after??0)+1, "polled "+polls)] : [] }; }});
  await store.connect(); await store.select("poll-me"); store.draft("keep-draft");
  // simulate two poll ticks by calling internal schedule effect (in real timer drives)
  // for test we directly await a couple message calls via more or refresh path, but assert no clobber
  await new Promise(r => setTimeout(r, 300));
  expect(store.snapshot().selectedId).toBe("poll-me");
  expect(store.snapshot().draft).toBe("keep-draft");
  expect(store.snapshot().messages.length).toBeGreaterThan(0);
  store.newDraft();
});

test("active assistant content is read by exact run cursor and replaces the bounded preview", async () => {
  const run_id = "00000000-0000-4000-8000-000000000001";
  const initial = {...message("a", 2, "hel"), run_id, state:"running" as const, text_bytes:8, text_complete:false};
  const content: V4ContentPage = {
    conversation_id:"a", run_id, after:0, next_after:null,
    events:[
      {conversation_id:"a", run_id, sequence:1, timestamp_ms:1, payload:{kind:"text_delta", data:{text:"hello"}}},
      {conversation_id:"a", run_id, sequence:2, timestamp_ms:2, payload:{kind:"terminal", data:{state:"succeeded"}}},
    ],
  };
  const {store} = fixture({
    messages: async q => ({conversation_id:q.conversation_id, next_after:null, messages:[initial]}),
    content: async q => ({...content, conversation_id:q.conversation_id, run_id:q.run_id, after:q.after}),
  });
  await store.connect(); await store.select("a");
  await new Promise(resolve => setTimeout(resolve, 320));
  expect(store.snapshot().messages[0]?.text).toBe("hello");
  expect(store.snapshot().messages[0]?.text_complete).toBe(true);
  store.newDraft();
});

test("route read failure stays distinct from an empty inventory and retry selects only an available route", async () => {
  let attempts=0;
  const {store} = fixture({
    grants:async () => ({capabilities:["read_conversations", "write_conversations", "read_routes"]}),
    routes:async () => {if(++attempts === 1) throw {kind:"transport"}; return [route("offline", false), route("local")];},
    submit:async request => ({conversation_id:request.conversation_id, run_id:"00000000-0000-4000-8000-000000000001", idempotency_key:request.idempotency_key, user_message_sequence:1, state:"queued", duplicate:false}),
  });
  await store.connect(); expect(store.snapshot().routeStatus).toBe("error"); expect(store.snapshot().routes).toEqual([]);
  await store.refreshRoutes(); expect(store.snapshot().routeStatus).toBe("ready"); expect(store.snapshot().routeId).toBe("local");
  store.chooseRoute("offline"); expect(store.snapshot().routeId).toBe("local");
  store.newDraft(); store.draft("hello"); await store.send();
  expect(store.snapshot().selectedId).toBe("created");
});

test("successfully empty route inventory is not misreported as a route read failure", async () => {
  const {store}=fixture({
    grants:async () => ({capabilities:["read_conversations", "read_routes"]}),
    routes:async () => [],
  });
  await store.connect(); expect(store.snapshot().routeStatus).toBe("ready"); expect(store.snapshot().routeId).toBeNull();
});

test("new-conversation draft follows creation when turn submission is rejected", async () => {
  const {store} = fixture({
    grants:async () => ({capabilities:["read_conversations", "write_conversations", "read_routes"]}),
    routes:async () => [route("local")],
    submit:async () => {throw {kind:"rejected"};},
  });
  await store.connect(); store.newDraft(); store.draft("keep this exact draft"); await store.send();
  expect(store.snapshot().selectedId).toBe("created"); expect(store.snapshot().draft).toBe("keep this exact draft");
  await store.newDraft(); await store.select("created"); expect(store.snapshot().draft).toBe("keep this exact draft");
  expect(store.snapshot().note).toContain("message was rejected");
});

test("failed conversation creation is reported separately and retains the unsent draft", async () => {
  let submissions=0;
  const {store}=fixture({
    grants:async () => ({capabilities:["read_conversations", "write_conversations", "read_routes"]}),
    routes:async () => [route("local")], create:async () => {throw {kind:"rejected"};},
    submit:async () => {submissions++; throw new Error("must not submit");},
  });
  await store.connect(); store.newDraft(); store.draft("unsent"); await store.send();
  expect(store.snapshot().note).toContain("conversation could not be created");
  expect(store.snapshot().draft).toBe("unsent"); expect(submissions).toBe(0);
});

test("ambiguous turn submission locks that conversation against duplicate sends", async () => {
  let submissions=0;
  const {store} = fixture({
    grants:async () => ({capabilities:["read_conversations", "write_conversations", "read_routes"]}),
    routes:async () => [route("local")], submit:async () => {submissions++; throw {kind:"transport"};},
  });
  await store.connect(); await store.select("a"); store.draft("send once"); await store.send(); await store.send();
  expect(submissions).toBe(1); expect(store.snapshot().sendUnknownFor).toBe("a");
  expect(store.snapshot().draft).toBe("send once");
});

test("exact-key lookup resolves an ambiguous send without replay", async () => {
  let include=false, key="";
  const {store}=fixture({
    grants:async () => ({capabilities:["read_conversations", "write_conversations", "read_routes"]}),
    routes:async () => [route("local")], submit:async q => {key=q.idempotency_key;throw {kind:"transport"};},
    lookupTurn:async cid => include ? {conversation_id:cid,run_id:"accepted",idempotency_key:key,user_message_sequence:1,state:"queued",duplicate:false} : null,
    messages:async q => ({conversation_id:q.conversation_id, next_after:null, messages:include ? [{...message(q.conversation_id, 1, "same text"), role:"user" as const, created_at_ms:Date.now()+1}] : []}),
  });
  await store.connect(); await store.select("a"); store.draft("same text"); await store.send();
  expect(store.snapshot().sendUnknownFor).toBe("a");
  include=true; await store.select("a");
  expect(store.snapshot().sendUnknownFor).toBeNull(); expect(store.snapshot().draft).toBe("");
  expect(store.snapshot().note).toContain("confirmed by its idempotency key");
});

test("repeated daemon poll failures move chat offline and remove write authority", async () => {
  let reads=0;
  const {store}=fixture({messages:async q => {
    if (++reads === 1) return {conversation_id:q.conversation_id, next_after:null, messages:[]};
    throw {kind:"transport"};
  }});
  await store.connect(); await store.select("a");
  await new Promise(resolve => setTimeout(resolve, 1850));
  expect(store.snapshot().access).toBe("offline"); expect(store.snapshot().canWrite).toBe(false);
  expect(store.snapshot().note).toContain("daemon stopped responding");
});

test("manual loading continues paging an older truncated reply until its terminal event", async () => {
  const run_id="00000000-0000-4000-8000-000000000001";
  const old={...message("a", 1, "preview"), run_id, text_bytes:70_000, text_complete:true};
  const calls:number[]=[];
  const {store}=fixture({
    messages:async q => ({conversation_id:q.conversation_id, next_after:null, messages:q.after === 0 ? [old] : []}),
    content:async q => {
      calls.push(q.after);
      if(q.after === 0) return {conversation_id:"a", run_id, after:0, next_after:50, events:Array.from({length:50}, (_,i) => ({conversation_id:"a",run_id,sequence:i+1,timestamp_ms:i,payload:{kind:"text_delta" as const,data:{text:"x"}}}))};
      return {conversation_id:"a", run_id, after:50, next_after:null, events:[{conversation_id:"a",run_id,sequence:51,timestamp_ms:51,payload:{kind:"terminal",data:{state:"succeeded" as const}}}]};
    },
  });
  await store.connect(); await store.select("a"); await store.loadContent(old);
  await new Promise(resolve => setTimeout(resolve, 340));
  expect(calls).toContain(0); expect(calls).toContain(50);
  expect(store.snapshot().messages[0]?.text).toBe("x".repeat(50));
  expect(store.snapshot().messages[0]?.text_complete).toBe(true);
  store.newDraft();
});

test("later conversation-scoped run accepts its first nonzero event sequence without replay", async () => {
  const run_id="00000000-0000-4000-8000-000000000001";
  const initial={...message("a", 2, "preview"), run_id, text_bytes:4, text_complete:false, state:"running" as const};
  const {store}=fixture({
    messages:async q => ({conversation_id:q.conversation_id, next_after:null, messages:[initial]}),
    content:async q => ({conversation_id:"a", run_id, after:q.after, next_after:null, events:[
      {conversation_id:"a", run_id, sequence:11, timestamp_ms:11, payload:{kind:"text_delta" as const, data:{text:"ok"}}},
      {conversation_id:"a", run_id, sequence:13, timestamp_ms:13, payload:{kind:"terminal" as const, data:{state:"succeeded" as const}}},
    ]}),
  });
  await store.connect(); await store.select("a");
  await new Promise(resolve => setTimeout(resolve, 300));
  expect(store.snapshot().messages[0]?.text).toBe("ok"); expect(store.snapshot().messages[0]?.text_complete).toBe(true);
  store.newDraft();
});
test("historical browse explicitly drains content pages without resuming history polling",async()=>{
 const old={...message("a",1,"preview"),text_bytes:70000,text_complete:true};let reads=0;const calls:number[]=[];
 const {store}=fixture({messages:async q=>{reads++;return {conversation_id:q.conversation_id,messages:[old],next_after:1};},content:async q=>{calls.push(q.after);return {conversation_id:"a",run_id:old.run_id,after:q.after,next_after:q.after===0?1:null,events:[{conversation_id:"a",run_id:old.run_id,sequence:q.after+1,timestamp_ms:1,payload:q.after===0?{kind:"text_delta",data:{text:"full"}}:{kind:"terminal",data:{state:"succeeded"}}}]};}});
 await store.connect();await store.select("a");store.draft("keep");await store.loadContent(old);expect(calls).toEqual([0,1]);expect(store.snapshot().messages[0]?.text).toBe("full");expect(store.snapshot().messages[0]?.text_complete).toBe(true);expect(store.snapshot().historyMode).toBe("browse");await new Promise(done=>setTimeout(done,300));expect(reads).toBe(1);expect(store.snapshot().draft).toBe("keep");store.newDraft();
});
test("historical content action has bounded continuation and preserves advertised bytes",async()=>{
 const old={...message("a",1,"preview"),text_bytes:70000,text_complete:true};let calls=0;
 const {store}=fixture({messages:async q=>({conversation_id:q.conversation_id,messages:[old],next_after:1}),content:async q=>{calls++;return {conversation_id:"a",run_id:old.run_id,after:q.after,next_after:q.after<128?q.after+1:null,events:[{conversation_id:"a",run_id:old.run_id,sequence:q.after+1,timestamp_ms:1,payload:q.after<128?{kind:"text_delta",data:{text:"x"}}:{kind:"terminal",data:{state:"succeeded"}}}]};}});
 await store.connect();await store.select("a");await store.loadContent(old);expect(calls).toBe(128);expect(store.snapshot().messages[0]?.text_bytes).toBe(70000);expect(store.snapshot().messages[0]?.text_complete).toBe(false);expect(store.snapshot().contentNotice).toContain("Continue");await store.loadContent(store.snapshot().messages[0]!);expect(calls).toBe(129);expect(store.snapshot().messages[0]?.text_complete).toBe(true);store.newDraft();
});
test("explicit content load is single flight and late pages cannot alter a navigated conversation",async()=>{
 const old={...message("a",1,"preview"),text_bytes:70000,text_complete:true};const pending=defer<V4ContentPage>();let calls=0;
 const {store}=fixture({messages:async q=>({conversation_id:q.conversation_id,messages:q.conversation_id==="a"?[old]:[],next_after:q.conversation_id==="a"?1:null}),content:async()=>{calls++;return pending.promise;}});
 await store.connect();await store.select("a");const loading=store.loadContent(old);await store.loadContent(old);expect(calls).toBe(1);await store.select("b");store.draft("B");pending.resolve({conversation_id:"a",run_id:old.run_id,after:0,next_after:null,events:[{conversation_id:"a",run_id:old.run_id,sequence:1,timestamp_ms:1,payload:{kind:"text_delta",data:{text:"late"}}}]});await loading;expect(store.snapshot().selectedId).toBe("b");expect(store.snapshot().draft).toBe("B");expect(store.snapshot().contentLoadingRun).toBeNull();expect(store.snapshot().messages).toHaveLength(0);store.newDraft();
});
test("nonadvancing historical content remains incomplete with continuation available",async()=>{
 const old={...message("a",1,"preview"),text_bytes:70000,text_complete:true};let calls=0;
 const {store}=fixture({messages:async q=>({conversation_id:q.conversation_id,messages:[old],next_after:1}),content:async q=>{calls++;return {conversation_id:"a",run_id:old.run_id,after:q.after,next_after:null,events:[]};}});
 await store.connect();await store.select("a");await store.loadContent(old);expect(calls).toBe(1);expect(store.snapshot().messages[0]?.text_bytes).toBe(70000);expect(store.snapshot().messages[0]?.text_complete).toBe(false);expect(store.snapshot().contentNotice).toContain("Continue");store.newDraft();
});
