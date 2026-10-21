import type { RunState, V4CapabilitySet, V4ContentPage, V4Conversation, V4ConversationCreate, V4ConversationPage, V4ConversationQuery, V4ConversationRename, V4Message, V4MessagePage, V4MessageQuery, V4ResourceQuery, V4Route, V4SubmitTurn, V4TurnSubmission } from "../ipc/generated";
export interface ConversationApi {
  daemon?: () => Promise<boolean>;
  grants: () => Promise<V4CapabilitySet>;
  list: (q: V4ConversationQuery) => Promise<V4ConversationPage>;
  search: (q: V4ConversationQuery) => Promise<V4ConversationPage>;
  read: (q: V4ResourceQuery) => Promise<V4Conversation>;
  messages: (q: V4MessageQuery) => Promise<V4MessagePage>;
  content?: (q: {conversation_id:string; run_id:string; after:number; limit:number}) => Promise<V4ContentPage>;
  create: (q: V4ConversationCreate) => Promise<V4Conversation>;
  rename: (q: V4ConversationRename) => Promise<V4Conversation>;
  archive: (q: V4ResourceQuery) => Promise<V4Conversation>;
  delete: (q: V4ResourceQuery) => Promise<void>;
  routes: () => Promise<V4Route[]>;
  submit: (request: V4SubmitTurn) => Promise<V4TurnSubmission>;
  cancel?: (conversationId:string, runId:string) => Promise<RunState>;
}
export type MutationKind = "create" | "rename" | "archive" | "delete";
interface Mutation { kind: MutationKind; id: string | null; title: string; phase: "sending" | "unknown" }
interface StopState { conversationId:string; runId:string; phase:"sending" | "requested" | "unknown" | "rejected" | "settled"; note:string }
const activeState = (value:RunState) => ["queued", "starting", "running", "cancel_requested"].includes(value);
interface Snapshot {
  access: "loading" | "ready" | "read-only" | "no-daemon" | "no-grant" | "offline";
  canRead: boolean; canWrite: boolean; canCancel:boolean; activeRunId:string | null; stop:StopState | null; query: string; items: V4Conversation[];
  listing: boolean; more: boolean; selectedId: string | null; selected: V4Conversation | null;
  messages: V4Message[]; reading: boolean; moreMessages: boolean; draft: string;
  mutation: Mutation | null; note: string | null;
  routes: V4Route[]; routeId: string | null; sending: boolean;
  routeStatus: "loading" | "ready" | "unavailable" | "error";
  sendUnknownFor: string | null;
  sendUnknownText: string | null;
  sendUnknownAt: number | null;
}
const PAGE = 16;
const kindOf = (error: unknown) => typeof error === "object" && error !== null && "kind" in error ? error.kind : null;
// One controller per application window. Never persisted or logged. No timers/retries.
export function createConversations(api: ConversationApi) {
  let state: Snapshot = {access:"loading", canRead:false, canWrite:false, canCancel:false, activeRunId:null, stop:null, query:"", items:[], listing:false, more:false, selectedId:null, selected:null, messages:[], reading:false, moreMessages:false, draft:"", mutation:null, note:null, routes:[], routeId:null, routeStatus:"loading", sendUnknownFor:null, sendUnknownText:null, sendUnknownAt:null, sending:false};
  const stops = new Map<string, StopState>();
  const listeners = new Set<() => void>();
  const drafts = new Map<string, string>();
  let accessGeneration = 0, listGeneration = 0, readGeneration = 0, metadataGeneration = 0, pollGeneration = 0;
  let pollFailures = 0, quietPolls = 0, pollDelay = 250;
  // A cursor belongs only to the successful first page of its exact query.
  // Conversation list uses ID cursors (after: ConvId, next_after); messages use numeric sequence.
  let listSnapshot: {query: string; after: string | null} | null = null;
  let messageAfter = 0;
  const contentByRun = new Map<string, {after:number; text:string; done:boolean}>();
  const contentTargets = new Map<string, {message:V4Message; conversationId:string}>();
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  const update = (patch: Partial<Snapshot>) => {
    state = {...state, ...patch};
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
    const saved = contentByRun.get(message.run_id) ?? {after:0, text:"", done:false};
    if (saved.done) return false;
    try {
      const page = await api.content({conversation_id:conversationId, run_id:message.run_id, after:saved.after, limit:50});
      if (generation !== pollGeneration || state.selectedId !== conversationId || page.conversation_id !== conversationId || page.run_id !== message.run_id || page.after !== saved.after) return false;
      let text = saved.text;
      let after = saved.after;
      let terminal = false;
      let terminalState: RunState | null = null;
      for (const event of page.events) {
        // Event sequences are conversation-scoped, while this page is filtered
        // to one run; gaps can therefore be valid but replay/out-of-order is not.
        if (event.sequence <= after || event.run_id !== message.run_id || event.conversation_id !== conversationId) return false;
        after = event.sequence;
        if (event.payload.kind === "text_delta") text += event.payload.data.text;
        if (event.payload.kind === "terminal") { terminal = true; terminalState = event.payload.data.state; }
      }
      if (page.next_after !== null && page.next_after !== after) return false;
      contentByRun.set(message.run_id, {after, text, done:terminal});
      const bytes = new TextEncoder().encode(text).length;
      const messages = state.messages.map(current => current.run_id === message.run_id && current.role === "assistant" ? {...current, text, text_bytes:bytes, text_complete:terminal, ...(terminalState ? {state:terminalState} : {})} : current);
      update({messages});
      if (terminal) contentTargets.delete(message.run_id);
      return page.events.length > 0;
    } catch { /* Poll/reconnect retries this exact read cursor; no provider request is repeated. */ return false; }
  };
  const schedulePoll = () => {
    stopPoll();
    if (!state.selectedId || !state.canRead) return;
    const id = state.selectedId;
    pollTimer = setTimeout(() => {
      pollTimer = null;
      const gen = ++pollGeneration;
      if (!state.selectedId || state.selectedId !== id || gen !== pollGeneration) return;
      void (async () => {
        try {
          let activity = false;
          const previousAfter = messageAfter;
          const after = messageAfter > 0 ? messageAfter - 1 : null;
          const tail = await api.messages({conversation_id: id, after, limit: 8});
          if (gen !== pollGeneration || state.selectedId !== id) return;
          if (tail.messages.length) {
            const bySeq = new Map(state.messages.map(message => [message.sequence, message]));
            for (const message of tail.messages) bySeq.set(message.sequence, message);
            const merged = [...bySeq.values()].sort((a, b) => a.sequence - b.sequence).slice(-256);
            messageAfter = merged.at(-1)?.sequence ?? messageAfter;
            activity = messageAfter > previousAfter;
            update({messages: merged});
            reconcileUnknownSend(id, tail.messages);
            for (const message of tail.messages) {
              if (message.role === "assistant" && (!message.text_complete || message.text_bytes > new TextEncoder().encode(message.text).length)) {
                contentTargets.set(message.run_id, {message, conversationId:id});
              }
            }
          }
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
  const draftKey = () => state.selectedId ?? "@new";
  const acceptMetadata = (selected: V4Conversation) => {
    metadataGeneration++;
    update({selected});
  };
  const reconcileUnknownSend = (conversationId: string, messages: V4Message[]) => {
    if (state.sendUnknownFor !== conversationId || state.sendUnknownText === null || state.sendUnknownAt === null) return;
    const accepted = messages.some(message => message.role === "user" && message.text === state.sendUnknownText && message.created_at_ms >= state.sendUnknownAt!);
    if (!accepted) return;
    drafts.delete(conversationId);
    update({sendUnknownFor:null, sendUnknownText:null, sendUnknownAt:null, ...(state.selectedId === conversationId ? {draft:""} : {}), note:"The submitted message is present in this conversation’s history. It was not sent again."});
  };
  const refresh = async (append = false) => {
    const previous = append ? listSnapshot : null;
    if (!state.canRead || (append && (state.listing || !state.more || !previous || previous.query !== state.query))) return;
    const generation = append ? listGeneration : ++listGeneration;
    const qtext = state.query;
    // Build canonical conversation query; list supports search term directly (search surface is narrow).
    const convQ: V4ConversationQuery = {
      search: qtext ? qtext : null,
      after: previous?.after ?? null,
      limit: PAGE,
      archived: false,
    };
    // Keep visible rows on failure, but never retain their cursor under a new snapshot.
    if (!append) listSnapshot = null;
    update({listing:true, ...(!append ? {more:false} : {})});
    try {
      const result = qtext ? await api.search(convQ) : await api.list(convQ);
      if (generation !== listGeneration) return;
      const items = append ? [...state.items, ...result.conversations] : result.conversations;
      const next = result.next_after ?? null;
      listSnapshot = {query: qtext, after: next };
      update({items:items.slice(0, 256), listing:false, more: next !== null && items.length < 256});
    } catch {
      if (generation === listGeneration) update({listing:false, note:"Conversation history couldn’t be read. Your current view and drafts are kept. Refresh to try the read again."});
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
    messageAfter = 0;
    update({selectedId:id, selected:same ? state.selected : null, messages:same ? state.messages : [], reading:true, moreMessages:false, draft:drafts.get(id) ?? ""});
    try {
      const [selected, history] = await Promise.all([api.read({resource_id:id}), api.messages({conversation_id:id, after:0, limit:PAGE})]);
      if (generation !== readGeneration) return;
      const histMsgs = history.messages;
      messageAfter = histMsgs.at(-1)?.sequence ?? history.next_after ?? 0;
      // A completed metadata effect/reconciliation outranks an older read, while
      // its independently valid history still finishes loading and keeps paging.
      update({...(metadataVersion === metadataGeneration ? {selected} : {}), messages:histMsgs, reading:false, moreMessages: history.next_after !== null });
      reconcileUnknownSend(id, histMsgs);
      for (const message of histMsgs) if (message.role === "assistant" && (!message.text_complete || message.text_bytes > new TextEncoder().encode(message.text).length)) contentTargets.set(message.run_id, {message, conversationId:id});
      schedulePoll();
    } catch {
      if (generation === readGeneration) update({reading:false, note:"This conversation couldn’t be read. Existing content and its draft are kept. Refresh the conversation to try again."});
    }
  };
  const newDraft = () => {
    stopPoll();
    readGeneration++; pollGeneration++; contentByRun.clear(); contentTargets.clear(); messageAfter = 0;
    update({selectedId:null, selected:null, messages:[], reading:false, moreMessages:false, draft:drafts.get("@new") ?? ""});
  };
  const connect = async () => {
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
      if (canRead) { await refresh(); if(generation === accessGeneration && state.selectedId) await select(state.selectedId); }
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
      if ((mutation.kind === "rename" && current.title === mutation.title) || (mutation.kind === "archive" && current.archived)) {
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
  return {
    snapshot: () => state,
    subscribe(fn: () => void) {listeners.add(fn); return () => {listeners.delete(fn);};},
    connect, select, newDraft, mutate, reconcile,
    draft(value: string) {
      const key = draftKey();
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
    async send() {
      if (!state.canWrite || state.sending || state.mutation || (state.selectedId !== null && state.sendUnknownFor === state.selectedId) || !state.routeId || state.routeStatus !== "ready" || !state.routes.some(route => route.available && route.route_id === state.routeId)) return;
      const input = state.draft.trim();
      if (!input) return;
      const idempotency = `turn-${crypto.randomUUID()}`;
      const routeId = state.routeId;
      let submittedAt = 0;
      update({sending:true, note:null});
      let conversationId = state.selectedId;
      if (!conversationId) {
        const line = (input.split("\n", 1)[0] ?? "").replace(/[\u0000-\u001f\u007f]/g, " ").trim();
        try {
          const created = await api.create({title:(line || "New conversation").slice(0, 80)});
          conversationId = created.id;
          drafts.delete("@new");
          drafts.set(conversationId, input);
          update({selectedId:created.id, selected:created});
        } catch (error) {
          const rejected = ["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)));
          update({sending:false, ...(rejected ? {} : {mutation:{kind:"create", id:null, title:line.slice(0, 80) || "New conversation", phase:"unknown"}}), note:rejected ? "The conversation could not be created. Your draft is kept." : "Conversation creation outcome unknown. Inspect history before retrying; the draft is kept and no message was submitted."});
          return;
        }
      }
      try {
        submittedAt = Date.now();
        await api.submit({conversation_id:conversationId, idempotency_key:idempotency, route_id:routeId, input});
        drafts.delete(conversationId);
        update({sending:false, draft:"", note:null});
        await select(conversationId);
      } catch (error) {
        const rejected = ["rejected", "configuration", "unsupported_platform", "not_found"].includes(String(kindOf(error)));
        drafts.set(conversationId, input);
        update({sending:false, ...(rejected ? {} : {sendUnknownFor:conversationId, sendUnknownText:input, sendUnknownAt:submittedAt}), note:rejected ? "The message was rejected. Your draft is kept." : "Outcome unknown: this message may have been submitted. It will not be sent again. Refresh this conversation before continuing."});
      }
    },
    refresh: () => refresh(), more: () => refresh(true),
    async moreMessages() {
      if(!state.canRead || !state.selectedId || state.reading || !state.moreMessages) return;
      const id = state.selectedId, generation = readGeneration;
      update({reading:true});
      try {
        const page = await api.messages({conversation_id:id, after: messageAfter, limit: PAGE});
        if(generation !== readGeneration) return;
        const newMsgs = page.messages;
        messageAfter = newMsgs.at(-1)?.sequence ?? page.next_after ?? 0;
        const messages = [...state.messages, ...newMsgs];
        update({messages:messages.slice(0, 256), reading:false, moreMessages: page.next_after !== null && messages.length < 256});
        for (const message of newMsgs) if (message.role === "assistant" && (!message.text_complete || message.text_bytes > new TextEncoder().encode(message.text).length)) contentTargets.set(message.run_id, {message, conversationId:id});
        schedulePoll();
      } catch {if(generation === readGeneration) update({reading:false, note:"More messages couldn’t be read. The current page and draft are kept."});}
    },
    async loadContent(message: V4Message) {
      if (!state.canRead || !state.selectedId || !api.content) return;
      const conversationId = state.selectedId;
      contentTargets.set(message.run_id, {message, conversationId});
      stopPoll();
      const generation = ++pollGeneration;
      await readContent(message, conversationId, generation);
      schedulePoll();
    },
  };
}
export type Conversations = ReturnType<typeof createConversations>;
