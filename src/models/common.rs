use serde::{Deserialize, Deserializer, Serialize};

pub fn deserialize_string_or_number<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(value.map(|v| match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    #[serde(default)]
    pub frame: u32,
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub module: Option<String>,
    pub offset: Option<String>,
}
