//! The slice of the Debug Adapter Protocol the editor shows: breakpoints,
//! threads, stack frames, scopes, variables, and the events that drive a
//! session. Everything else stays `serde_json::Value`.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default)]
    pub supports_configuration_done_request: bool,
    #[serde(default)]
    pub supports_terminate_request: bool,
    #[serde(default)]
    pub supports_restart_request: bool,
    #[serde(default)]
    pub supports_conditional_breakpoints: bool,
    #[serde(default)]
    pub supports_evaluate_for_hovers: bool,
    #[serde(default)]
    pub exception_breakpoint_filters: Vec<ExceptionFilter>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionFilter {
    pub filter: String,
    pub label: String,
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub source: Option<Source>,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub column: u32,
    /// `subtle`, `label`, `normal` or absent.
    #[serde(default)]
    pub presentation_hint: Option<String>,
}

impl StackFrame {
    pub fn path(&self) -> Option<&str> {
        self.source.as_ref().and_then(|s| s.path.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub name: String,
    pub variables_reference: i64,
    #[serde(default)]
    pub expensive: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub r#type: Option<String>,
    /// Non-zero: the value has children (`variables` fetches them).
    #[serde(default)]
    pub variables_reference: i64,
    #[serde(default)]
    pub evaluate_name: Option<String>,
}

/// `stopped` event body.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stopped {
    pub reason: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub thread_id: Option<i64>,
    #[serde(default)]
    pub all_threads_stopped: bool,
    #[serde(default)]
    pub text: Option<String>,
}

/// `output` event body.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// `console`, `stdout`, `stderr`, `telemetry`, `important`, or absent.
    #[serde(default)]
    pub category: Option<String>,
    pub output: String,
}

/// `evaluate` response body.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evaluated {
    pub result: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub variables_reference: i64,
}

/// Parse the `body` of a response as a typed list under `key`.
pub fn list<T: for<'de> Deserialize<'de>>(body: &Value, key: &str) -> Vec<T> {
    body.get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_and_variables_parse_with_optional_fields() {
        let body = json!({"stackFrames": [
            {"id": 1, "name": "main", "line": 3, "column": 1, "source": {"path": "/a.py", "name": "a.py"}},
            {"id": 2, "name": "<module>", "line": 0}
        ]});
        let frames: Vec<StackFrame> = list(&body, "stackFrames");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].path(), Some("/a.py"));
        assert_eq!(frames[1].path(), None);
        let vars: Vec<Variable> = list(
            &json!({"variables": [{"name": "x", "value": "1", "type": "int", "variablesReference": 0}]}),
            "variables",
        );
        assert_eq!(vars[0].r#type.as_deref(), Some("int"));
        let caps: Capabilities =
            serde_json::from_value(json!({"supportsConfigurationDoneRequest": true,
            "exceptionBreakpointFilters": [{"filter": "raised", "label": "Raised Exceptions"}]}))
            .unwrap();
        assert!(caps.supports_configuration_done_request);
        assert_eq!(caps.exception_breakpoint_filters[0].filter, "raised");
        let empty: Vec<Thread> = list(&json!({}), "threads");
        assert!(empty.is_empty());
    }
}
