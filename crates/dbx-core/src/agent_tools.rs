use std::sync::Arc;

use serde_json::json;

use crate::agent_events::{BeforeToolHook, SchemaCache, ToolCall, ToolDefinition, ToolResult, ValidatedArgs};
use crate::connection::AppState;
use crate::models::connection::DatabaseType;
use crate::query::QueryExecutionOptions;
use crate::query_execution_sql::{build_explain_sql, supports_explain_plan, supports_fk_introspection, supports_sql_query, ExplainSqlOptions};
use crate::types::QueryResult;

/// Maximum number of tables returned by list_tables tool.
const LIST_TABLES_LIMIT: usize = 200;

/// Maximum number of rows returned by execute_query tool.
const EXECUTE_QUERY_LIMIT: usize = 50;

/// Maximum number of rows returned by get_sample_data tool.
const SAMPLE_DATA_LIMIT: usize = 20;

/// Absolute maximum rows any query tool may request.
const MAX_ALLOWED_ROWS: usize = 100;

/// Borrowed context shared by a single tool execution.
pub struct ToolExecutionContext<'a> {
    pub state: &'a Arc<AppState>,
    pub connection_id: &'a str,
    pub database: &'a str,
    pub db_type: &'a DatabaseType,
    pub tools: &'a [ToolDefinition],
    pub before_hook: Option<&'a BeforeToolHook>,
    pub schema_cache: Option<&'a Arc<SchemaCache>>,
    pub on_progress: Option<&'a (dyn Fn(serde_json::Value) + Send + Sync)>,
}

/// Get read-only tool definitions (list_tables + get_columns).
pub fn read_only_tools() -> Vec<ToolDefinition> {
    vec![list_tables_tool(), get_columns_tool()]
}

/// Get all available tool definitions for the given database type.
/// Includes read-only tools plus execute_query, get_sample_data, and
/// explain_query for database types that support them.
pub fn all_tools(db_type: DatabaseType) -> Vec<ToolDefinition> {
    let mut tools = vec![list_tables_tool(), get_columns_tool()];
    if supports_sql_query(db_type) {
        tools.push(execute_query_tool());
        tools.push(get_sample_data_tool());
    }
    if supports_explain_plan(Some(db_type)) {
        tools.push(explain_query_tool());
    }
    if supports_fk_introspection(db_type) {
        tools.push(get_foreign_keys_tool());
        tools.push(get_indexes_tool());
    }
    tools
}

/// list_tables tool definition.
fn list_tables_tool() -> ToolDefinition {
    ToolDefinition {
        name: "list_tables",
        description: "List all tables and views in the current database. Returns table names, types, and comments.",
        parameters: json!({
            "type": "object",
            "properties": {
                "schema": {
                    "type": "string",
                    "description": "Schema name to list tables from (optional, defaults to current database)"
                }
            },
            "required": []
        }),
        read_only: true,
        parallel_ok: true,
        validate_args: Some(|args| {
            let schema = args.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ValidatedArgs { parsed: json!({ "schema": schema }), warnings: vec![] })
        }),
    }
}

/// get_columns tool definition.
fn get_columns_tool() -> ToolDefinition {
    ToolDefinition {
        name: "get_columns",
        description:
            "Get column definitions for a table: names, types, primary keys, nullable, defaults, and comments. \
             Use this when the user asks about table structure, column details, or field information — \
             even if some schema context was provided, this tool returns the authoritative and complete column list.",
        parameters: json!({
            "type": "object",
            "properties": {
                "table": {
                    "type": "string",
                    "description": "Table name to get columns for"
                },
                "schema": {
                    "type": "string",
                    "description": "Schema name (optional, defaults to current database)"
                }
            },
            "required": ["table"]
        }),
        read_only: true,
        parallel_ok: true,
        validate_args: Some(|args| {
            let table = args
                .get("table")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: 'table'".to_string())?
                .trim();
            if table.is_empty() {
                return Err("Table name cannot be empty".to_string());
            }
            if table.len() > 256 {
                return Err(format!("Table name too long: {} characters (max 256)", table.len()));
            }
            if table.contains(';') || table.contains('\'') || table.contains('"') || table.contains('\\') {
                return Err(format!("Table name contains invalid characters: '{}'", table));
            }
            let schema = args.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ValidatedArgs { parsed: json!({ "table": table, "schema": schema }), warnings: vec![] })
        }),
    }
}
/// execute_query tool definition.
fn execute_query_tool() -> ToolDefinition {
    ToolDefinition {
        name: "execute_query",
        description: "Execute a read-only SQL query and return results (max 50 rows). \
                      Only SELECT, WITH, SHOW, DESCRIBE, EXPLAIN statements are allowed. \
                      Write operations (INSERT/UPDATE/DELETE/DDL) are blocked.",
        parameters: json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "The SQL query to execute"
                },
                "limit": {
                    "type": "number",
                    "description": "Max rows to return (default 50, max 100)"
                }
            },
            "required": ["sql"]
        }),
        read_only: true,
        parallel_ok: false,
        validate_args: Some(|args| {
            let sql =
                args.get("sql").and_then(|v| v.as_str()).ok_or("Missing required parameter: 'sql'".to_string())?.trim();
            if sql.is_empty() {
                return Err("SQL query cannot be empty".to_string());
            }
            let mut warnings = Vec::new();
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| {
                    let l = l as usize;
                    if l < 1 {
                        EXECUTE_QUERY_LIMIT
                    } else if l > MAX_ALLOWED_ROWS {
                        warnings.push(format!("limit capped from {} to {}", l, MAX_ALLOWED_ROWS));
                        MAX_ALLOWED_ROWS
                    } else {
                        l
                    }
                })
                .unwrap_or(EXECUTE_QUERY_LIMIT);
            Ok(ValidatedArgs { parsed: json!({ "sql": sql, "limit": limit }), warnings })
        }),
    }
}

