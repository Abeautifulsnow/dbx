import { defineStore } from "pinia";
import { ref } from "vue";
import * as api from "@/lib/backend/api";
import type { PromptTemplate } from "@/types/promptTemplate";

export const usePromptTemplateStore = defineStore("promptTemplate", () => {
  const templates = ref<PromptTemplate[]>([]);
  const globalInstructions = ref("");
  const isLoaded = ref(false);
  const isLoading = ref(false);

  async function init() {
    if (isLoaded.value || isLoading.value) return;
    isLoading.value = true;
    try {
      const [tpls, gi] = await Promise.all([api.loadPromptTemplates(), api.getAiGlobalCustomInstructions()]);
      templates.value = tpls;
      globalInstructions.value = gi;
      isLoaded.value = true;
    } catch {
      // Start-up backend unavailable — leave isLoaded=false so consumers
      // show loading state and ensureLoaded() can retry on user interaction.
    } finally {
      isLoading.value = false;
    }
  }

  /** Call from consumers when !isLoaded — idempotent retry on prior failure. */
  async function ensureLoaded() {
    if (isLoaded.value) return;
    await init();
  }

  async function save(id: string, name: string, content: string): Promise<PromptTemplate> {
    const saved = await api.savePromptTemplate(id, name, content);
    const idx = templates.value.findIndex((t) => t.id === id);
    if (idx >= 0) {
      templates.value[idx] = saved;
    } else {
      templates.value.push(saved);
    }
    // Maintain stable sort order: created_at, then id
    templates.value = [...templates.value].sort(sortTemplates);
    return saved;
  }

  async function remove(id: string): Promise<void> {
    await api.deletePromptTemplate(id);
    templates.value = templates.value.filter((t) => t.id !== id);
  }

  async function saveGlobalInstructions(content: string): Promise<void> {
    const trimmed = content.trim();
    await api.setAiGlobalCustomInstructions(trimmed);
    globalInstructions.value = trimmed;
  }

  return { templates, globalInstructions, isLoaded, isLoading, init, ensureLoaded, save, remove, saveGlobalInstructions };
});

function sortTemplates(a: PromptTemplate, b: PromptTemplate): number {
  if (a.createdAt !== b.createdAt) return a.createdAt < b.createdAt ? -1 : 1;
  return a.id < b.id ? -1 : 1;
}
