import { splitSqlStatementRanges } from "@/lib/sql/sqlStatementRanges";
import { tokenizeSqlSemantic } from "@/lib/sql/semantic/tokens";
import type { ConnectionConfig, DatabaseType } from "@/types/database";

export const DEFAULT_QUERY_TIMEOUT_SECS = 30;

/**
 * Query timeout (seconds) the table structure editor applies when the change
 * script contains a PostgreSQL `CREATE INDEX CONCURRENTLY`.
 *
 * Concurrent builds scan the whole table while only taking `ShareUpdateExclusiveLock`,
 * so they routinely run far beyond the default 30s query timeout — the default
 * budget would cancel a legitimate build and leave an INVALID index behind.
 * PostgreSQL itself does not impose a build deadline
 * (https://www.postgresql.org/docs/current/sql-createindex.html). This is the
 * dedicated 30-minute floor used by `queryTimeoutSecsForConcurrentIndex` for
 * such statements; it never overrides an unlimited setting (0) and never
 * shortens a larger configured timeout.
 */
export const CONCURRENT_INDEX_QUERY_TIMEOUT_SECS = 1800;
const MAX_BROWSER_TIMER_DELAY_MS = 2_147_483_647;

const POSTGRES_ROW_STATEMENT_KEYWORDS = new Set(["select", "show", "explain", "table", "with"]);
const POSTGRES_RETURNING_STATEMENT_KEYWORDS = new Set(["insert", "update", "delete", "merge"]);

export function queryTimeoutSecsForConnection(connection?: Pick<ConnectionConfig, "query_timeout_secs" | "query_timeout_inherit"> | null, globalQueryTimeoutSecs = DEFAULT_QUERY_TIMEOUT_SECS): number {
  if (connection?.query_timeout_inherit === true) {
    const globalValue = Number(globalQueryTimeoutSecs);
    return Number.isFinite(globalValue) && globalValue >= 0 ? globalValue : DEFAULT_QUERY_TIMEOUT_SECS;
  }
  const value = Number(connection?.query_timeout_secs);
  return Number.isFinite(value) && value >= 0 ? value : DEFAULT_QUERY_TIMEOUT_SECS;
}

/** Floor for the metadata-load deadline so slow WAN/tunnel round-trips don't trip a too-tight guard. */
export const METADATA_LOAD_MIN_TIMEOUT_MS = 15_000;
/** Fixed window when the query timeout is disabled (0 = unlimited): the 60s
 * backend metadata fallback budget plus a ~5s connect round-trip buffer, so the
 * PostgreSQL diagnostic can surface before the frontend generic timeout. */
export const METADATA_LOAD_DISABLED_QUERY_TIMEOUT_MS = 65_000;

/**
 * Deadline for silent metadata loads (e.g. the visible-databases picker).
 * Resolves the effective query timeout (inheritance + global via
 * queryTimeoutSecsForConnection), then a configured timeout gets a 5s buffer
 * with a floor; 0 (query timeout disabled) gets a fixed window so "unlimited"
 * isn't silently capped at the default bound. The deadline equals the backend
 * metadata budget (the effective query timeout, or the 60s fallback when
 * disabled) plus a ~5s connect round-trip buffer; the backend draws its
 * best-effort cancel allowance from inside the budget, so the whole metadata
 * call (query + cancel) returns within the budget the deadline is aligned to.
 */
export function metadataLoadTimeoutMs(connection?: Pick<ConnectionConfig, "query_timeout_secs" | "query_timeout_inherit"> | null, globalQueryTimeoutSecs = DEFAULT_QUERY_TIMEOUT_SECS): number {
  const queryTimeoutSecs = queryTimeoutSecsForConnection(connection, globalQueryTimeoutSecs);
  if (queryTimeoutSecs === 0) return METADATA_LOAD_DISABLED_QUERY_TIMEOUT_MS;
  return Math.max(METADATA_LOAD_MIN_TIMEOUT_MS, (queryTimeoutSecs + 5) * 1000);
}

/**
 * Effective query timeout (seconds) for a table-structure change script.
 *
 * Concurrent builds use at least the dedicated 30-minute floor while
 * preserving an unlimited setting (0) and any larger configured timeout.
 * Non-concurrent scripts keep the configured timeout unchanged.
 */
export function queryTimeoutSecsForConcurrentIndex(configuredTimeoutSecs: number, hasConcurrentIndexBuild: boolean): number {
  if (!hasConcurrentIndexBuild) return configuredTimeoutSecs;
  if (configuredTimeoutSecs === 0) return 0;
  return Math.max(configuredTimeoutSecs, CONCURRENT_INDEX_QUERY_TIMEOUT_SECS);
}

export function frontendQueryTimeoutSecsForSql(sql: string, databaseType: DatabaseType | undefined, queryTimeoutSecs: number): number {
  if (queryTimeoutSecs === 0) return 0;

  // PostgreSQL applies the configured timeout while the backend receives rows
  // over a slow SSH tunnel. An absolute browser-side deadline can reject a
  // successful, steadily progressing page fetch first, so let the backend own
  // the timeout for row-returning PostgreSQL statements.
  if (databaseType === "postgres" && postgresQueryMayReturnRows(sql, databaseType)) return 0;

  const baseTimeoutSecs = Math.max(queryTimeoutSecs * 2, 60);
  const statementCount = Math.max(splitSqlStatementRanges(sql, databaseType).length, 1);
  return baseTimeoutSecs * statementCount;
}

export function frontendQueryTimeoutDelayMs(timeoutSecs: number): number | undefined {
  const timeoutMs = timeoutSecs * 1000;
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > MAX_BROWSER_TIMER_DELAY_MS) return undefined;
  return timeoutMs;
}

function postgresQueryMayReturnRows(sql: string, databaseType: DatabaseType): boolean {
  return splitSqlStatementRanges(sql, databaseType).some(({ sql: statement }) => {
    const tokens = tokenizeSqlSemantic(statement, "postgres");
    const firstKeyword = tokens.find((token) => token.kind === "word" && token.depth === 0)?.normalized;
    if (!firstKeyword) return false;
    if (POSTGRES_ROW_STATEMENT_KEYWORDS.has(firstKeyword)) return true;
    if (!POSTGRES_RETURNING_STATEMENT_KEYWORDS.has(firstKeyword)) return false;
    return tokens.some((token) => token.kind === "word" && token.depth === 0 && token.normalized === "returning");
  });
}
