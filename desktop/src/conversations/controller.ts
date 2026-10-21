import type { RunState, V4CapabilitySet, V4ContentPage, V4Conversation, V4ConversationCreate, V4ConversationPatch, V4ConversationPage, V4ConversationQuery, V4ConversationRename, V4Message, V4MessagePage, V4MessageQuery, V4ResourceQuery, V4Route, V4SubmitTurn, V4TurnSubmission } from "../ipc/generated";
export interface ConversationApi {
  daemon?: () => Promise<boolean>;
  grants: () => Promise<V4CapabilitySet>;
  list: (q: V4ConversationQuery) => Promise<V4ConversationPage>;
  search: (q: V4ConversationQuery) => Promise<V4ConversationPage>;
  read: (q: V4ResourceQuery) => Promise<V4Conversation>;
  messages: (q: V4MessageQuery) => Promise<V4MessagePage>;
  content?: (q: {conversation_id:string; run_id:string; after:number; limit:number}) => Promise<V4ContentPage>;
  create: (q: V4ConversationCreate) => Promise<V4Conversation>;
  update?: (conversationId:string, patch:V4ConversationPatch) => Promise<V4Conversation>;
  rename: (q: V4ConversationRename) => Promise<V4Conversation>;
  archive: (q: V4ResourceQuery) => Promise<V4Conversation>;
  delete: (q: V4ResourceQuery) => Promise<void>;
  routes: () => Promise<V4Route[]>;
  submit: (request: V4SubmitTurn) => Promise<V4TurnSubmission>;
  lookupTurn?: (conversationId:string, idempotencyKey:string) => Promise<V4TurnSubmission | null>;
  cancel?: (conversationId:string, runId:string) => Promise<RunState>;
}
export type MutationKind = "create" | "rename" | "archive" | "delete";
interface Mutation { kind: MutationKind | "update"; patch?:V4ConversationPatch; id: string | null; title: string; phase: "sending" | "unknown" }
interface StopState { conversationId:string; runId:string; phase:"sending" | "requested" | "unknown" | "rejected" | "settled"; note:string }
const activeState = (value:RunState) => ["queued", "starting", "running", "cancel_requested"].includes(value);
interface RetryAttempt { idempotencyKey:string; state:RunState | null; conversationId:string; originalRunId:string; runId:string | null; routeId:string; phase:"sending" | "accepted" | "unknown" | "rejected" }
interface Snapshot {
  retries: RetryAttempt[]; canUpdate:boolean; archived:boolean;
  access: "loading" | "ready" | "read-only" | "no-daemon" | "no-grant" | "offline";
  canRead: boolean; canWrite: boolean; canCancel:boolean; activeRunId:string | null; stop:StopState | null; query: string; items: V4Conversation[];
  listing: boolean; more: boolean; selectedId: string | null; selected: V4Conversation | null;
  contentLoadingRun:string|null; contentNotice:string|null;
  historyMode:"browse" | "following"; earlierMessages:boolean; seekingLatest:boolean; historyProgress:number;
  messages: V4Message[]; reading: boolean; moreMessages: boolean; draft: string;
  mutation: Mutation | null; note: string | null;
  routes: V4Route[]; routeId: string | null; sending: boolean;
  routeStatus: "loading" | "ready" | "unavailable" | "error";
  sendUnknownFor: string | null;
  sendUnknownText: string | null;
  sendUnknownAt: number | null;
}
const PAGE = 16;
const MESSAGE_PAGE = 32;
const kindOf = (error: unknown) => typeof error === "object" && error !== null && "kind" in error ? error.kind : null;
// One controller per application window. Never persisted or logged. No timers/retries.
export function createConversations(api: ConversationApi) {
  let state: Snapshot = {contentLoadingRun:null,contentNotice:null,historyMode:"browse",earlierMessages:false,seekingLatest:false,historyProgress:0,retries:[], canUpdate:!!api.update, archived:false, access:"loading", canRead:false, canWrite:false, canCancel:false, activeRunId:null, stop:null, query:"", items:[], listing:false, more:false, selectedId:null, selected:null, messages:[], reading:false, moreMessages:false, draft:"", mutation:null, note:null, routes:[], routeId:null, routeStatus:"loading", sendUnknownFor:null, sendUnknownText:null, sendUnknownAt:null, sending:false};
  const stops = new Map<string, StopState>();
  const listeners = new Set<() => void>();
  const drafts = new Map<string, string>();
  const draftVersions = new Map<string, number>();
  const unknownSends = new Map<string, {key:string; input:string; at:number; draftVersion:number}>();
  let accessGeneration = 0, listGeneration = 0, readGeneration = 0, metadataGeneration = 0, pollGeneration = 0;
  let pollFailures = 0, quietPolls = 0, pollDelay = 250;
  // A cursor belongs only to the successful first page of its exact query.
  // Conversation list uses ID cursors (after: ConvId, next_after); messages use numeric sequence.
  let listSnapshot: {query: string; archived:boolean; after: string | null} | null = null;
  let messageAfter = 0;
  let windowAfter=0, windowNext:number|null=null;
  let previousWindows:number[]=[];
  const originalInputs = new Map<string,V4Message>();
  const inputKey=(conversationId:string,runId:string)=>JSON.stringify([conversationId,runId]);
  const contentByRun = new Map<string, {after:number; text:string; done:boolean; more:boolean}>();
  const contentTargets = new Map<string, {message:V4Message; conversationId:string}>();
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  const update = (patch: Partial<Snapshot>) => {
    state = {...state, ...patch};
    const unknown=state.selectedId ? unknownSends.get(state.selectedId) : undefined;
    state={...state,sendUnknownFor:unknown ? state.selectedId : null,sendUnknownText:unknown?.input ?? null,sendUnknownAt:unknown?.at ?? null};
    const runs = new Map<string, V4Message>();
    for (const message of state.messages) {
      if (message.conversation_id === state.selectedId) runs.set(message.run_id, message);
    }
    for (const message of runs.values()) {
      const stopped = stops.get(message.run_id);
      if (stopped && !activeState(message.state)) stops.set(message.run_id, {...stopped, phase:"settled", note:`Reply ${message.state} (confirmed by conversation history).`});
    }
    const active = [...runs.values()].reverse().find(message => activeState(message.state) && stops.get(message.run_id)?.phase !== "settled");
    const visibleRun = active?.run_id ?? [...runs.keys()].at(-1);
    const stop = visibleRun ? stops.get(visibleRun) ?? null : null;
    state = {...state, activeRunId:active?.run_id ?? null, stop};
    for (const fn of listeners) fn();
  };
  const stopPoll = () => { if (pollTimer) { clearTimeout(pollTimer); pollTimer = null; } };
  const readContent = async (message: V4Message, conversationId: string, generation: number): Promise<boolean> => {
    if (!api.content || message.role !== "assistant" || generation !== pollGeneration || state.selectedId !== conversationId) return false;
    const saved = contentByRun.get(message.run_id) ?? {after:0, text:"", done:false,more:true};
    if (saved.done) return false;
    try {
      const page = await api.content({conversation_id:conversationId, run_id:message.run_id, after:saved.after, limit:50});
      if (generation !== pollGeneration || state.selectedId !== conversationId || page.conversation_id !== conversationId || page.run_id !== message.run_id || page.after !== saved.after) return false;
      if(page.events.length>50)return false;
      let text = saved.text;
      let after = saved.after;
      let terminal = false;
      let terminalState: RunState | null = null;
      for (const event of page.events) {
        // Event sequences are conversation-scoped, while this page is filtered
        // to one run; gaps can therefore be valid but replay/out-of-order is not.
        if (terminal || event.sequence>65536 || event.sequence <= after || event.run_id !== message.run_id || event.conversation_id !== conversationId) return false;
        after = event.sequence;
        if (event.payload.kind === "text_delta") {if(new TextEncoder().encode(event.payload.data.text).length>16*1024)return false;text += event.payload.data.text;}
        if (event.payload.kind === "terminal") { if(!["succeeded","failed","cancelled","interrupted"].includes(event.payload.data.state))return false;terminal = true; terminalState = event.payload.data.state; }
      }
      if (page.next_after !== null && page.next_after !== after) return false;
      const bytes = new TextEncoder().encode(text).length;
      if(bytes>4*1024*1024)return false;
      contentByRun.set(message.run_id, {after, text, done:terminal,more:page.next_after!==null});
      const messages = state.messages.map(current => current.run_id === message.run_id && current.role === "assistant" ? {...current, text, text_bytes:terminal ? bytes : Math.max(current.text_bytes,bytes), text_complete:terminal, ...(terminalState ? {state:terminalState} : {})} : current);
      update({messages});
      if (terminal) contentTargets.delete(message.run_id);
      return page.events.length > 0;
    } catch { /* Poll/reconnect retries this exact read cursor; no provider request is repeated. */ return false; }
  };
  const schedulePoll = () => {
    stopPoll();
    if (!state.selectedId || !state.canRead || state.historyMode!=="following" || state.reading) return;
    const id = state.selectedId;
    pollTimer = setTimeout(() => {
      pollTimer = null;
      const gen = ++pollGeneration;
      if (!state.selectedId || state.selectedId !== id || gen !== pollGeneration) return;
      void (async () => {
        try {
          let activity = false;
          const after=windowNext ?? windowAfter;
          const tail=await api.messages({conversation_id:id,after,limit:MESSAGE_PAGE});
          if(gen!==pollGeneration || state.selectedId!==id || state.historyMode!=="following")return;
          validateWindow(tail,id,after);
          activity=tail.messages.at(-1)?.sequence!==messageAfter;
          if(after!==windowAfter)previousWindows=[...previousWindows,windowAfter].slice(-256);
          installWindow(tail,after,"following");
          await reconcileUnknownSend(id);
          if(gen!==pollGeneration || state.selectedId!==id)return;
          for (const target of [...contentTargets.values()]) {
            if (target.conversationId === id && await readContent(target.message, id, gen)) activity = true;
          }
          pollFailures = 0;
          quietPolls = activity ? 0 : quietPolls + 1;
          pollDelay = quietPolls < 8 ? 250 : quietPolls < 30 ? 1000 : 4000;
        } catch {
          pollFailures++;
          pollDelay = Math.min(4000, Math.max(500, pollDelay * 2));
          if (pollFailures >= 3 && state.selectedId === id) update({access:"offline", canRead:false, canWrite:false, canCancel:false, note:"The daemon stopped responding. Existing content and drafts are kept; reconnect to continue."});
        }
        schedulePoll();
      })();
    }, pollDelay);
  };
  const validateWindow=(page:V4MessagePage,id:string,after:number) => {
    let previous=after;
    if(page.conversation_id!==id || page.messages.length>MESSAGE_PAGE)throw Error("Invalid message window");
    for(const message of page.messages){if(message.conversation_id!==id || message.sequence<=previous)throw Error("Invalid message order");previous=message.sequence;}
    if(page.next_after!==null && (page.messages.length===0 || page.next_after!==previous))throw Error("Invalid message cursor");
    for(const message of page.messages)if(message.role==="user" && message.text_complete && new TextEncoder().encode(message.text).length===message.text_bytes) {
      const key=inputKey(id,message.run_id);
      originalInputs.delete(key);originalInputs.set(key,{...message});
      if(originalInputs.size>64)originalInputs.delete(originalInputs.keys().next().value!);
    }
  };
  const installWindow=(page:V4MessagePage,after:number,mode:Snapshot["historyMode"]) => {
    windowAfter=after;windowNext=page.next_after;messageAfter=page.messages.at(-1)?.sequence ?? after;
    const visible=new Set(page.messages.map(message=>message.run_id));
    for(const key of contentByRun.keys())if(!visible.has(key))contentByRun.delete(key);
    contentTargets.clear();
    const messages=page.messages.map(message=>{const cached=contentByRun.get(message.run_id);return cached && message.role==="assistant" ? {...message,text:cached.text,text_bytes:Math.max(message.text_bytes,new TextEncoder().encode(cached.text).length),text_complete:cached.done} : message;});
    for(const message of messages)if(message.role==="assistant" && (!message.text_complete || message.text_bytes>new TextEncoder().encode(message.text).length))contentTargets.set(message.run_id,{message,conversationId:page.conversation_id});
    update({messages,reading:false,seekingLatest:false,earlierMessages:after>0,moreMessages:page.next_after!==null,historyMode:mode});
  };
  const moveWindow=async(direction:"earlier" | "later" | "latest") => {
    if(!state.canRead || !state.selectedId || state.reading || (direction==="earlier" && windowAfter===0) || (direction==="later" && windowNext===null))return;
    const id=state.selectedId,generation=++readGeneration;
    stopPoll();pollGeneration++;
    update({contentLoadingRun:null,contentNotice:null,reading:true,seekingLatest:direction==="latest",historyProgress:0,historyMode:"browse"});
    try {
      let after=direction==="later" ? windowNext! : direction==="earlier" && previousWindows.length ? previousWindows.at(-1)! : 0;
      const rebuilding=direction==="earlier" && previousWindows.length===0;
      const target=windowAfter;
      let trail=direction==="latest" || rebuilding ? [] : [...previousWindows];
      if(direction==="earlier" && !rebuilding)trail.pop();
      if(direction==="later")trail=[...trail,windowAfter].slice(-256);
      let page:V4MessagePage;
      for(;;){
        page=await api.messages({conversation_id:id,after,limit:MESSAGE_PAGE});
        if(generation!==readGeneration || state.selectedId!==id)return;
        validateWindow(page,id,after);
        update({historyProgress:state.historyProgress+page.messages.length});
        if(direction!=="latest" && !rebuilding)break;
        if(rebuilding && page.next_after===target)break;
        if(page.next_after===null){if(rebuilding)throw Error("Previous window changed");break;}
        if(rebuilding && page.next_after>target)throw Error("Previous cursor changed");
        trail=[...trail,after].slice(-256);after=page.next_after;
      }
      previousWindows=trail;
      installWindow(page,after,direction!=="earlier" && page.next_after===null ? "following" : "browse");
      schedulePoll();
    }catch{if(generation===readGeneration)update({reading:false,seekingLatest:false,note:"History page couldn’t be read. The visible window and draft are kept; try the read again."});}
  };
  const draftKey = () => state.selectedId ?? "@new";
  const acceptMetadata = (selected: V4Conversation) => {
    metadataGeneration++;
    update({selected});
  };
  const reconcileUnknownSend = async (conversationId:string) => {
    const pending=unknownSends.get(conversationId);
    if(!pending || !api.lookupTurn)return;
    try {
      const result=await api.lookupTurn(conversationId,pending.key);
      if(unknownSends.get(conversationId)!==pending || !result || result.conversation_id!==conversationId || result.idempotency_key!==pending.key || !result.run_id || !["queued","starting","running","cancel_requested","succeeded","failed","interrupted","cancelled"].includes(result.state))return;
      unknownSends.delete(conversationId);
      const unchanged=(draftVersions.get(conversationId) ?? 0)===pending.draftVersion;
      if(unchanged)drafts.delete(conversationId);
      update(state.selectedId===conversationId ? {...(unchanged ? {draft:""} : {}),note:"The exact submitted turn was confirmed by its idempotency key. It was not sent again."} : {});
    } catch { /* Missing/failed lookup leaves this exact pending send untouched. */ }
  };
  const refresh = async (append = false) => {
    const previous = append ? listSnapshot : null;
    if (!state.canRead || (append && (state.listing || !state.more || !previous || previous.query !== state.query || previous.archived !== state.archived))) return;
    const generation = append ? listGeneration : ++listGeneration;
    const qtext = state.query;
    // Build canonical conversation query; list supports search term directly (search surface is narrow).
    const convQ: V4ConversationQuery = {
      search: qtext ? qtext : null,
      after: previous?.after ?? null,
      limit: PAGE,
      archived: state.archived,
    };
    // Keep visible rows on failure, but never retain their cursor under a new snapshot.
    if (!append) listSnapshot = null;
    update({listing:true, ...(!append ? {more:false} : {})});
    try {
      const result = qtext ? await api.search(convQ) : await api.list(convQ);
      if (generation !== listGeneration) return;
      const items = append ? [...state.items, ...result.conversations] : result.conversations;
      const next = result.next_after ?? null;
      listSnapshot = {query: qtext, archived:convQ.archived, after: next };
      update({items:items.slice(0, 256), listing:false, more: next !== null && items.length < 256});
    } catch {
      if (generation === listGeneration) update({listing:false, note:"Conversation history couldn’t be read. Your current view and drafts are kept. Refresh to try the read again."});
    }
  };
  const reconcileRetries = async (conversationId:string) => {
    if(!api.lookupTurn)return;
    for(const attempt of state.retries.filter(current=>current.conversationId===conversationId && current.phase==="unknown")) {
      try {
        const result=await api.lookupTurn(conversationId,attempt.idempotencyKey);
        if(!result || result.conversation_id!==conversationId || result.idempotency_key!==attempt.idempotencyKey || !result.run_id || result.run_id===attempt.originalRunId || !["queued","starting","running","cancel_requested","succeeded","failed","interrupted","cancelled"].includes(result.state))continue;
        update({retries:state.retries.map(current=>current===attempt ? {...attempt,phase:"accepted",runId:result.run_id,state:result.state} : current)});
      } catch { /* Unknown remains unknown; exact lookup never resubmits. */ }
    }
  };
  const select = async (id: string) => {
    if (!state.canRead) return;
    stopPoll();
    pollGeneration++;
    pollFailures = 0; quietPolls = 0; pollDelay = 250;
    contentByRun.clear();
    contentTargets.clear();
    const generation = ++readGeneration;
    const metadataVersion = metadataGeneration;
    const same = state.selectedId === id;
    if(!same)originalInputs.clear();
    messageAfter = 0;windowAfter=0;windowNext=null;previousWindows=[];
    update({contentLoadingRun:null,contentNotice:null,historyMode:"browse",earlierMessages:false,seekingLatest:false,historyProgress:0,selectedId:id, selected:same ? state.selected : null, messages:same ? state.messages : [], reading:true, moreMessages:false, draft:drafts.get(id) ?? ""});
    try {
      const [selected, history] = await Promise.all([api.read({resource_id:id}), api.messages({conversation_id:id, after:0, limit:MESSAGE_PAGE})]);
      if (generation !== readGeneration) return;
      validateWindow(history,id,0);
      const histMsgs = history.messages;
      messageAfter = histMsgs.at(-1)?.sequence ?? history.next_after ?? 0;
      // A completed metadata effect/reconciliation outranks an older read, while
      // its independently valid history still finishes loading and keeps paging.
      update({...(metadataVersion === metadataGeneration ? {selected} : {}), reading:false });
      installWindow(history,0,history.next_after===null ? "following" : "browse");
      await reconcileUnknownSend(id);
      if(generation !== readGeneration)return;
      await reconcileRetries(id);
      if(generation !== readGeneration)return;
      for (const message of histMsgs) if (message.role === "assistant" && (!message.text_complete || message.text_bytes > new TextEncoder().encode(message.text).length)) contentTargets.set(message.run_id, {message, conversationId:id});
      schedulePoll();
    } catch {
      if (generation === readGeneration) update({reading:false, note:"This conversation couldn’t be read. Existing content and its draft are kept. Refresh the conversation to try again."});
    }
  };
  const newDraft = () => {
    stopPoll();
    readGeneration++; pollGeneration++; contentByRun.clear(); contentTargets.clear(); messageAfter = 0;
    windowAfter=0;windowNext=null;previousWindows=[];originalInputs.clear();
    update({contentLoadingRun:null,contentNotice:null,historyMode:"browse",earlierMessages:false,seekingLatest:false,historyProgress:0,selectedId:null, selected:null, messages:[], reading:false, moreMessages:false, draft:drafts.get("@new") ?? ""});
  };
  const connect = async () => {
    stopPoll();pollGeneration++;
    update({contentLoadingRun:null,contentNotice:null});
    const generation = ++accessGeneration; listGeneration++; readGeneration++;
    update({access:"loading", canRead:false, canWrite:false, canCancel:false, listing:false, reading:false});
    try {
      if (api.daemon && !await api.daemon()) {
        if (generation === accessGeneration) update({access:"no-daemon"});
        return;
      }
      const grants = await api.grants(); if (generation !== accessGeneration) return;
      const canRead = grants.capabilities.includes("read_conversations"), canWrite = grants.capabilities.includes("write_conversations");
      let routes: V4Route[] = [];
      let routeStatus: Snapshot["routeStatus"] = "unavailable";
      if (canRead && grants.capabilities.includes("read_routes")) {
        routeStatus = "loading";
        try { routes = await api.routes(); routeStatus = "ready"; }
        catch { routes = []; routeStatus = "error"; }
      }
      if (generation !== accessGeneration) return;
      const routeId = routes.some(route => route.available && route.route_id === state.routeId) ? state.routeId : routes.find(route => route.available)?.route_id ?? null;
      update({canRead, canWrite:canRead && canWrite, canCancel:canRead && !!api.cancel && grants.capabilities.includes("cancel_turns"), access:canRead ? canWrite ? "ready" : "read-only" : "no-grant", routes, routeId, routeStatus});
      if (canRead) { for(const id of [...unknownSends.keys()]) await reconcileUnknownSend(id); for(const id of new Set(state.retries.filter(attempt=>attempt.phase==="unknown").map(attempt=>attempt.conversationId))) await reconcileRetries(id); await refresh(); if(generation === accessGeneration && state.selectedId) await select(state.selectedId); }
      else { stopPoll(); }
    } catch {
      stopPoll();
      if (generation === accessGeneration) update({access:"offline", note:"Conversation connection unavailable. Existing content and drafts are kept."});
    }
  };
  const mutate = async (kind: MutationKind, title = "") => {
    if (!state.canWrite || state.mutation || (kind !== "create" && !state.selectedId)) return;
    const id = state.selectedId;
    const mutation: Mutation = {kind, id, title, phase:"sending"};
    update({mutation, note:null}); // Synchronous duplicate-click guard, before invoking once.
    try {
      let result: V4Conversation | void = undefined;
      if(kind === "create") result = await api.create({title});
      else if(kind === "rename") result = await api.rename({conversation_id:id!, title});
      else if(kind === "archive") result = await api.archive({resource_id:id!});
      else await api.delete({resource_id:id!});
      update({mutation:null, note:kind === "delete" ? "Conversation deleted." : "Conversation saved."});
      if(kind === "delete" && state.selectedId === id) { drafts.delete(id!); newDraft(); }
      else if(result && (kind === "create" ? state.selectedId === id : state.selectedId === result.id)) {
        // Creating metadata never sends or consumes a draft.
        if(kind === "create") await select(result.id); else acceptMetadata(result);
      }
      await refresh();
    } catch (error) {
      if (["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)))) {
        update({mutation:null, note:"The conversation request was rejected. Your draft is kept."});
      } else update({mutation:{...mutation, phase:"unknown"}, note:"Outcome unknown: the action may have happened. It will not be sent again. Check through reads before making another change."});
    }
  };
  const reconcile = async () => {
    const mutation = state.mutation; if (!mutation || mutation.phase !== "unknown") return;
    await refresh(); // A list alone can never prove absence or identify a lost create.
    if (!mutation.id || mutation.kind === "create") return;
    try {
      const current = await api.read({resource_id:mutation.id});
      if (state.mutation !== mutation) return;
      if (current.id === mutation.id && ((mutation.kind === "update" && mutation.patch && (mutation.patch.title === null || current.title === mutation.patch.title) && (mutation.patch.pinned === null || current.pinned === mutation.patch.pinned) && (mutation.patch.archived === null || current.archived === mutation.patch.archived)) || (mutation.kind === "rename" && current.title === mutation.title) || (mutation.kind === "archive" && current.archived))) {
        update({mutation:null, note:"Current conversation state matches the requested change (read reconciliation)."});
        if(state.selectedId === current.id) acceptMetadata(current);
      }
    } catch (error) {
      if (state.mutation === mutation && mutation.kind === "delete" && kindOf(error) === "not_found") {
        update({mutation:null, note:"The exact conversation is no longer present (read reconciliation)."});
        if (state.selectedId === mutation.id) { drafts.delete(mutation.id); newDraft(); }
      }
    }
  };
  const canRetry = (conversationId:string, runId:string) => {
    if (conversationId !== state.selectedId || state.historyMode!=="following" || !state.canWrite || state.reading || state.sending || state.mutation || state.activeRunId || state.sendUnknownFor === conversationId || state.retries.some(attempt => (attempt.phase === "unknown" && attempt.conversationId === conversationId) || (attempt.phase === "accepted" && attempt.conversationId === conversationId && (attempt.state === null || activeState(attempt.state)) && !state.messages.some(message => message.run_id === attempt.runId && !activeState(message.state))) || (attempt.conversationId === conversationId && attempt.originalRunId === runId && attempt.phase !== "rejected")) || state.retries.length >= 256 || state.routeStatus !== "ready" || !state.routes.some(route => route.available && route.route_id === state.routeId)) return false;
    const messages = state.messages.filter(message => message.conversation_id === conversationId && message.run_id === runId);
    const latest = messages.at(-1);
    const original = messages.find(message => message.role === "user") ?? originalInputs.get(inputKey(conversationId,runId));
    return !!latest && ["failed", "interrupted", "cancelled"].includes(latest.state) && !!original && original.text_complete && original.text.trim().length > 0 && new TextEncoder().encode(original.text).length === original.text_bytes;
  };
  return {
    snapshot: () => state,
    subscribe(fn: () => void) {listeners.add(fn); return () => {listeners.delete(fn);};},
    connect, select, newDraft, mutate, reconcile,
    async filterArchived(archived:boolean) {
      if(state.archived===archived)return;
      listGeneration++;listSnapshot=null;
      update({archived,items:[],more:false,listing:false});
      await refresh();
    },
    async updateMetadata(patch:V4ConversationPatch) {
      if(!state.canWrite || !api.update || !state.selectedId || state.mutation || state.reading)return;
      const id=state.selectedId;
      const exact={title:patch.title,archived:patch.archived,pinned:patch.pinned};
      const mutation:Mutation={kind:"update",id,title:"",patch:exact,phase:"sending"};
      update({mutation,note:null});
      try {
        const current=await api.update(id,{...exact});
        if(current.id!==id)throw Error("Uncorrelated conversation update");
        update({mutation:null,...(state.selectedId===id ? {note:"Conversation saved."} : {})});
        if(state.selectedId===id)acceptMetadata(current);
        await refresh();
      } catch(error) {
        const rejected=["rejected","configuration","unsupported_platform","not_found"].includes(String(kindOf(error)));
        update({mutation:rejected ? null : {...mutation,phase:"unknown"},note:rejected ? "Conversation update rejected. Your draft is kept." : "Update outcome unknown. Check current state before making another change; it will not be sent again."});
      }
    },
    draft(value: string) {
      const key = draftKey();
      draftVersions.set(key,(draftVersions.get(key) ?? 0)+1);
      if (!drafts.has(key) && drafts.size >= 32 && value) {update({note:"This window holds 32 drafts. Clear an existing draft before adding another."}); return;}
      const draft = value.slice(0, 8192); if(draft) drafts.set(key, draft); else drafts.delete(key); update({draft});
    },
    search(query: string) {update({query:query.slice(0, 256)}); return refresh();},
    chooseRoute(routeId: string) {
      if (state.routes.some(route => route.available && route.route_id === routeId)) update({routeId});
    },
    async refreshRoutes() {
      if (!state.canRead) return;
      if (!state.routes.length && state.routeStatus !== "error") return;
      update({routeStatus:"loading"});
      try {
        const routes = await api.routes();
        const routeId = routes.some(route => route.available && route.route_id === state.routeId) ? state.routeId : routes.find(route => route.available)?.route_id ?? null;
        update({routes, routeId, routeStatus:"ready"});
      } catch {
        update({routes:[], routeId:null, routeStatus:"error"});
      }
    },
    async stop(conversationId = state.selectedId, runId = state.activeRunId) {
      if (conversationId !== state.selectedId || runId !== state.activeRunId) return;
      if (!state.canRead || !state.canCancel || !api.cancel || !conversationId || !runId || state.reading || stops.has(runId) || state.messages.some(message => message.run_id === runId && message.state === "cancel_requested")) return;
      const action: StopState = {conversationId, runId, phase:"sending", note:"Requesting Stop…"};
      stops.set(runId, action);
      update({}); // Guard is synchronous, before the one effect request.
      try {
        const result = await api.cancel(conversationId, runId);
        stops.set(runId, {...action, phase:activeState(result) ? "requested" : "settled", note:result === "cancel_requested" ? "Stop requested. Waiting for the reply to finish stopping…" : `Reply ${result}.`});
      } catch (error) {
        const rejected = ["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)));
        stops.set(runId, {...action, phase:rejected ? "rejected" : "unknown", note:rejected ? "Stop was rejected. Refresh this conversation to read its current state." : "Stop outcome unknown. It will not be sent again. Checking conversation history…"});
      }
      // Never select the captured conversation after navigation. Existing polling
      // and explicit Refresh reconcile reads, and never resend cancellation.
      update({});
      if (state.selectedId === conversationId) schedulePoll();
    },
    canRetry,
    async retry(conversationId:string, originalRunId:string) {
      if (!canRetry(conversationId, originalRunId)) return;
      const input = (state.messages.find(message => message.conversation_id === conversationId && message.run_id === originalRunId && message.role === "user") ?? originalInputs.get(inputKey(conversationId,originalRunId)))!.text;
      const routeId = state.routeId!;
      const idempotency = `turn-${crypto.randomUUID()}`;
      const attempt:RetryAttempt = {conversationId, originalRunId, routeId, runId:null, idempotencyKey:idempotency, state:null, phase:"sending"};
      update({sending:true, retries:[...state.retries.filter(previous => previous.conversationId !== conversationId || previous.originalRunId !== originalRunId), attempt], note:null});
      const finish = (patch:Partial<RetryAttempt>, note:string | null) => update({sending:false, retries:state.retries.map(current => current === attempt ? {...attempt, ...patch} : current), ...(state.selectedId === conversationId ? {note} : {})});
      try {
        const result = await api.submit({conversation_id:conversationId, idempotency_key:idempotency, route_id:routeId, input});
        if (result.conversation_id !== conversationId || result.idempotency_key !== idempotency || !result.run_id || result.run_id === originalRunId || result.duplicate) throw new Error("Uncorrelated retry response");
        finish({phase:"accepted", runId:result.run_id, state:result.state}, "Retry accepted. Your composer draft is kept.");
        if (state.selectedId === conversationId) {await select(conversationId);if(state.selectedId===conversationId && state.moreMessages)await moveWindow("latest");}
      } catch (error) {
        const rejected = ["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)));
        finish({phase:rejected ? "rejected" : "unknown"}, rejected ? "Retry rejected. Your draft is kept." : "Retry outcome unknown. It will not be sent again; inspect history. Your draft is kept.");
      }
    },
    async send() {
      if (!state.canWrite || state.sending || state.retries.some(attempt => attempt.phase === "unknown" && attempt.conversationId === state.selectedId) || state.mutation || (state.selectedId !== null && state.sendUnknownFor === state.selectedId) || !state.routeId || state.routeStatus !== "ready" || !state.routes.some(route => route.available && route.route_id === state.routeId)) return;
      if(unknownSends.size>=256) {update({note:"This window holds 256 unresolved sends. Reconcile an existing send before submitting another."});return;}
      const input = state.draft.trim();
      if (!input) return;
      const idempotency = `turn-${crypto.randomUUID()}`;
      const routeId = state.routeId;
      let submittedAt = 0;
      const originalDraftVersion=draftVersions.get(draftKey()) ?? 0;
      update({sending:true, note:null});
      let conversationId = state.selectedId;
      if (!conversationId) {
        const line = (input.split("\n", 1)[0] ?? "").replace(/[\u0000-\u001f\u007f]/g, " ").trim();
        try {
          const created = await api.create({title:(line || "New conversation").slice(0, 80)});
          conversationId = created.id;
          drafts.delete("@new");
          drafts.set(conversationId, input);
          draftVersions.set(conversationId,originalDraftVersion);
          update({selectedId:created.id, selected:created});
        } catch (error) {
          const rejected = ["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)));
          update({sending:false, ...(rejected ? {} : {mutation:{kind:"create", id:null, title:line.slice(0, 80) || "New conversation", phase:"unknown"}}), note:rejected ? "The conversation could not be created. Your draft is kept." : "Conversation creation outcome unknown. Inspect history before retrying; the draft is kept and no message was submitted."});
          return;
        }
      }
      try {
        submittedAt = Date.now();
        const result=await api.submit({conversation_id:conversationId, idempotency_key:idempotency, route_id:routeId, input});
        if(result.conversation_id!==conversationId || result.idempotency_key!==idempotency || !result.run_id || !["queued","starting","running","cancel_requested","succeeded","failed","interrupted","cancelled"].includes(result.state))throw new Error("Uncorrelated submission response");
        const unchanged=(draftVersions.get(conversationId) ?? 0)===originalDraftVersion;
        if(unchanged)drafts.delete(conversationId);
        update({sending:false,...(state.selectedId===conversationId ? {...(unchanged ? {draft:""} : {}),note:null} : {})});
        if(state.selectedId===conversationId){await select(conversationId);if(state.selectedId===conversationId && state.moreMessages)await moveWindow("latest");}
      } catch (error) {
        const rejected = ["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)));
        if(!rejected)unknownSends.set(conversationId,{key:idempotency,input,at:submittedAt,draftVersion:originalDraftVersion});
        update({sending:false,...(state.selectedId===conversationId ? {note:rejected ? "The message was rejected. Your draft is kept." : "Outcome unknown: this message may have been submitted. It will not be sent again. Refresh this conversation before continuing."} : {})});
      }
    },
    refresh: () => refresh(), more: () => refresh(true),
    moreMessages:()=>moveWindow("later"),
    laterMessages:()=>moveWindow("later"),
    earlierMessages:()=>moveWindow("earlier"),
    latestMessages:()=>moveWindow("latest"),
    async loadContent(message: V4Message) {
      if (!state.canRead || !state.selectedId || !api.content || state.contentLoadingRun || !state.messages.some(current=>current.sequence===message.sequence && current.run_id===message.run_id)) return;
      const conversationId=state.selectedId;
      contentTargets.set(message.run_id,{message,conversationId});
      stopPoll();
      const generation=++pollGeneration;
      update({contentLoadingRun:message.run_id,contentNotice:null});
      let notice:string|null=null;
      for(let page=0;page<128;page++) {
        const progress=await readContent(message,conversationId,generation);
        if(generation!==pollGeneration || state.selectedId!==conversationId)return;
        const saved=contentByRun.get(message.run_id);
        if(saved?.done)break;
        if(!progress){notice="Reply content couldn’t advance. Continue loading to try the read again.";break;}
        if(!saved?.more){notice="Available reply content loaded; the reply is incomplete. Continue loading to check again.";break;}
        if(page===127)notice="Read limit reached. Continue loading reply to read the next bounded portion.";
      }
      update({contentLoadingRun:null,contentNotice:notice});
      schedulePoll();
    },
  };
}
export type Conversations = ReturnType<typeof createConversations>;
