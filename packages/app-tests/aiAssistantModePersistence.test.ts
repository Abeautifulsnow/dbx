import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

const aiAssistantPath = fileURLToPath(new URL("../../apps/desktop/src/components/editor/AiAssistant.vue", import.meta.url));
const aiAssistantSource = readFileSync(aiAssistantPath, "utf8");

test("AI mode resets to Ask for a new conversation and panel remount", () => {
  assert.match(aiAssistantSource, /const assistantMode = ref<AiAssistantMode>\("ask"\)/);
  assert.doesNotMatch(aiAssistantSource, /"update:mode"/);

  const start = aiAssistantSource.indexOf("function startNewChat()");
  const end = aiAssistantSource.indexOf("\nonMounted", start);
  const startNewChat = aiAssistantSource.slice(start, end);
  assert.match(startNewChat, /assistantMode\.value = "ask"/);
  assert.match(startNewChat, /activeAction\.value = resolveDefaultAction\("ask"\)/);
});
