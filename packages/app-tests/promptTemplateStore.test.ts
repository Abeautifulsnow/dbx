import assert from "node:assert/strict";
import { beforeEach, test, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { PromptTemplate } from "../../apps/desktop/src/types/promptTemplate";

const apiMock = vi.hoisted(() => ({
  getAiGlobalCustomInstructions: vi.fn(),
  loadPromptTemplates: vi.fn(),
}));

vi.mock("@/lib/backend/api", () => apiMock);

import { usePromptTemplateStore } from "../../apps/desktop/src/stores/promptTemplateStore.ts";

const template: PromptTemplate = {
  id: "production-rules",
  name: "Production Rules",
  content: "Use tenant_id filters.",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

beforeEach(() => {
  setActivePinia(createPinia());
  apiMock.loadPromptTemplates.mockReset();
  apiMock.getAiGlobalCustomInstructions.mockReset();
});

test("concurrent prompt initialization waits for one complete load", async () => {
  let resolveTemplates!: (value: PromptTemplate[]) => void;
  let resolveGlobalInstructions!: (value: string) => void;
  const templates = new Promise<PromptTemplate[]>((resolve) => {
    resolveTemplates = resolve;
  });
  const globalInstructions = new Promise<string>((resolve) => {
    resolveGlobalInstructions = resolve;
  });
  apiMock.loadPromptTemplates.mockReturnValueOnce(templates);
  apiMock.getAiGlobalCustomInstructions.mockReturnValueOnce(globalInstructions);

  const store = usePromptTemplateStore();
  const initialLoad = store.init();
  const sendLoad = store.ensureLoaded();

  assert.equal(apiMock.loadPromptTemplates.mock.calls.length, 1);
  assert.equal(apiMock.getAiGlobalCustomInstructions.mock.calls.length, 1);

  resolveTemplates([template]);
  resolveGlobalInstructions("Always use read-only SQL first.");

  assert.deepEqual(await Promise.all([initialLoad, sendLoad]), [true, true]);
  assert.deepEqual(store.templates, [template]);
  assert.equal(store.globalInstructions, "Always use read-only SQL first.");
});

test("failed prompt initialization remains retryable", async () => {
  apiMock.loadPromptTemplates.mockRejectedValueOnce(new Error("backend unavailable"));
  apiMock.getAiGlobalCustomInstructions.mockResolvedValueOnce("stale instruction");
  apiMock.loadPromptTemplates.mockResolvedValueOnce([template]);
  apiMock.getAiGlobalCustomInstructions.mockResolvedValueOnce("Recovered instruction");

  const store = usePromptTemplateStore();

  assert.equal(await store.ensureLoaded(), false);
  assert.equal(store.isLoaded, false);
  assert.equal(await store.ensureLoaded(), true);
  assert.equal(store.globalInstructions, "Recovered instruction");
  assert.deepEqual(store.templates, [template]);
});
