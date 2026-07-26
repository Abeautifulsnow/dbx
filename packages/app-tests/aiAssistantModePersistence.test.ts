import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

const appPath = fileURLToPath(new URL("../../apps/desktop/src/App.vue", import.meta.url));
const aiAssistantPath = fileURLToPath(new URL("../../apps/desktop/src/components/editor/AiAssistant.vue", import.meta.url));
const appSource = readFileSync(appPath, "utf8");
const aiAssistantSource = readFileSync(aiAssistantPath, "utf8");

test("AI mode is retained when the panel remounts during the current app run", () => {
  assert.match(appSource, /const aiAssistantMode = ref<AiAssistantMode>\("ask"\)/);
  assert.match(appSource, /<AiAssistant[\s\S]*?v-model:mode="aiAssistantMode"/);
  assert.match(aiAssistantSource, /mode\?: AiAssistantMode/);
  assert.match(aiAssistantSource, /"update:mode": \[mode: AiAssistantMode\]/);
  assert.match(aiAssistantSource, /get: \(\) => props\.mode \?\? "ask"/);
  assert.match(aiAssistantSource, /set: \(mode\) => emit\("update:mode", mode\)/);
});
