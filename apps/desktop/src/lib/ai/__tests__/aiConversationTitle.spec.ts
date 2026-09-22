import { reactive, ref } from "vue";
import { describe, expect, it, vi } from "vitest";
import { renameConversationTitle, resolveConversationTitle, useConversationTitle, type ResolveConversationTitleOptions } from "@/lib/ai/aiConversationTitle";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("AI conversation title resolution", () => {
  const stored = [
    { id: "conversation-1", title: "Repair user records" },
    { id: "conversation-2", title: "订单排查" },
  ];
  const cases: { name: string; options: ResolveConversationTitleOptions; expected: string }[] = [
    {
      name: "prefers a title renamed in this session over the stored one",
      options: { conversationId: "conversation-1", renamedTitles: new Map([["conversation-1", "User analysis"]]), conversations: stored, deriveFromFirstMessage: () => "How do I find duplicate emails?", fallback: "New chat" },
      expected: "User analysis",
    },
    {
      name: "falls back to the stored title when the session has no rename claim",
      options: { conversationId: "conversation-1", renamedTitles: new Map(), conversations: stored, deriveFromFirstMessage: () => "How do I find duplicate emails?", fallback: "New chat" },
      expected: "Repair user records",
    },
    {
      name: "derives from the first message for a chat that has not been persisted yet",
      options: { conversationId: "unsaved-conversation", renamedTitles: new Map(), conversations: stored, deriveFromFirstMessage: () => "Which indexes are unused?", fallback: "New chat" },
      expected: "Which indexes are unused?",
    },
    {
      name: "keeps a renamed title when a recovered run re-writes the stored snapshot",
      options: { conversationId: "conversation-1", renamedTitles: new Map([["conversation-1", "User analysis"]]), conversations: stored, deriveFromFirstMessage: () => "How do I find duplicate emails?", fallback: "Untitled" },
      expected: "User analysis",
    },
    {
      name: "falls back to the record's own title when the conversation is no longer listed",
      options: { conversationId: "conversation-9", renamedTitles: new Map(), conversations: stored, storedTitle: "Recovered title", deriveFromFirstMessage: () => "How do I find duplicate emails?", fallback: "Untitled" },
      expected: "Recovered title",
    },
    {
      name: "prefers the listed stored title over the record-level fallback",
      options: { conversationId: "conversation-1", renamedTitles: new Map(), conversations: stored, storedTitle: "Stale title", fallback: "Untitled" },
      expected: "Repair user records",
    },
    {
      name: "prefers the plugin's title over a rename and the stored title",
      options: { pluginContextTitle: "Analyze users table", conversationId: "conversation-1", renamedTitles: new Map([["conversation-1", "User analysis"]]), conversations: stored, deriveFromFirstMessage: () => "How do I find duplicate emails?", fallback: "New chat" },
      expected: "Analyze users table",
    },
    {
      name: "ignores an empty plugin title",
      options: { pluginContextTitle: "", conversationId: "conversation-1", renamedTitles: new Map(), conversations: stored, fallback: "New chat" },
      expected: "Repair user records",
    },
    {
      name: "uses the persisted Untitled fallback for a chat with no messages and no stored record",
      options: { conversationId: "conversation-9", renamedTitles: new Map(), conversations: stored, fallback: "Untitled" },
      expected: "Untitled",
    },
    {
      name: "uses the new-chat label when rendering an empty chat",
      options: { conversationId: "", renamedTitles: new Map(), conversations: stored, fallback: "New chat" },
      expected: "New chat",
    },
    {
      name: "ignores a rename claim that belongs to another conversation",
      options: { conversationId: "conversation-2", renamedTitles: new Map([["conversation-1", "User analysis"]]), conversations: stored, fallback: "New chat" },
      expected: "订单排查",
    },
    {
      name: "treats an empty stored title as absent",
      options: { conversationId: "conversation-3", renamedTitles: new Map(), conversations: [{ id: "conversation-3", title: "" }], deriveFromFirstMessage: () => "Any long-running queries?", fallback: "Untitled" },
      expected: "Any long-running queries?",
    },
  ];

  it.each(cases)("$name", ({ options, expected }) => {
    expect(resolveConversationTitle(options)).toBe(expected);
  });

  it("derives from the transcript only when no rename claim or stored title exists", () => {
    const deriveFromFirstMessage = vi.fn().mockReturnValue("How do I find duplicate emails?");

    expect(resolveConversationTitle({ conversationId: "conversation-1", renamedTitles: new Map([["conversation-1", "User analysis"]]), conversations: stored, deriveFromFirstMessage, fallback: "New chat" })).toBe("User analysis");
    expect(resolveConversationTitle({ conversationId: "conversation-1", renamedTitles: new Map(), conversations: stored, deriveFromFirstMessage, fallback: "New chat" })).toBe("Repair user records");
    expect(deriveFromFirstMessage).not.toHaveBeenCalled();

    expect(resolveConversationTitle({ conversationId: "unsaved-conversation", renamedTitles: new Map(), conversations: stored, deriveFromFirstMessage, fallback: "New chat" })).toBe("How do I find duplicate emails?");
    expect(deriveFromFirstMessage).toHaveBeenCalledTimes(1);
  });
});

