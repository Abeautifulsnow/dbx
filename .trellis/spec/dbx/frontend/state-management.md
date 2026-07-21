# State Management (Desktop Frontend)

> Pinia store contracts for `apps/desktop/src` — lifecycle rules that are not
> obvious from the code and must survive refactors.

---

## Contract: AI `activeModel` Lifecycle (settingsStore)

**Decided with the user (2026-07-17).** The in-conversation AI model selector
implements a **run-scoped "last used"** model, distinct from the persisted
default config. Breaking any rule below reintroduces UX bugs that were
deliberately designed away.

### 1. Scope / Trigger

- Trigger: cross-cutting state contract between the AI assistant panel
  (`apps/desktop/src/components/editor/AiAssistant.vue`) and the settings
  store (`apps/desktop/src/stores/settingsStore.ts`), with a persisted
  backend counterpart (`isDefault` flag via `api.setDefaultAiConfig`).
- Applies whenever touching: `activeModel`, `updateActiveModel`,
  `initAiConfigs`, `reloadAiConfigs`, `setDefaultAiConfig`, `startNewChat`,
  `handleModelSelect`, or the model-selector UI.

### 2. Signatures

```ts
// stores/settingsStore.ts
const activeModel = ref<{ configId: string; modelId: string } | null>(null);
function updateActiveModel(model: { configId: string; modelId: string }): void;
async function setDefaultAiConfig(id: string): Promise<void>; // persists isDefault via api
async function initAiConfigs(): Promise<void>;   // seeds activeModel from isDefault config
async function reloadAiConfigs(): Promise<void>; // re-inits; repairs dangling activeModel
```

### 3. Contracts

| State | Storage | Lifetime | Changed by |
|-------|---------|----------|------------|
| `activeModel` (last used) | **In-memory `ref` only — never localStorage, never backend** | One app run | Model selector in AI panel (`updateActiveModel`), `setDefaultAiConfig`, `initAiConfigs`/`reloadAiConfigs` |
| `isDefault` on `AiConfigItem` | Backend (persisted) | Permanent | Settings > AI only (`setDefaultAiConfig`) |

Lifecycle rules:

1. `activeModel` is a run-scoped last-used selection. Do **not** add
   persistence (localStorage/backend) without an explicit product decision.
2. In-conversation model switching (`handleModelSelect` →
   `updateActiveModel`) must **never** mutate any config's `isDefault`.
   The default config is only changeable from Settings > AI.
3. `startNewChat()` and switching between conversations must **not** reset
   `activeModel` to the default — last used is sticky within one app run.
   (A reset existed here historically and was deliberately removed.)
4. App restart intentionally falls back to the `isDefault` config:
   `initAiConfigs()` seeds `activeModel` from the default config.
5. `setDefaultAiConfig(id)` discards last-used: after the backend call
   succeeds it resets `activeModel` to the new default. On API failure it
   must **not** reset (reset sits after the `await`).
6. `reloadAiConfigs()` (e.g. after WebDAV/snippet snapshot download) re-runs
   `initAiConfigs()`, which unconditionally resets `activeModel` to the
   default config — discarding the in-session manual selection after a sync
   download is a confirmed product decision. If the reloaded list is empty,
   `activeModel` becomes `null`. (A dangling-config fallback branch that
   existed here was unreachable and was removed 2026-07-17.)

### 4. Validation & Error Matrix

| Condition | Expected behavior |
|-----------|-------------------|
| `api.setDefaultAiConfig` throws | `isDefault` flags and `activeModel` unchanged |
| New default id not found in `aiConfigs` | Guard with `if (config)`; leave `activeModel` as-is (never set undefined fields) |
| `aiConfigs` empty after `reloadAiConfigs` | `activeModel = null`; UI uses optional chaining (`settings.activeModel?.configId`) |

### 5. Good/Base/Bad Cases

- **Good**: user switches model mid-conversation → starts a new chat → new
  chat uses the switched model; Settings > AI still shows the original
  default.
- **Base**: fresh app start → `activeModel` = default config's model.
- **Bad (regression)**: `startNewChat()` re-resolving
  `aiConfigs.find((c) => c.isDefault)` and resetting `activeModel` — this
  silently reverts the user's in-session choice (removed 2026-07-17; do not
  reintroduce).
- **Bad (regression)**: model selector writing `isDefault` or calling
  `setDefaultAiConfig` as a side effect of selection.

### 6. Tests Required

Covered by `apps/desktop/src/stores/__tests__/settingsStore.spec.ts`
(`describe("settingsStore activeModel lifecycle")`, added 2026-07-17):

- `updateActiveModel` changes `activeModel` only; no `aiConfigs[i].isDefault`
  mutation.
- `setDefaultAiConfig` resets `activeModel` to the new default on success;
  rejected `api.setDefaultAiConfig` leaves both `isDefault` and `activeModel`
  untouched.
- `reloadAiConfigs` with an empty list nulls `activeModel`; with a non-empty
  list it selects the `isDefault` config even when it is not first in the
  list.

Keep these assertions passing when touching the functions above.

### 7. Wrong vs Correct

#### Wrong

```ts
function startNewChat() {
  clearMessages();
  showConversationList.value = false;
  // ❌ resets the user's in-session model choice on every new chat
  const defaultConfig = settings.aiConfigs.find((c) => c.isDefault) || settings.aiConfigs[0];
  if (defaultConfig) {
    settings.updateActiveModel({ configId: defaultConfig.id, modelId: defaultConfig.model });
  }
}
```

#### Correct

```ts
function startNewChat() {
  clearMessages();
  showConversationList.value = false;
  // activeModel intentionally untouched: last-used model is sticky per app run
}

async function setDefaultAiConfig(id: string): Promise<void> {
  await api.setDefaultAiConfig(id); // may throw — nothing below runs on failure
  aiConfigs.value.forEach((c) => {
    c.isDefault = c.id === id;
  });
  const config = aiConfigs.value.find((c) => c.id === id);
  if (config) {
    activeModel.value = { configId: config.id, modelId: config.model };
  }
}
```

---

## Contract: Prompt Template Initialization (promptTemplateStore)

### 1. Scope / Trigger

- Applies to `apps/desktop/src/stores/promptTemplateStore.ts` and consumers
  that construct AI system prompts from global instructions or selected prompt
  templates.
- The app may start `init()` without awaiting it, but every send path must
  `await ensureLoaded()` before mutating conversation state or starting an AI
  stream.

### 2. Signatures

```ts
async function init(): Promise<boolean>;
async function ensureLoaded(): Promise<boolean>;
```

### 3. Contract

- Concurrent calls share one in-flight `Promise<boolean>` so they observe the
  same complete templates and global-instructions snapshot.
- Return `true` only after both backend reads succeed and state is assigned.
- Return `false` on failure, clear the in-flight promise, and keep the store
  retryable. Callers must block the AI request and show a retryable error;
  never silently send a system prompt without configured global instructions.
- A panel's active template IDs are session-local. When the store collection
  changes, filter IDs that no longer exist so deleted templates cannot remain
  selected or inflate its label.

### 4. Tests Required

`packages/app-tests/promptTemplateStore.test.ts` must cover:

- concurrent `init()` and `ensureLoaded()` calls issue one pair of backend
  reads and both resolve only after global instructions are ready;
- a failed load returns `false` and a later `ensureLoaded()` retries
  successfully.

Keep these assertions passing when changing prompt-template loading.

---

## Common Mistakes

<!-- State management mistakes your team has made -->

(To be filled by the team)
