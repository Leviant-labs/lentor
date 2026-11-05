use crate::CodecError;
use respan_gateway_conversation::Tool;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

// Count UTF-8 bytes conservatively: even byte-level tokenization cannot exceed
// this per-item limit. Include wrapper/namespace text in the final wire check.
pub(crate) const MAX_TOOL_BYTES: usize = 8_000;
pub(crate) const MAX_TOOLS_BYTES: usize = 256_000;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FreeformInput {
    input: String,
}

/// The canonical identity and calling convention of a translated tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolIdentity {
    pub name: String,
    pub namespace: Option<String>,
    pub freeform: bool,
}

/// A provider call restored to its local identity and input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoredToolCall {
    pub name: String,
    pub namespace: Option<String>,
    pub freeform: bool,
    pub input: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ToolRegistry {
    entries: BTreeMap<String, ToolIdentity>,
}

impl ToolRegistry {
    pub(crate) fn history_wire_name(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> Result<String, CodecError> {
        if let Some(wire) = self.entries.iter().find_map(|(wire, identity)| {
            (identity.name == name && same_namespace(identity.namespace.as_deref(), namespace))
                .then(|| wire.clone())
        }) {
            return Ok(wire);
        }
        // Closed calls can outlive the active tools, notably in compaction.
        // This only encodes retained history; restore() still requires an
        // active declaration before releasing any newly generated call.
        let wire = encoded_tool_name(name, namespace)?;
        if self.entries.contains_key(&wire) {
            return Err(CodecError::new(
                "historical tool name conflicts with a current tool",
            ));
        }
        Ok(wire)
    }

    pub(crate) fn restore(
        &self,
        wire: &str,
        arguments: &str,
    ) -> Result<RestoredToolCall, CodecError> {
        let identity = self
            .entries
            .get(wire)
            .ok_or_else(|| CodecError::new("unknown provider tool name"))?;
        let input = if identity.freeform {
            let value: FreeformInput = serde_json::from_str(arguments)
                .map_err(|_| CodecError::new("invalid freeform wrapper JSON"))?;
            value.input
        } else {
            let value: Value = serde_json::from_str(arguments)
                .map_err(|_| CodecError::new("invalid function arguments"))?;
            if !value.is_object() {
                return Err(CodecError::new("function arguments must be an object"));
            }
            arguments.to_owned()
        };
        Ok(RestoredToolCall {
            name: identity.name.clone(),
            namespace: identity.namespace.clone(),
            freeform: identity.freeform,
            input,
        })
    }

    pub(crate) fn add(
        &mut self,
        definitions: &[Value],
        out: &mut Vec<Tool>,
        namespace: Option<&str>,
    ) -> Result<(), CodecError> {
        for definition in definitions {
            let obj = definition
                .as_object()
                .ok_or_else(|| CodecError::new("invalid tool declaration"))?;
            let kind = obj
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| CodecError::new("tool type missing"))?;
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CodecError::new("tool name missing"))?;
            if kind == "namespace" {
                if namespace.is_some()
                    || obj
                        .keys()
                        .any(|k| !matches!(k.as_str(), "type" | "name" | "description" | "tools"))
                {
                    return Err(CodecError::new("unsupported namespace declaration"));
                }
                let children = obj
                    .get("tools")
                    .and_then(Value::as_array)
                    .ok_or_else(|| CodecError::new("namespace tools missing"))?;
                let description = obj
                    .get("description")
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| CodecError::new("invalid namespace description"))
                    })
                    .transpose()?
                    .unwrap_or("");
                if description.len() > MAX_TOOL_BYTES {
                    return Err(CodecError::new("namespace description exceeds tool limit"));
                }
                let mut children = children.clone();
                if !description.is_empty() {
                    for child in &mut children {
                        let child = child
                            .as_object_mut()
                            .ok_or_else(|| CodecError::new("invalid namespace tool"))?;
                        let child_description = child
                            .get("description")
                            .map(|value| {
                                value
                                    .as_str()
                                    .ok_or_else(|| CodecError::new("invalid tool description"))
                            })
                            .transpose()?
                            .unwrap_or("");
                        child.insert(
                            "description".into(),
                            format!("{description}\n{child_description}").into(),
                        );
                    }
                }
                self.add(&children, out, Some(name))?;
                continue;
            }
            if serde_json::to_vec(definition)
                .map_err(|_| CodecError::new("invalid tool declaration"))?
                .len()
                > MAX_TOOL_BYTES + 1_024
            {
                return Err(CodecError::new("tool declaration exceeds limit"));
            }
            let freeform = match kind {
                "function" => false,
                "custom" => true,
                _ => return Err(CodecError::new(format!("unsupported tool type {kind}"))),
            };
            if obj.get("description").is_some_and(|v| !v.is_string())
                || obj.get("strict").is_some_and(|v| !v.is_boolean())
                || obj.get("defer_loading").is_some_and(|v| !v.is_boolean())
                || (freeform && (obj.contains_key("parameters") || obj.contains_key("strict")))
                || (!freeform && obj.contains_key("format"))
            {
                return Err(CodecError::new("invalid or unsupported tool field"));
            }
            if obj.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "type"
                        | "name"
                        | "description"
                        | "parameters"
                        | "strict"
                        | "format"
                        | "defer_loading"
                )
            }) || obj.get("defer_loading").and_then(Value::as_bool) == Some(true)
            {
                return Err(CodecError::new("unsupported tool declaration field"));
            }
            if obj.get("strict").and_then(Value::as_bool) == Some(true) {
                return Err(CodecError::new(
                    "strict function tools cannot be preserved across protocols",
                ));
            }
            let wire = encoded_tool_name(name, namespace)?;
            let identity = ToolIdentity {
                name: name.to_owned(),
                namespace: namespace.map(str::to_owned),
                freeform,
            };
            if self.entries.values().any(|existing| {
                existing.name == name && same_namespace(existing.namespace.as_deref(), namespace)
            }) {
                return Err(CodecError::new("duplicate canonical tool identity"));
            }
            if self.entries.insert(wire.clone(), identity).is_some() {
                return Err(CodecError::new("duplicate translated tool name"));
            }
            let params = if freeform {
                json!({"type":"object","properties":{"input":{"type":"string","description":"The exact freeform tool input."}},"required":["input"],"additionalProperties":false})
            } else {
                obj.get("parameters")
                    .filter(|v| v.is_object())
                    .cloned()
                    .ok_or_else(|| CodecError::new("function parameters must be a schema object"))?
            };
            let mut tool = Tool::new(wire, params);
            let description = obj.get("description").and_then(Value::as_str).unwrap_or("");
            tool.description = Some(if freeform {
                let format = obj
                    .get("format")
                    .ok_or_else(|| CodecError::new("freeform tool format missing"))?;
                let fields = format
                    .as_object()
                    .ok_or_else(|| CodecError::new("invalid freeform format"))?;
                let valid = match fields.get("type").and_then(Value::as_str) {
                    Some("text") => fields.len() == 1,
                    Some("grammar") => {
                        fields.len() == 3
                            && matches!(
                                fields.get("syntax").and_then(Value::as_str),
                                Some("lark" | "regex")
                            )
                            && fields
                                .get("definition")
                                .and_then(Value::as_str)
                                .is_some_and(|text| !text.trim().is_empty())
                    }
                    _ => false,
                };
                if !valid {
                    return Err(CodecError::new("unsupported freeform format"));
                }
                // The JSON envelope replaces provider-side grammar-constrained
                // generation. Local tool parsing/approvals remain authoritative;
                // never claim this description enforces arbitrary Lark grammars.
                format!(
                    "Call this tool with a JSON object whose input field contains the exact freeform text. The local tool validates the decoded input. Original tool description (applies to the input string): {description}\nInput format guidance (not constrained generation on this endpoint): {format}"
                )
            } else {
                description.to_owned()
            });
            out.push(tool);
        }
        Ok(())
    }
}

fn encoded_tool_name(name: &str, namespace: Option<&str>) -> Result<String, CodecError> {
    let wire = match namespace.filter(|namespace| !namespace.is_empty()) {
        Some(namespace) => format!("{namespace}__{name}"),
        None => name.to_owned(),
    };
    if name.is_empty()
        || wire.len() > 64
        || !wire
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(CodecError::new(
            "tool name cannot be represented by provider",
        ));
    }
    Ok(wire)
}

fn same_namespace(left: Option<&str>, right: Option<&str>) -> bool {
    let default = |value: Option<&str>| {
        matches!(
            value,
            None | Some("") | Some(codex_protocol::DEFAULT_FUNCTION_NAMESPACE)
        )
    };
    left == right || (default(left) && default(right))
}