describe("AI conversation title reactivity", () => {
  it("re-resolves the header title when a rename claim or the conversation list changes", () => {
    const renamedTitles = reactive(new Map<string, string>());
    const conversationId = ref("conversation-1");
    const conversations = ref<{ id: string; title: string }[]>([{ id: "conversation-1", title: "Repair user records" }]);
    const title = useConversationTitle({
      conversationId: () => conversationId.value,
      renamedTitles,
      conversations: () => conversations.value,
      deriveFromFirstMessage: () => "How do I find duplicate emails?",
      fallback: () => "New chat",
    });
    expect(title.value).toBe("Repair user records");

    // Renaming the open conversation from the history list shows up in the
    // header without reopening the panel (issue #9904).
    renamedTitles.set("conversation-1", "User analysis");
    expect(title.value).toBe("User analysis");

    // A failed save retracts the claim, so the stored title comes back.
    renamedTitles.delete("conversation-1");
    expect(title.value).toBe("Repair user records");

    // Switching conversations follows the id rather than the previous title.
    conversations.value = [...conversations.value, { id: "conversation-2", title: "订单排查" }];
    conversationId.value = "conversation-2";
    expect(title.value).toBe("订单排查");
  });

  it("tracks the locale label for a chat that has no title yet", () => {
    const locale = ref("New chat");
    const title = useConversationTitle({
      conversationId: () => "",
      renamedTitles: new Map(),
      conversations: () => [],
      fallback: () => locale.value,
    });
    expect(title.value).toBe("New chat");
    locale.value = "新对话";
    expect(title.value).toBe("新对话");
  });
});

describe("AI conversation rename", () => {
  const conversation = { id: "conversation-1", title: "Repair user records", updatedAt: "2026-01-01T00:00:00.000Z" };

  it("claims the title before the save lands so a mid-save snapshot keeps the new title", async () => {
    const save = deferred();
    const renamedTitles = new Map<string, string>();
    const persist = vi.fn().mockReturnValue(save.promise);
    const replaceInList = vi.fn();

    const renaming = renameConversationTitle({ conversation, title: "User analysis", renamedTitles, persist, replaceInList });

    // Mid-save: the claim is already visible to every title reader and the
    // history list still shows the old title.
    expect(renamedTitles.get(conversation.id)).toBe("User analysis");
    expect(replaceInList).not.toHaveBeenCalled();

    save.resolve();
    await expect(renaming).resolves.toBe(true);
    expect(replaceInList).toHaveBeenCalledWith(expect.objectContaining({ id: conversation.id, title: "User analysis" }));
  });

  it("persists the submitted title with a fresh updatedAt and keeps the other fields", async () => {
    const persist = vi.fn().mockResolvedValue(undefined);

    await renameConversationTitle({ conversation: { ...conversation, connectionName: "dev-mysql" }, title: "User analysis", renamedTitles: new Map(), persist, replaceInList: vi.fn() });

    expect(persist).toHaveBeenCalledWith(expect.objectContaining({ id: conversation.id, title: "User analysis", connectionName: "dev-mysql", updatedAt: expect.stringMatching(/^\d{4}-\d{2}-\d{2}T/) }));
  });

  it("retracts the claim and leaves the list alone when the save fails, so readers keep the stored title", async () => {
    const renamedTitles = new Map<string, string>();
    const replaceInList = vi.fn();

    const persisted = await renameConversationTitle({ conversation, title: "User analysis", renamedTitles, persist: vi.fn().mockRejectedValue(new Error("backend offline")), replaceInList });

    expect(persisted).toBe(false);
    expect(renamedTitles.has(conversation.id)).toBe(false);
    expect(replaceInList).not.toHaveBeenCalled();
    // The header, the history row and the next snapshot therefore all resolve
    // back to the stored title.
    expect(resolveConversationTitle({ conversationId: conversation.id, renamedTitles, conversations: [conversation], fallback: "New chat" })).toBe("Repair user records");
  });
});
