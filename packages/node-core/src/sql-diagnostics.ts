const DEFAULT_SQL_DIAGNOSTIC_MAX_CHARS = 512;
const SENSITIVE_NAME_RE = /(?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|credential|authorization|bearer)/i;

function truncateForDiagnostic(value: string, maxChars = DEFAULT_SQL_DIAGNOSTIC_MAX_CHARS): string {
  if (value.length <= maxChars) return value;
  return `${value.slice(0, maxChars)}…[truncated ${value.length - maxChars} chars]`;
}

function redactSqlLiterals(sql: string): string {
  let result = "";
  let i = 0;
  while (i < sql.length) {
    const ch = sql[i];
    const next = sql[i + 1];
    if (ch === "'" || ch === '"' || ch === "`") {
      const quote = ch;
      result += `${quote}[REDACTED]${quote}`;
      i += 1;
      while (i < sql.length) {
        const current = sql[i];
        if (current === quote) {
          if (sql[i + 1] === quote) {
            i += 2;
            continue;
          }
          i += 1;
          break;
        }
        if (current === "\\" && quote !== "`") {
          i += 2;
        } else {
          i += 1;
        }
      }
      continue;
    }
    if (ch === "$") {
      let j = i + 1;
      while (j < sql.length && sql[j] !== "$") j += 1;
      if (j >= sql.length) {
        result += "$";
        i += 1;
        continue;
      }
      const tag = sql.slice(i, j + 1);
      i = j + 1;
      while (i < sql.length) {
        if (sql.slice(i, i + tag.length) === tag) {
          i += tag.length;
          break;
        }
        i += 1;
      }
      result += "$[REDACTED]$";
      continue;
    }
    if (ch === "-" && next === "-") {
      result += "--[REDACTED_COMMENT]";
      i += 2;
      while (i < sql.length && sql[i] !== "\n" && sql[i] !== "\r") i += 1;
      continue;
    }
    if (ch === "/" && next === "*") {
      result += "/*[REDACTED_COMMENT]*/";
      i += 2;
      while (i < sql.length && !(sql[i] === "*" && sql[i + 1] === "/")) i += 1;
      if (i < sql.length) i += 2;
      continue;
    }
    result += ch;
    i += 1;
  }
  return result;
}

export function redactSqlForDiagnostics(sql: string, maxChars = DEFAULT_SQL_DIAGNOSTIC_MAX_CHARS): string {
  const literalRedacted = redactSqlLiterals(sql);
  const sensitiveRedacted = literalRedacted.replace(/\b([A-Za-z_][\w.-]*)(\s*[:=]\s*)([^\s,;)]+)/g, (match, key: string, separator: string) => {
    if (!SENSITIVE_NAME_RE.test(key)) return match;
    return `${key}${separator}[REDACTED]`;
  });
  return truncateForDiagnostic(sensitiveRedacted, maxChars);
}

export function sqlDiagnosticsEnabled(env: NodeJS.ProcessEnv = process.env): boolean {
  const value = env.DBX_SQL_DEBUG ?? env.DBX_DEBUG_SQL ?? env.DBX_MCP_DEBUG_SQL;
  return value === "1" || value?.toLowerCase() === "true";
}

export function logSqlDiagnostic(scope: string, sql: string, details: Record<string, unknown> = {}, env?: NodeJS.ProcessEnv): void {
  if (!sqlDiagnosticsEnabled(env)) return;
  console.error(`[${scope}] sql:`, JSON.stringify({ ...details, sql: redactSqlForDiagnostics(sql) }));
}