/// get_sample_data tool definition.
fn get_sample_data_tool() -> ToolDefinition {
    ToolDefinition {
        name: "get_sample_data",
        description: "Get sample rows from a table to understand its data. Returns up to 20 rows.",
        parameters: json!({
            "type": "object",
            "properties": {
                "table": {
                    "type": "string",
                    "description": "Table name"
                },
                "schema": {
                    "type": "string",
                    "description": "Schema name (optional)"
                },
                "limit": {
                    "type": "number",
                    "description": "Max rows (default 20)"
                }
            },
            "required": ["table"]
        }),
        read_only: true,
        parallel_ok: true,
        validate_args: Some(|args| {
            let table = args
                .get("table")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: 'table'".to_string())?
                .trim();
            if table.is_empty() {
                return Err("Table name cannot be empty".to_string());
            }
            if table.contains(';') || table.contains('\'') || table.contains('"') || table.contains('\\') {
                return Err(format!("Table name contains invalid characters: '{}'", table));
            }
            let schema = args.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut warnings = Vec::new();
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| {
                    let l = l as usize;
                    if l > MAX_ALLOWED_ROWS {
                        warnings.push(format!("limit capped from {} to {}", l, MAX_ALLOWED_ROWS));
                        MAX_ALLOWED_ROWS
                    } else {
                        l
                    }
                })
                .unwrap_or(SAMPLE_DATA_LIMIT);
            Ok(ValidatedArgs { parsed: json!({ "table": table, "schema": schema, "limit": limit }), warnings })
        }),
    }
}

/// explain_query tool definition (Phase 3).
fn explain_query_tool() -> ToolDefinition {
    ToolDefinition {
        name: "explain_query",
        description: "Get the execution plan for a SQL query using EXPLAIN. \
                      Shows how the database will execute the query (scan type, indexes, cost). \
                      Only read-only queries (SELECT, WITH, SHOW, DESCRIBE, EXPLAIN) are allowed. \
                      Use this to analyze query performance and suggest index optimizations.",
        parameters: json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "The SQL query to explain (must be read-only)"
                }
            },
            "required": ["sql"]
        }),
        read_only: true,
        parallel_ok: true,
        validate_args: Some(|args| {
            let sql =
                args.get("sql").and_then(|v| v.as_str()).ok_or("Missing required parameter: 'sql'".to_string())?.trim();
            if sql.is_empty() {
                return Err("SQL query cannot be empty".to_string());
            }
            Ok(ValidatedArgs { parsed: json!({ "sql": sql }), warnings: vec![] })
        }),
    }
}

/// get_foreign_keys tool definition.
fn get_foreign_keys_tool() -> ToolDefinition {
    ToolDefinition {
        name: "get_foreign_keys",
        description: "Get foreign key relationships for a table. Returns the columns, referenced tables/columns, and delete/update rules.",
        parameters: json!({
            "type": "object",
            "properties": {
                "table": {
                    "type": "string",
                    "description": "Table name to get foreign keys for"
                },
                "schema": {
                    "type": "string",
                    "description": "Schema name (optional, defaults to current database)"
                }
            },
            "required": ["table"]
        }),
        read_only: true,
        parallel_ok: true,
        validate_args: Some(|args| {
            let table = args
                .get("table")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: 'table'".to_string())?
                .trim();
            if table.is_empty() {
                return Err("Table name cannot be empty".to_string());
            }
            if table.len() > 256 {
                return Err(format!("Table name too long: {} characters (max 256)", table.len()));
            }
            if table.contains(';') || table.contains('\'') || table.contains('"') || table.contains('\\') {
                return Err(format!("Table name contains invalid characters: '{}'", table));
            }
            let schema = args.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ValidatedArgs { parsed: json!({ "table": table, "schema": schema }), warnings: vec![] })
        }),
    }
}

