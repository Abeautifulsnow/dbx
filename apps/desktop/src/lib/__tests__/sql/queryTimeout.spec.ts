import { describe, expect, it } from "vitest";
import { frontendQueryTimeoutDelayMs, frontendQueryTimeoutSecsForSql, metadataLoadTimeoutMs, queryTimeoutSecsForConnection } from "@/lib/sql/queryTimeout";

describe("queryTimeout", () => {
  it("lets PostgreSQL row queries use the backend inactivity timeout", () => {
    expect(frontendQueryTimeoutSecsForSql("SELECT * FROM sample_records LIMIT 2000", "postgres", 30)).toBe(0);
    expect(frontendQueryTimeoutSecsForSql("/* page */\nWITH rows AS (SELECT 1) SELECT * FROM rows", "postgres", 30)).toBe(0);
    expect(frontendQueryTimeoutSecsForSql("UPDATE sample_records SET state = 'ready' RETURNING id", "postgres", 30)).toBe(0);
  });

  it("keeps the frontend guard for non-row PostgreSQL statements", () => {
    expect(frontendQueryTimeoutSecsForSql("UPDATE sample_records SET state = 'ready'", "postgres", 30)).toBe(60);
    expect(frontendQueryTimeoutSecsForSql("INSERT INTO sample_records(note) VALUES ('RETURNING is text')", "postgres", 30)).toBe(60);
    expect(frontendQueryTimeoutSecsForSql("UPDATE sample_records SET note = 'ready' /* RETURNING */", "postgres", 30)).toBe(60);
  });

  it("keeps the existing frontend guard for other database types", () => {
    expect(frontendQueryTimeoutSecsForSql("SELECT * FROM sample_records LIMIT 2000", "mysql", 30)).toBe(60);
    expect(queryTimeoutSecsForConnection({ query_timeout_secs: undefined })).toBe(30);
  });

  it("does not schedule frontend timeouts beyond the browser timer limit", () => {
    expect(frontendQueryTimeoutDelayMs(60)).toBe(60_000);
    expect(frontendQueryTimeoutDelayMs(11_401_200)).toBeUndefined();
    expect(frontendQueryTimeoutDelayMs(0)).toBeUndefined();
  });

  it("uses the global timeout only for inheriting connections", () => {
    expect(queryTimeoutSecsForConnection({ query_timeout_secs: 30, query_timeout_inherit: true }, 12)).toBe(12);
    expect(queryTimeoutSecsForConnection({ query_timeout_secs: 30, query_timeout_inherit: false }, 12)).toBe(30);
    expect(queryTimeoutSecsForConnection({ query_timeout_secs: 0, query_timeout_inherit: false }, 12)).toBe(0);
  });

  it("falls back safely when an inherited global timeout is invalid", () => {
    expect(queryTimeoutSecsForConnection({ query_timeout_inherit: true }, Number.NaN)).toBe(30);
  });

  it("resolves the metadata load deadline from the inherited global timeout", () => {
    expect(metadataLoadTimeoutMs({ query_timeout_secs: 30, query_timeout_inherit: true }, 120)).toBe(125_000);
    expect(metadataLoadTimeoutMs({ query_timeout_secs: 30, query_timeout_inherit: true })).toBe(35_000);
  });

  it("uses the local query timeout for the metadata load deadline when not inheriting", () => {
    expect(metadataLoadTimeoutMs({ query_timeout_secs: 45, query_timeout_inherit: false }, 120)).toBe(50_000);
  });

  it("gives a fixed window when the query timeout is disabled (0 = unlimited)", () => {
    expect(metadataLoadTimeoutMs({ query_timeout_secs: 0, query_timeout_inherit: false }, 120)).toBe(65_000);
    expect(metadataLoadTimeoutMs({ query_timeout_secs: 30, query_timeout_inherit: true }, 0)).toBe(65_000);
  });

  it("floors the metadata load deadline and defaults safely", () => {
    expect(metadataLoadTimeoutMs(undefined)).toBe(35_000);
    expect(metadataLoadTimeoutMs({})).toBe(35_000);
    expect(metadataLoadTimeoutMs({ query_timeout_secs: 1 }, 120)).toBe(15_000);
  });
});
