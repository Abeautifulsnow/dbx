use serde::{Deserialize, Serialize};

/// Events emitted by the agent loop, streamed to the frontend via SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A turn in the agent loop has started.
    TurnStart { turn: u32 },
    /// Text delta from the LLM response.
    TextDelta { delta: String },
    /// Reasoning/thinking delta (for models that support it).
    ReasoningDelta { delta: String },
    /// The LLM wants to call a tool.
    ToolCallStart { tool_call_id: String, tool_name: String, args: serde_json::Value },
    /// A tool execution has completed.
    ToolCallEnd { tool_call_id: String, tool_name: String, result: serde_json::Value, is_error: bool },
    /// Partial progress update during a long-running tool execution.
    ToolExecutionUpdate { tool_call_id: String, tool_name: String, partial_result: serde_json::Value },
    /// A turn in the agent loop has ended.
    TurnEnd { turn: u32 },
    /// The agent loop has finished.
    AgentEnd { input_tokens: Option<u32>, output_tokens: Option<u32> },
    /// Context was compacted to stay within context window limits.
    ContextCompacted {
        summary: String,
        summary_tokens: u32,
        compacted_messages: usize,
        estimated_before: u32,
        estimated_after: u32,
    },
    /// A steering message was injected into the conversation.
    SteeringApplied { content: String },
    /// An error occurred during the agent loop.
    Error { message: String },
}

/// A unified tool call extracted from any provider's response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
    /// Structured explain data (QueryResult) for explain_query tool results.
    /// Set only by execute_explain_query; None for all other tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_data: Option<serde_json::Value>,
}

/// Definition of a tool available to the agent.
#[derive(Clone)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
    pub read_only: bool,
    /// Whether this tool can be executed in parallel with other tools.
    /// false = must run sequentially (e.g. execute_query).
    pub parallel_ok: bool,
    /// Optional argument validator. Receives the raw arguments JSON and returns
    /// either validated/normalized arguments with optional warnings, or an error message.
    /// When None, arguments are passed through as-is (backward compatible).
    pub validate_args: Option<ValidateArgsFn>,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("parameters", &self.parameters)
            .field("read_only", &self.read_only)
            .field("parallel_ok", &self.parallel_ok)
            .field("validate_args", &self.validate_args.map(|_| "<fn>"))
            .finish()
    }
}

/// Result of tool argument validation.
#[derive(Debug, Clone)]
pub struct ValidatedArgs {
    /// Normalized/coerced arguments to pass to the tool executor.
    pub parsed: serde_json::Value,
    /// Non-fatal warnings (e.g. "limit capped from 200 to 100").
    pub warnings: Vec<String>,
}

/// Function pointer type for tool argument validators.
pub type ValidateArgsFn = fn(&serde_json::Value) -> Result<ValidatedArgs, String>;

impl ToolDefinition {
    /// Convert to OpenAI-compatible tool JSON for the API request.
    pub fn to_openai_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters
            }
        })
    }
    /// Convert to Anthropic-compatible tool JSON.
    pub fn to_anthropic_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters
        })
    }
    /// Convert to Gemini-compatible function declaration.
    pub fn to_gemini_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "parameters": self.parameters
        })
    }
}

/// Hook called before a tool executes. Return Err(reason) to block execution.
pub type BeforeToolHook = dyn Fn(&ToolCall, &ValidatedArgs) -> Result<(), String> + Send + Sync;

/// In-session schema metadata cache. Stored per-connection in AppState with TTL.
#[derive(Default)]
pub struct SchemaCache {
    pub tables: tokio::sync::Mutex<Option<String>>,
    pub columns: tokio::sync::Mutex<std::collections::HashMap<String, String>>,
}