/// get_indexes tool definition.
fn get_indexes_tool() -> ToolDefinition {
    ToolDefinition {
        name: "get_indexes",
        description: "Get indexes for a table. Returns index names, columns, uniqueness, primary key status, and index type.",
        parameters: json!({
            "type": "object",
            "properties": {
                "table": {
                    "type": "string",
                    "description": "Table name to get indexes for"
                },
                "schema": {
                    "type": "string",
                    "description": "Schema name (optional, defaults to current database)"
                }
            },
            "required": ["table"]
        }),
        read_only: true,
        parallel_ok: true,
        validate_args: Some(|args| {
            let table = args
                .get("table")
                .and_then(|v| v.as_str())
                .ok_or("Missing required parameter: 'table'".to_string())?
                .trim();
            if table.is_empty() {
                return Err("Table name cannot be empty".to_string());
            }
            if table.len() > 256 {
                return Err(format!("Table name too long: {} characters (max 256)", table.len()));
            }
            if table.contains(';') || table.contains('\'') || table.contains('"') || table.contains('\\') {
                return Err(format!("Table name contains invalid characters: '{}'", table));
            }
            let schema = args.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ValidatedArgs { parsed: json!({ "table": table, "schema": schema }), warnings: vec![] })
        }),
    }
}

/// Execute a tool call and return the result.
pub async fn execute_tool(tool_call: &ToolCall, context: ToolExecutionContext<'_>) -> ToolResult {
    // 1. Find tool definition and run argument validator
    let tool_def = context.tools.iter().find(|t| t.name == tool_call.name);

    let validated = if let Some(td) = tool_def {
        if let Some(validator) = td.validate_args {
            match validator(&tool_call.arguments) {
                Ok(v) => v,
                Err(err) => {
                    return ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        tool_name: tool_call.name.clone(),
                        content: format!("Error: {err}"),
                        is_error: true,
                        explain_data: None,
                    };
                }
            }
        } else {
            ValidatedArgs { parsed: tool_call.arguments.clone(), warnings: Vec::new() }
        }
    } else {
        ValidatedArgs { parsed: tool_call.arguments.clone(), warnings: Vec::new() }
    };

    // 2. Run before_hook if present
    if let Some(hook) = context.before_hook {
        if let Err(reason) = hook(tool_call, &validated) {
            return ToolResult {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                content: format!("Blocked: {reason}"),
                is_error: true,
                explain_data: None,
            };
        }
    }

    // 3. Create effective ToolCall with validated/normalized args
    let effective_call =
        ToolCall { id: tool_call.id.clone(), name: tool_call.name.clone(), arguments: validated.parsed.clone() };

    let result = match effective_call.name.as_str() {
        "list_tables" => {
            execute_list_tables(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
                context.schema_cache,
            )
            .await
        }
        "get_columns" => {
            execute_get_columns(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
                context.schema_cache,
            )
            .await
        }
        "execute_query" => {
            execute_execute_query(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
            )
            .await
        }
        "get_sample_data" => {
            execute_get_sample_data(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
                context.on_progress,
            )
            .await
        }
        "explain_query" => {
            let (text_result, explain_data) = execute_explain_query(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
            )
            .await;
            let warnings = &validated.warnings;
            let prefix =
                if warnings.is_empty() { String::new() } else { format!("[WARNING] {}\n\n", warnings.join("; ")) };
            match text_result {
                Ok(content) => {
                    return ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        tool_name: tool_call.name.clone(),
                        content: format!("{prefix}{content}"),
                        is_error: false,
                        explain_data,
                    };
                }
                Err(err) => {
                    return ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        tool_name: tool_call.name.clone(),
                        content: format!("{prefix}Error: {err}"),
                        is_error: true,
                        explain_data: None,
                    };
                }
            }
        }
        "get_foreign_keys" => {
            execute_get_foreign_keys(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
            )
            .await
        }
        "get_indexes" => {
            execute_get_indexes(
                &effective_call,
                context.state,
                context.connection_id,
                context.database,
                context.db_type,
            )
            .await
        }
        _ => Err(format!("Unknown tool: {}", tool_call.name)),
    };

    let prefix = if validated.warnings.is_empty() {
        String::new()
    } else {
        format!("[WARNING] {}\n\n", validated.warnings.join("; "))
    };

    match result {
        Ok(content) => ToolResult {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            content: format!("{prefix}{content}"),
            is_error: false,
            explain_data: None,
        },
        Err(err) => ToolResult {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            content: format!("{prefix}Error: {err}"),
            is_error: true,
            explain_data: None,
        },
    }
}

