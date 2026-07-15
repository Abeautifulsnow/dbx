const DEFAULT_SQL_DIAGNOSTIC_MAX_CHARS: usize = 512;

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("passwd")
        || key == "pwd"
        || key.contains("secret")
        || key.contains("token")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("access_key")
        || key.contains("private_key")
        || key.contains("credential")
        || key.contains("authorization")
        || key.contains("bearer")
}

fn truncate_for_diagnostics(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let head: String = value.chars().take(max_chars).collect();
    let omitted = value.chars().count().saturating_sub(max_chars);
    format!("{head}…[truncated {omitted} chars]")
}

fn redact_literals(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();
        if matches!(ch, '\'' | '"' | '`') {
            out.push(ch);
            out.push_str("[REDACTED]");
            out.push(ch);
            i += 1;
            while i < chars.len() {
                let current = chars[i];
                if current == ch {
                    if chars.get(i + 1).copied() == Some(ch) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                if current == '\\' && ch != '`' {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if ch == '-' && next == Some('-') {
            out.push_str("--[REDACTED_COMMENT]");
            i += 2;
            while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && next == Some('*') {
            out.push_str("/*[REDACTED_COMMENT]*/");
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < chars.len() {
                i += 2;
            }
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn redact_sensitive_assignments(sql: &str) -> String {
    sql.split_whitespace()
        .map(|part| {
            for separator in ['=', ':'] {
                if let Some((key, _value)) = part.split_once(separator) {
                    if is_sensitive_key(key.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-'))
                    {
                        return format!("{key}{separator}[REDACTED]");
                    }
                }
            }
            part.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn redact_sql_for_diagnostics(sql: &str) -> String {
    truncate_for_diagnostics(redact_sensitive_assignments(&redact_literals(sql)), DEFAULT_SQL_DIAGNOSTIC_MAX_CHARS)
}

pub fn debug_sql(scope: &str, sql: &str) {
    log::debug!("[{scope}] sql={}", redact_sql_for_diagnostics(sql));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_literals_and_bounds_large_sql() {
        let sql = format!("select * from users where password = 'secret-123' and api_key=abc {};", "x".repeat(900));
        let redacted = redact_sql_for_diagnostics(&sql);
        assert!(!redacted.contains("secret-123"));
        assert!(!redacted.contains("api_key=abc"));
        assert!(redacted.contains("'[REDACTED]'"));
        assert!(redacted.contains("api_key=[REDACTED]"));
        assert!(redacted.contains("truncated"));
        assert!(redacted.len() < sql.len());
    }
}
