import { useEffect, useSyncExternalStore } from "react";
import { appConnection, appConversationGrants, appConversationsList, appConversationsSearch, appConversationRead, appConversationMessages, appConversationContent, appConversationCreate, appConversationRename, appConversationArchive, appConversationDelete, appConversationRoutes, appConversationSubmitTurn, appConversationCancelTurn, appConversationLookupTurn, appConversationUpdate } from "../ipc/client";
import { createConversations } from "./controller";
// Window-scoped, retained while the user navigates between surfaces. No storage.
export const conversations = createConversations({
  daemon: async () => (await appConnection()).source === "daemon",
  grants:appConversationGrants, list:appConversationsList, search:appConversationsSearch,
  read:appConversationRead, messages:appConversationMessages, content:appConversationContent, create:appConversationCreate,
  rename:appConversationRename, archive:appConversationArchive, delete:appConversationDelete,
  routes:appConversationRoutes, submit:appConversationSubmitTurn, cancel:appConversationCancelTurn, lookupTurn:appConversationLookupTurn, update:appConversationUpdate,
});
export function useConversations() {
  // The server snapshot is the same store: this window's conversation state
  // lives in memory, so a server render sees exactly what the client would
  // render first.
  const state = useSyncExternalStore(conversations.subscribe, conversations.snapshot, conversations.snapshot);
  useEffect(() => { void conversations.connect(); }, []);
  return {state, conversations};
}