async fn execute_list_tables(
    tool_call: &ToolCall,
    state: &Arc<AppState>,
    connection_id: &str,
    database: &str,
    _db_type: &DatabaseType,
    schema_cache: Option<&Arc<SchemaCache>>,
) -> Result<String, String> {
    let schema = tool_call.arguments.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Check schema cache for tables
    if let Some(cache) = schema_cache {
        let tables_mutex = cache.tables.lock().await;
        if let Some(ref cached) = *tables_mutex {
            return Ok(cached.clone());
        }
        drop(tables_mutex);
    }

    // Request one extra to detect whether more tables exist beyond the limit.
    let tables = crate::schema::list_tables_core(
        state,
        connection_id,
        database,
        &schema,
        None,
        Some(LIST_TABLES_LIMIT + 1),
        None,
    )
    .await
    .map_err(|e| format!("Failed to list tables: {e}"))?;

    let total = tables.len();
    let truncated = total > LIST_TABLES_LIMIT;

    let mut lines = Vec::new();
    let display_count = if truncated { LIST_TABLES_LIMIT } else { total };
    for table in tables.iter().take(display_count) {
        let mut line = format!("- {} ({})", table.name, table.table_type);
        if let Some(comment) = &table.comment {
            let trimmed = comment.trim();
            if !trimmed.is_empty() {
                line.push_str(&format!(" -- {}", trimmed));
            }
        }
        lines.push(line);
    }

    if truncated {
        lines.push(format!("... (showing {LIST_TABLES_LIMIT} of {total} tables)"));
    }

    let result = if lines.is_empty() {
        "No tables found in this database/schema.".to_string()
    } else {
        lines.join("\n")
    };

    // Cache result (including empty result)
    if let Some(cache) = schema_cache {
        let mut tables_mutex = cache.tables.lock().await;
        *tables_mutex = Some(result.clone());
    }

    Ok(result)
}

async fn execute_get_columns(
    tool_call: &ToolCall,
    state: &Arc<AppState>,
    connection_id: &str,
    database: &str,
    _db_type: &DatabaseType,
    schema_cache: Option<&Arc<SchemaCache>>,
) -> Result<String, String> {
    let table = tool_call
        .arguments
        .get("table")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: table")?
        .trim()
        .to_string();

    if table.is_empty() {
        return Err("Table name cannot be empty".to_string());
    }
    if table.len() > 256 {
        return Err(format!("Table name too long: {} characters (max 256)", table.len()));
    }
    // Reject names with characters that are unlikely to be valid identifiers
    if table.contains(';') || table.contains('\'') || table.contains('"') || table.contains('\\') {
        return Err(format!("Table name contains invalid characters: '{}'", table));
    }

    let schema = tool_call.arguments.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Check schema cache for columns
    let cache_key = format!("{}.{}", if schema.is_empty() { "" } else { &schema }, &table);
    if let Some(cache) = schema_cache {
        let columns_lock = cache.columns.lock().await;
        if let Some(cached) = columns_lock.get(&cache_key) {
            return Ok(cached.clone());
        }
        drop(columns_lock);
    }

    let columns = crate::schema::get_columns_core(state, connection_id, database, &schema, &table)
        .await
        .map_err(|e| format!("Failed to get columns for {table}: {e}"))?;

    let result = if columns.is_empty() {
        format!("No columns found for table '{table}'.")
    } else {
        let mut lines = Vec::new();
        lines.push(format!("Columns of {table}:"));
        for col in &columns {
            let mut flags: Vec<String> = Vec::new();
            if col.is_primary_key {
                flags.push("PK".to_string());
            }
            if col.is_nullable {
                flags.push("nullable".to_string());
            } else {
                flags.push("NOT NULL".to_string());
            }
            if let Some(default) = &col.column_default {
                if !default.is_empty() {
                    flags.push(format!("default {default}"));
                }
            }
            if let Some(extra) = &col.extra {
                if !extra.is_empty() {
                    flags.push(extra.clone());
                }
            }

            let flags_str = if flags.is_empty() { String::new() } else { format!(" ({})", flags.join(", ")) };

            let comment_str = col
                .comment
                .as_ref()
                .filter(|c| !c.trim().is_empty())
                .map(|c| format!(" -- {}", c.trim()))
                .unwrap_or_default();

            lines.push(format!("  - {}: {}{}{}", col.name, col.data_type, flags_str, comment_str));
        }
        lines.join("\n")
    };

    // Cache result (including empty result)
    if let Some(cache) = schema_cache {
        let mut columns_lock = cache.columns.lock().await;
        columns_lock.insert(cache_key, result.clone());
    }

    Ok(result)
}

