import { describe, expect, it } from "vitest";
import { supportsTransaction } from "@/lib/databaseFeatureSupport";

describe("supportsTransaction", () => {
  it("returns true for supported database types", () => {
    expect(supportsTransaction("postgres")).toBe(true);
    expect(supportsTransaction("mysql")).toBe(true);
    expect(supportsTransaction("sqlite")).toBe(true);
    expect(supportsTransaction("clickhouse")).toBe(true);
    expect(supportsTransaction("sqlserver")).toBe(true);
    expect(supportsTransaction("oracle")).toBe(true);
    expect(supportsTransaction("dameng")).toBe(true);
    expect(supportsTransaction("rqlite")).toBe(true);
    expect(supportsTransaction("agent")).toBe(true);
  });

  it("returns false for unsupported database types", () => {
    expect(supportsTransaction("redis")).toBe(false);
    expect(supportsTransaction("mongodb")).toBe(false);
    expect(supportsTransaction("duckdb")).toBe(false);
    expect(supportsTransaction("qdrant")).toBe(false);
    expect(supportsTransaction("turso")).toBe(false);
  });

  it("returns false for undefined or empty input", () => {
    expect(supportsTransaction(undefined)).toBe(false);
  });
});
