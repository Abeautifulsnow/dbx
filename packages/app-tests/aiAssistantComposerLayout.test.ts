import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "vitest";
import { compileTemplate, parse } from "vue/compiler-sfc";

const aiAssistantPath = "apps/desktop/src/components/editor/AiAssistant.vue";
const source = readFileSync(aiAssistantPath, "utf8");

test("AI composer keeps templates in the connection context row", () => {
  const contextRowStart = source.indexOf('v-if="connectionStore.connections.length"');
  const contextRowEnd = source.indexOf('v-if="mentionOpen"', contextRowStart);
  const contextRow = source.slice(contextRowStart, contextRowEnd);

  assert.notEqual(contextRowStart, -1, "the connection context row should exist");
  assert.notEqual(contextRowEnd, -1, "the connection context row should end before mention suggestions");
  assert.match(contextRow, /<Popover v-model:open="showTemplateSelector">/);
  assert.match(contextRow, /max-w-\[40%\]/);
  assert.match(contextRow, /<span class="truncate">\{\{ templateSelectorLabel \}\}<\/span>/);
  assert.match(contextRow, /:aria-label="t\('ai\.templateSelectorLabel', \{ label: templateSelectorLabel \}\)"/);
});

test("AI composer exposes mode and action as one compact selector", () => {
  const footerStart = source.indexOf('<!-- Combined mode + action selector -->');
  const footerEnd = source.indexOf('<!-- Combined provider + model selector -->', footerStart);
  const footer = source.slice(footerStart, footerEnd);

  assert.notEqual(footerStart, -1, "the combined mode and action selector should exist");
  assert.notEqual(footerEnd, -1, "the model selector should follow the combined selector");
  assert.match(footer, /<Popover v-model:open="modeActionOpen">/);
  assert.match(footer, /switchModeActionTab\('ask'\)/);
  assert.match(footer, /switchModeActionTab\('agent'\)/);
  assert.match(footer, /selectModeActionItem\(button\.action\)/);
  assert.doesNotMatch(footer, /selectAction\(button\.action\)/);
});

test("AI composer template remains compilable", () => {
  const { descriptor, errors } = parse(source, { filename: aiAssistantPath });
  assert.deepEqual(errors, []);
  assert.ok(descriptor.template);

  const result = compileTemplate({ id: aiAssistantPath, filename: aiAssistantPath, source: descriptor.template.content });
  assert.deepEqual(result.errors, []);
});