/// Execute a read-only SQL query via the execute_query tool.
async fn execute_execute_query(
    tool_call: &ToolCall,
    state: &Arc<AppState>,
    connection_id: &str,
    database: &str,
    _db_type: &DatabaseType,
) -> Result<String, String> {
    let sql = tool_call.arguments.get("sql").and_then(|v| v.as_str()).ok_or("Missing required parameter: sql")?.trim();

    if sql.is_empty() {
        return Err("SQL query cannot be empty".to_string());
    }

    let limit = tool_call
        .arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|l| (l as usize).min(MAX_ALLOWED_ROWS))
        .unwrap_or(EXECUTE_QUERY_LIMIT);

    // Execute query using existing infrastructure
    let options = QueryExecutionOptions { max_rows: Some(limit), timeout_secs: Some(30), ..Default::default() };
    let result =
        crate::query::execute_sql_statement_with_options(state, connection_id, database, sql, None, None, options)
            .await?;

    format_query_result_as_text(&result, limit)
}

/// Format a QueryResult as a Markdown table for LLM consumption.
fn format_query_result_as_text(result: &QueryResult, limit: usize) -> Result<String, String> {
    if result.rows.is_empty() {
        return Ok("Query returned 0 rows.".to_string());
    }

    let mut lines = Vec::new();

    // Header row
    lines.push(format!("| {} |", result.columns.join(" | ")));
    // Separator row
    lines.push(format!("|{}|", result.columns.iter().map(|_| "---").collect::<Vec<_>>().join("|")));

    // Data rows
    for row in &result.rows {
        let cells: Vec<String> = row
            .iter()
            .map(|v| match v {
                serde_json::Value::Null => "NULL".to_string(),
                serde_json::Value::String(s) => {
                    // Truncate long strings to keep result compact
                    if s.len() > 200 {
                        let truncated: String =
                            s.char_indices().take_while(|(i, _)| *i < 200).map(|(_, c)| c).collect();
                        format!("{}...", truncated)
                    } else {
                        s.clone()
                    }
                }
                other => other.to_string(),
            })
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));
    }

    // Truncation notice
    if result.truncated || result.rows.len() >= limit {
        lines.push(format!("... (showing {} rows, result may be truncated)", result.rows.len()));
    }

    // Stats line
    lines.push(format!("({} rows, {}ms)", result.rows.len(), result.execution_time_ms));

    Ok(lines.join("\n"))
}

/// Get sample data from a table via the get_sample_data tool.
async fn execute_get_sample_data(
    tool_call: &ToolCall,
    state: &Arc<AppState>,
    connection_id: &str,
    database: &str,
    db_type: &DatabaseType,
    on_progress: Option<&(dyn Fn(serde_json::Value) + Send + Sync)>,
) -> Result<String, String> {
    let table =
        tool_call.arguments.get("table").and_then(|v| v.as_str()).ok_or("Missing required parameter: table")?.trim();

    if table.is_empty() {
        return Err("Table name cannot be empty".to_string());
    }
    if table.contains(';') || table.contains('\'') || table.contains('"') || table.contains('\\') {
        return Err(format!("Table name contains invalid characters: '{}'", table));
    }

    let schema = tool_call.arguments.get("schema").and_then(|v| v.as_str());
    let limit = tool_call
        .arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|l| (l as usize).min(MAX_ALLOWED_ROWS))
        .unwrap_or(SAMPLE_DATA_LIMIT);

    // Report sampling phase start
    if let Some(progress) = on_progress {
        progress(serde_json::json!({
            "phase": "sampling",
            "rows_collected": 0
        }));
    }

    // Build SELECT * FROM table LIMIT N
    let schema_prefix = schema.filter(|s| !s.is_empty()).map(|s| format!("\"{}\".", s)).unwrap_or_default();
    let sql = format!("SELECT * FROM {}\"{}\" LIMIT {}", schema_prefix, table, limit);

    // Delegate to execute_execute_query with a synthetic tool call
    let synthetic_call = ToolCall {
        id: tool_call.id.clone(),
        name: "execute_query".to_string(),
        arguments: serde_json::json!({ "sql": sql, "limit": limit }),
    };
    let result = execute_execute_query(&synthetic_call, state, connection_id, database, db_type).await;

    // Report completion progress
    if let Some(progress) = on_progress {
        let rows = match &result {
            Ok(content) => {
                // Count rows by counting lines that start with "|" (after header+separator)
                content.lines().filter(|l| l.starts_with('|')).count().saturating_sub(1) // subtract header row
            }
            Err(_) => 0,
        };
        progress(serde_json::json!({
            "phase": "sampling",
            "rows_collected": rows
        }));
    }

    result
}

