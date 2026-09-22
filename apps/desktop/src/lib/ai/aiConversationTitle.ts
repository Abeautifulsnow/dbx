import { computed, type ComputedRef } from "vue";

export interface ResolveConversationTitleOptions {
  /** Title supplied by the plugin that owns the conversation; outranks the rest. */
  pluginContextTitle?: string;
  conversationId: string;
  /** Titles renamed in this session, claimed before the async save lands. */
  renamedTitles: ReadonlyMap<string, string>;
  /** Known conversations; a stored title outranks the first-message excerpt. */
  conversations: readonly { id: string; title: string }[];
  /** Stored title of the record being resolved, for callers holding a record the
   *  list may no longer contain (e.g. a recovery write for a pruned conversation). */
  storedTitle?: string;
  /** Called only when no rename claim or stored title exists, so the transcript
   *  stays a reactive dependency of the caller only while a chat has no title. */
  deriveFromFirstMessage?: () => string | undefined;
  /** Last resort: the translated new-chat label when rendering, `Untitled` when persisting. */
  fallback: string;
}

/**
 * Resolves the title of an AI conversation. The panel header, the persisted
 * snapshot and the recovered-run write all go through this one function so they
 * cannot drift apart (issue #9904): a plugin-supplied title wins, then a rename
 * claimed in this session, then the stored title, then the derived first-message
 * excerpt, then the fallback.
 */
export function resolveConversationTitle(options: ResolveConversationTitleOptions): string {
  return options.pluginContextTitle || options.renamedTitles.get(options.conversationId) || options.conversations.find((conversation) => conversation.id === options.conversationId)?.title || options.storedTitle || options.deriveFromFirstMessage?.() || options.fallback;
}

export interface ConversationTitleSources {
  /** Plugin-supplied title; read lazily so the plugin context stays a dependency. */
  pluginContextTitle?: () => string | undefined;
  conversationId: () => string;
  renamedTitles: ReadonlyMap<string, string>;
  conversations: () => readonly { id: string; title: string }[];
  /** Called only when no rename claim or stored title exists. */
  deriveFromFirstMessage?: () => string | undefined;
  /** Called lazily so the active locale stays a dependency of the header. */
  fallback: () => string;
}

/**
 * Reactive form of {@link resolveConversationTitle} for the panel header. The
 * component and its tests share this composable, so a rename, a retracted claim
 * or a conversation switch re-renders through the same production code path.
 */
export function useConversationTitle(sources: ConversationTitleSources): ComputedRef<string> {
  return computed(() =>
    resolveConversationTitle({
      pluginContextTitle: sources.pluginContextTitle?.(),
      conversationId: sources.conversationId(),
      renamedTitles: sources.renamedTitles,
      conversations: sources.conversations(),
      deriveFromFirstMessage: sources.deriveFromFirstMessage,
      fallback: sources.fallback(),
    }),
  );
}

export interface RenameConversationTitleOptions<TConversation extends { id: string; title: string; updatedAt: string }> {
  /** Latest known record of the conversation being renamed. */
  conversation: TConversation;
  /** Trimmed title submitted by the user. */
  title: string;
  renamedTitles: Map<string, string>;
  /** Persists the renamed conversation; a rejection retracts the claim. */
  persist: (conversation: TConversation) => Promise<unknown>;
  /** Replaces the conversation in the in-memory list once the save succeeded. */
  replaceInList: (conversation: TConversation) => void;
}

/**
 * Persists a user rename. The title is claimed before the async save so a
 * throttled snapshot firing during the await window cannot persist the previous
 * title over it; the list entry is replaced only after the save succeeded, and a
 * failed save retracts the claim so the header, the history row and the stored
 * record all keep the old title. Returns whether the rename was persisted.
 *
 * Callers must flush any pending live-run snapshot BEFORE calling this: the
 * claim is set first, so a snapshot taken after it would persist the new title
 * (and the record spread into the save would be a snapshot interval stale).
 */
export async function renameConversationTitle<TConversation extends { id: string; title: string; updatedAt: string }>(options: RenameConversationTitleOptions<TConversation>): Promise<boolean> {
  const updated = { ...options.conversation, title: options.title, updatedAt: new Date().toISOString() };
  options.renamedTitles.set(options.conversation.id, options.title);
  try {
    await options.persist(updated);
    options.replaceInList(updated);
    return true;
  } catch {
    options.renamedTitles.delete(options.conversation.id);
    return false;
  }
}
