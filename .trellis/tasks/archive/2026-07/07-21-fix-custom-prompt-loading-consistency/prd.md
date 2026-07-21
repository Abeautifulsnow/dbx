# Fix custom prompt loading consistency

## Goal

Ensure DBX AI requests never silently omit configured global custom instructions during startup, and keep prompt-template UI validation and selection state consistent with backend behavior.

## Requirements

* Await the single in-flight prompt-template load before sending an AI request.
* Block an AI request and show a localized error when custom instructions cannot be loaded.
* Preserve retry behavior after a failed startup load.
* Count Unicode characters in the frontend with semantics matching Rust `str::chars().count()`.
* Remove deleted template IDs from each AI panel's active selection and render selection labels from valid templates.
* Add focused regression tests for the new shared character-count utility.

## Acceptance Criteria

* [ ] A request sent while prompt-template initialization is in progress waits for the existing load and receives the loaded global instructions.
* [ ] A failed load prevents the request from being sent with an incomplete system prompt.
* [ ] Emoji and other non-BMP characters have matching frontend and backend character-limit behavior.
* [ ] Deleting an active template clears its stale per-panel selection state.
* [ ] Targeted tests, type checking, linting, Rust checks, formatting, and diff checks pass.

## Definition of Done

* Regression tests cover the character-count utility.
* Existing prompt-template storage and prompt-building tests remain green.
* No unrelated working-tree changes are modified.

## Technical Approach

* Replace the prompt-template store's transient loading flag behavior with a shared in-flight `Promise<boolean>`.
* Await `ensureLoaded()` in `AiAssistant.send()` before mutating conversation state or invoking the agent stream.
* Add a shared `promptTemplateCharacterCount()` helper based on `Array.from()` and use it for all relevant UI limits and counters.
* Watch the template collection in each assistant panel and filter stale active template IDs.

## Decision (ADR-lite)

**Context**: Global custom instructions are an always-on system-prompt contract, so sending while their storage load is unresolved would silently weaken configured behavior.

**Decision**: Block sending if the prompt data cannot be loaded, while retaining an idempotent retry path for future user actions.

**Consequences**: A temporarily unavailable backend produces an explicit retryable error instead of a silently incomplete AI request.

## Out of Scope

* Persisting scenario-template selection across application restarts.
* Changing prompt-template storage schema or cloud-sync behavior.
* Refactoring unrelated DBX Web `AppError` changes already present on the branch.

## Technical Notes

* Primary files: `apps/desktop/src/stores/promptTemplateStore.ts`, `apps/desktop/src/components/editor/AiAssistant.vue`, `apps/desktop/src/components/editor/EditorSettingsDialog.vue`, and `apps/desktop/src/types/promptTemplate.ts`.
* Existing prompt construction tests are in `packages/app-tests/aiCustomPrompt.test.ts`.
