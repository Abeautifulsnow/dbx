import { strict as assert } from "node:assert";
import { test } from "vitest";
import { buildAppendedEditorSql } from "../../apps/desktop/src/lib/ai/aiSqlAppend.ts";

test("buildAppendedEditorSql returns newSql unchanged when editor is empty", () => {
  assert.equal(buildAppendedEditorSql("", "SELECT 1"), "SELECT 1");
});

test("buildAppendedEditorSql prepends blank-line separator when editor has content", () => {
  assert.equal(buildAppendedEditorSql("SELECT 1", "SELECT 2"), "SELECT 1\n\nSELECT 2");
});

test("buildAppendedEditorSql preserves multiline existing content", () => {
  assert.equal(
    buildAppendedEditorSql("SELECT *\nFROM users", "SELECT *\nFROM orders"),
    "SELECT *\nFROM users\n\nSELECT *\nFROM orders",
  );
});

test("buildAppendedEditorSql collapses trailing newlines into a single blank-line separator", () => {
  assert.equal(buildAppendedEditorSql("SELECT 1\n\n\n", "SELECT 2"), "SELECT 1\n\nSELECT 2");
});