/// Execute an EXPLAIN query via the explain_query tool.
/// Returns (text_for_llm, optional_explain_data_for_frontend).
async fn execute_explain_query(
    tool_call: &ToolCall,
    state: &Arc<AppState>,
    connection_id: &str,
    database: &str,
    db_type: &DatabaseType,
) -> (Result<String, String>, Option<serde_json::Value>) {
    let sql = match tool_call.arguments.get("sql").and_then(|v| v.as_str()) {
        Some(s) => s.trim(),
        None => return (Err("Missing required parameter: sql".to_string()), None),
    };

    if sql.is_empty() {
        return (Err("SQL query cannot be empty".to_string()), None);
    }

    // Build the database-specific EXPLAIN SQL
    let explain_result = build_explain_sql(ExplainSqlOptions { database_type: Some(*db_type), sql: sql.to_string() });

    let explain_sql = match (explain_result.ok, explain_result.sql) {
        (true, Some(sql)) => sql,
        (true, None) => return (Err("EXPLAIN SQL is empty".to_string()), None),
        (false, _) => {
            let reason = explain_result.reason.unwrap_or_else(|| "unknown".to_string());
            return (Err(format!("Cannot explain this query: {}. The database type may not support EXPLAIN, or the query may be unsafe.", reason)), None);
        }
    };

    // Execute the EXPLAIN query
    let options = QueryExecutionOptions { max_rows: Some(100), timeout_secs: Some(30), ..Default::default() };
    let result = match crate::query::execute_sql_statement_with_options(
        state,
        connection_id,
        database,
        &explain_sql,
        None,
        None,
        options,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return (Err(e), None),
    };

    // Serialize the raw QueryResult for the frontend ExplainPlanViewer
    let explain_data = serde_json::to_value(&result).ok();
    let text = match format_query_result_as_text(&result, 100) {
        Ok(t) => t,
        Err(e) => return (Err(e), None),
    };

    (Ok(text), explain_data)
}

/// Build the SQL query to retrieve foreign keys for a given table and schema.
fn build_fk_sql(table: &str, schema: &str, database: &str, db_type: &DatabaseType) -> Option<String> {
    match db_type {
        DatabaseType::Postgres
        | DatabaseType::Redshift
        | DatabaseType::OpenGauss
        | DatabaseType::Gaussdb
        | DatabaseType::Vastbase
        | DatabaseType::Kingbase => {
            let schema = if schema.is_empty() { "public" } else { schema };
            Some(format!(
                "SELECT kcu.column_name, ccu.table_schema AS referenced_schema, \
                 ccu.table_name AS referenced_table, ccu.column_name AS referenced_column, \
                 rc.delete_rule, rc.update_rule \
                 FROM information_schema.table_constraints tc \
                 JOIN information_schema.key_column_usage kcu \
                 ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema \
                 JOIN information_schema.referential_constraints rc \
                 ON tc.constraint_name = rc.constraint_name \
                 JOIN information_schema.constraint_column_usage ccu \
                 ON rc.unique_constraint_name = ccu.constraint_name \
                 WHERE tc.constraint_type = 'FOREIGN KEY' \
                 AND tc.table_schema = '{schema}' AND tc.table_name = '{table}'"
            ))
        }
        DatabaseType::Mysql | DatabaseType::Doris | DatabaseType::StarRocks => Some(format!(
            "SELECT kcu.column_name, kcu.referenced_table_schema, kcu.referenced_table_name, \
             kcu.referenced_column_name, rc.delete_rule, rc.update_rule \
             FROM information_schema.key_column_usage kcu \
             JOIN information_schema.referential_constraints rc \
             ON kcu.constraint_name = rc.constraint_name \
             AND kcu.constraint_schema = rc.constraint_schema \
             WHERE kcu.table_schema = '{database}' AND kcu.table_name = '{table}' \
             AND kcu.referenced_table_name IS NOT NULL"
        )),
        DatabaseType::Sqlite | DatabaseType::Rqlite | DatabaseType::Turso => {
            Some(format!("PRAGMA foreign_key_list('{table}')"))
        }
        DatabaseType::SqlServer => Some(format!(
            "SELECT fkc.name AS column_name, rt.name AS referenced_table, \
             rc.name AS referenced_column, \
             fk.delete_referential_action_desc AS delete_rule, \
             fk.update_referential_action_desc AS update_rule \
             FROM sys.foreign_keys fk \
             JOIN sys.foreign_key_columns fkcc ON fk.object_id = fkcc.constraint_object_id \
             JOIN sys.columns fkc ON fkcc.parent_column_id = fkc.column_id AND fkcc.parent_object_id = fkc.object_id \
             JOIN sys.tables rt ON fkcc.referenced_object_id = rt.object_id \
             JOIN sys.columns rc ON fkcc.referenced_column_id = rc.column_id AND fkcc.referenced_object_id = rc.object_id \
             WHERE OBJECT_NAME(fk.parent_object_id) = '{table}'"
        )),
        DatabaseType::Oracle => {
            let schema_upper = if schema.is_empty() { database.to_uppercase() } else { schema.to_uppercase() };
            Some(format!(
                "SELECT acc.column_name, ac2.table_name AS referenced_table, \
                 acc2.column_name AS referenced_column, ac.delete_rule \
                 FROM all_constraints ac \
                 JOIN all_cons_columns acc ON ac.constraint_name = acc.constraint_name AND ac.owner = acc.owner \
                 JOIN all_constraints ac2 ON ac.r_constraint_name = ac2.constraint_name AND ac.r_owner = ac2.owner \
                 JOIN all_cons_columns acc2 ON ac2.constraint_name = acc2.constraint_name AND ac2.owner = acc2.owner \
                 WHERE ac.constraint_type = 'R' \
                 AND ac.owner = '{schema_upper}' AND ac.table_name = UPPER('{table}')"
            ))
        }
        _ => None,
    }
}

