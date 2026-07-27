import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const dataGridSource = readFileSync(new URL("../DataGrid.vue", import.meta.url), "utf8");

describe("DataGrid native clipboard regions", () => {
  it("keeps table info text selection out of grid copy shortcuts", () => {
    // Match the table-info-drawer opening <div> tag (attributes may span multiple lines)
    const drawerTag = dataGridSource.match(/<div[^>]*v-if="showTableInfo"[^>]*>/)?.[0];

    expect(drawerTag).toBeDefined();
    expect(drawerTag).toContain("data-native-clipboard");
  });
});