/// Build the SQL query to retrieve indexes for a given table and schema.
fn build_index_sql(table: &str, schema: &str, database: &str, db_type: &DatabaseType) -> Option<String> {
    match db_type {
        DatabaseType::Postgres
        | DatabaseType::Redshift
        | DatabaseType::OpenGauss
        | DatabaseType::Gaussdb
        | DatabaseType::Vastbase
        | DatabaseType::Kingbase => {
            let schema = if schema.is_empty() { "public" } else { schema };
            Some(format!(
                "SELECT pi.indexname, pi.indexdef, \
                 CASE WHEN ix.indisunique THEN 'YES' ELSE 'NO' END AS is_unique, \
                 CASE WHEN ix.indisprimary THEN 'YES' ELSE 'NO' END AS is_primary \
                 FROM pg_indexes pi \
                 JOIN pg_class c ON c.relname = pi.tablename \
                 JOIN pg_index ix ON c.oid = ix.indrelid \
                 JOIN pg_class ic ON ic.oid = ix.indexrelid AND ic.relname = pi.indexname \
                 WHERE pi.schemaname = '{schema}' AND pi.tablename = '{table}'"
            ))
        }
        DatabaseType::Mysql | DatabaseType::Doris | DatabaseType::StarRocks => Some(format!(
            "SELECT index_name, GROUP_CONCAT(column_name ORDER BY seq_in_index) AS columns, \
             IF(non_unique=0,'YES','NO') AS is_unique, index_type \
             FROM information_schema.statistics \
             WHERE table_schema = '{database}' AND table_name = '{table}' \
             GROUP BY index_name, non_unique, index_type"
        )),
        DatabaseType::Sqlite | DatabaseType::Rqlite | DatabaseType::Turso => {
            Some(format!("PRAGMA index_list('{table}')"))
        }
        DatabaseType::SqlServer => Some(format!(
            "SELECT i.name AS index_name, \
             STRING_AGG(c.name, ', ') WITHIN GROUP (ORDER BY ic.key_ordinal) AS columns, \
             CASE WHEN i.is_unique = 1 THEN 'YES' ELSE 'NO' END AS is_unique, \
             CASE WHEN i.is_primary_key = 1 THEN 'YES' ELSE 'NO' END AS is_primary, \
             i.type_desc AS index_type \
             FROM sys.indexes i \
             JOIN sys.index_columns ic ON i.object_id = ic.object_id AND i.index_id = ic.index_id \
             JOIN sys.columns c ON ic.object_id = c.object_id AND ic.column_id = c.column_id \
             WHERE OBJECT_NAME(i.object_id) = '{table}' AND i.is_hypothetical = 0 \
             GROUP BY i.name, i.is_unique, i.is_primary_key, i.type_desc"
        )),
        DatabaseType::Oracle => {
            let schema_upper = if schema.is_empty() { database.to_uppercase() } else { schema.to_uppercase() };
            Some(format!(
                "SELECT ai.index_name, aic.column_name, ai.uniqueness, \
                 CASE WHEN ac.constraint_type = 'P' THEN 'YES' ELSE 'NO' END AS is_primary, \
                 ai.index_type \
                 FROM all_indexes ai \
                 JOIN all_ind_columns aic ON ai.index_name = aic.index_name AND ai.owner = aic.index_owner \
                 LEFT JOIN all_constraints ac ON ai.index_name = ac.index_name AND ai.owner = ac.owner AND ac.constraint_type = 'P' \
                 WHERE ai.owner = '{schema_upper}' AND ai.table_name = UPPER('{table}') \
                 ORDER BY ai.index_name, aic.column_position"
            ))
        }
        _ => None,
    }
}

/// Format a QueryResult as a padded text table with a title and empty-message fallback.
fn format_query_result_table(result: &QueryResult, title: &str, empty_msg: &str) -> String {
    if result.rows.is_empty() {
        return empty_msg.to_string();
    }
    let mut lines = vec![title.to_string()];
    let columns = &result.columns;
    let mut col_widths: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for row in &result.rows {
        for (i, col) in columns.iter().enumerate() {
            let val = row_value_as_str(&row[i]);
            let entry = col_widths.entry(col.as_str()).or_insert(col.len());
            *entry = (*entry).max(val.len());
        }
    }
    let header: Vec<String> =
        columns.iter().map(|c| format!("{:width$}", c, width = col_widths[c.as_str()])).collect();
    let separator: Vec<String> = columns.iter().map(|c| "-".repeat(col_widths[c.as_str()])).collect();
    lines.push(format!("  {}", header.join(" | ")));
    lines.push(format!("  {}", separator.join(" | ")));
    for row in &result.rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:width$}", row_value_as_str(v), width = col_widths[columns[i].as_str()]))
            .collect();
        lines.push(format!("  {}", cells.join(" | ")));
    }
    lines.join("\n")
}

fn format_fk_result(result: &QueryResult, table: &str) -> String {
    format_query_result_table(result, &format!("Foreign keys for \"{table}\":"), &format!("No foreign keys found for \"{table}\"."))
}

fn format_index_result(result: &QueryResult, table: &str) -> String {
    format_query_result_table(result, &format!("Indexes for \"{table}\":"), &format!("No indexes found for \"{table}\"."))
}

/// Convert a serde_json::Value row cell to a string for display.
fn row_value_as_str(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Execute a table introspection query (FK or index) with a given SQL builder and result formatter.
#[allow(clippy::too_many_arguments)]
async fn execute_introspection(
    tool_call: &ToolCall,
    state: &Arc<AppState>,
    connection_id: &str,
    database: &str,
    db_type: &DatabaseType,
    build_sql: fn(&str, &str, &str, &DatabaseType) -> Option<String>,
    format_result: fn(&QueryResult, &str) -> String,
    kind: &str,
) -> Result<String, String> {
    let table = tool_call
        .arguments
        .get("table")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: table")?
        .trim();
    if table.is_empty() {
        return Err("Table name cannot be empty".to_string());
    }
    let schema = tool_call.arguments.get("schema").and_then(|v| v.as_str()).unwrap_or("");
    let sql = build_sql(table, schema, database, db_type)
        .ok_or_else(|| format!("{kind} introspection not supported for {:?}", db_type))?;
    let options = QueryExecutionOptions { max_rows: Some(200), timeout_secs: Some(30), ..Default::default() };
    let result = crate::query::execute_sql_statement_with_options(
        state, connection_id, database, &sql, None, None, options,
    )
    .await?;
    Ok(format_result(&result, table))
}

async fn execute_get_foreign_keys(
    tc: &ToolCall,
    state: &Arc<AppState>,
    conn: &str,
    db: &str,
    db_type: &DatabaseType,
) -> Result<String, String> {
    execute_introspection(tc, state, conn, db, db_type, build_fk_sql, format_fk_result, "FK").await
}

async fn execute_get_indexes(
    tc: &ToolCall,
    state: &Arc<AppState>,
    conn: &str,
    db: &str,
    db_type: &DatabaseType,
) -> Result<String, String> {
    execute_introspection(tc, state, conn, db, db_type, build_index_sql, format_index_result, "Index").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::QueryResult;

    fn make_result(columns: &[&str], rows: &[&[&str]]) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            rows: rows
                .iter()
                .map(|row| row.iter().map(|v| serde_json::Value::String(v.to_string())).collect())
                .collect(),
            column_types: vec![],
            column_sortables: vec![],
            affected_rows: 0,
            execution_time_ms: 0,
            truncated: false,
            session_id: None,
            has_more: false,
        }
    }

    #[test]
    fn format_fk_result_empty_returns_no_fk_message() {
        let result = make_result(&["a"], &[]);
        assert_eq!(format_fk_result(&result, "orders"), "No foreign keys found for \"orders\".");
    }

    #[test]
    fn format_fk_result_formats_rows() {
        let result = make_result(
            &["column_name", "referenced_table", "referenced_column"],
            &[&["user_id", "users", "id"]],
        );
        let out = format_fk_result(&result, "orders");
        assert!(out.contains("Foreign keys for \"orders\":"), "header missing: {out}");
        assert!(out.contains("user_id"), "row data missing: {out}");
        assert!(out.contains("users"), "ref table missing: {out}");
    }

    #[test]
    fn format_index_result_empty_returns_no_index_message() {
        let result = make_result(&["index_name"], &[]);
        assert_eq!(format_index_result(&result, "users"), "No indexes found for \"users\".");
    }

    #[test]
    fn format_index_result_formats_rows() {
        let result = make_result(
            &["index_name", "columns", "is_unique"],
            &[&["PRIMARY", "id", "YES"]],
        );
        let out = format_index_result(&result, "users");
        assert!(out.contains("Indexes for \"users\":"), "header missing: {out}");
        assert!(out.contains("PRIMARY"), "index name missing: {out}");
    }
}
