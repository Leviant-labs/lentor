use super::CodecStream;
use super::Item;
use super::MAX_ARGS;
use super::MessageBlock;
use super::StreamBatch;
use super::err;
use super::message_item;
use crate::CodecError;
use crate::Protocol;
use codex_api::ResponseEvent;
use respan_gateway_conversation::Block;
use respan_gateway_conversation::Delta;
use respan_gateway_conversation::Output;
use respan_gateway_conversation::block_fields;
use serde_json::Value;

impl CodecStream {
    pub(super) fn map(
        &mut self,
        output: Output,
        batch: &mut StreamBatch,
    ) -> Result<(), CodecError> {
        match &output {
            Output::Start(start) => {
                if self.response_id.is_some()
                    || start.id.is_empty()
                    || start.id.len() > 512
                    || start.native.is_some()
                {
                    return Err(err("invalid response start"));
                }
                self.response_id = Some(start.id.clone());
                batch.events.push(ResponseEvent::Created {
                    response_id: Some(start.id.clone()),
                });
                if !start.model.is_empty() {
                    batch
                        .events
                        .push(ResponseEvent::ServerModel(start.model.clone()));
                }
            }
            Output::BlockStart { block, kind } => {
                if self.response_id.is_none()
                    || self.finish.is_some()
                    || self.items.contains_key(block)
                {
                    return Err(err("invalid block start"));
                }
                let id = format!(
                    "{}_{}",
                    self.response_id.as_deref().unwrap_or_default(),
                    block
                );
                if id.len() > 512 {
                    return Err(err("canonical item identity exceeds limit"));
                }
                if let Block::ToolCall { id, name } = kind
                    && (id.is_empty()
                        || id.len() > 128
                        || name.is_empty()
                        || name.len() > 128
                        || self.calls.len() >= 64
                        || !self.calls.insert(id.clone()))
                {
                    return Err(err("invalid tool identity"));
                }
                let item = Item {
                    id,
                    kind: kind.clone(),
                    content: String::new(),
                    ended: false,
                    deferred: self.defer_output
                        || !matches!(kind, Block::Text)
                        || self.items.values().any(|item| !item.ended),
                };
                // Native state and tools cannot commit until clean EOF. Hold
                // every later item behind them so the consumer sees complete
                // item lifecycles in order, while text-only prefixes stream.
                self.defer_output = item.deferred;
                match kind {
                    Block::Text => {
                        if !item.deferred {
                            batch
                                .events
                                .push(ResponseEvent::OutputItemAdded(message_item(&item, "")?));
                        }
                    }
                    Block::Reasoning => {
                        if self.protocol == Protocol::Messages {
                            self.native
                                .entry(*block)
                                .or_default()
                                .push(&output)
                                .map_err(|_| err("invalid native reasoning"))?;
                        }
                    }
                    Block::ToolCall { .. } => {}
                    _ => return Err(err("unsupported block")),
                }
                self.items.insert(*block, item);
            }
            Output::Delta { block, delta } => {
                let item = self
                    .items
                    .get_mut(block)
                    .ok_or_else(|| err("delta without block"))?;
                if item.ended {
                    return Err(err("delta after block end"));
                }
                let (text, valid) = match (delta, &item.kind) {
                    (Delta::Text(s), Block::Text)
                    | (Delta::Reasoning(s), Block::Reasoning)
                    | (Delta::Arguments(s), Block::ToolCall { .. }) => (s.as_str(), true),
                    _ => (delta.as_str(), false),
                };
                if !valid {
                    return Err(err("delta does not match block"));
                }
                if matches!(item.kind, Block::ToolCall { .. })
                    && item.content.len().saturating_add(text.len()) > MAX_ARGS
                {
                    return Err(err("tool arguments exceed limit"));
                }
                item.content.push_str(text);
                match delta {
                    Delta::Text(s) => {
                        if !item.deferred {
                            batch.events.push(ResponseEvent::OutputTextDelta(s.clone()));
                        }
                    }
                    Delta::Reasoning(_) => {
                        if self.protocol == Protocol::Messages {
                            self.native
                                .entry(*block)
                                .or_default()
                                .push(&output)
                                .map_err(|_| err("invalid native reasoning"))?;
                        }
                    }
                    Delta::Arguments(_) => {}
                    _ => return Err(err("unsupported delta")),
                }
            }
            Output::BlockEnd { block } => {
                let item = self
                    .items
                    .get_mut(block)
                    .ok_or_else(|| err("end without block"))?;
                if item.ended {
                    return Err(err("duplicate block end"));
                }
                item.ended = true;
                if matches!(item.kind, Block::Text) && !item.deferred {
                    batch
                        .events
                        .push(ResponseEvent::OutputItemDone(message_item(
                            item,
                            &item.content,
                        )?));
                }
            }
            Output::Usage(usage) => {
                self.usage += usage;
                self.has_usage = true;
            }
            Output::Finish(finish) => {
                if self.finish.replace(finish.clone()).is_some() {
                    return Err(err("duplicate finish"));
                }
            }
            Output::Native(native) => {
                if self.protocol != Protocol::Messages
                    || native.format.as_ref() != "anthropic-messages"
                {
                    return Err(err("unsupported native output"));
                }
                let index = block_fields(&native.value)
                    .map(|(i, _)| i)
                    .or_else(|| {
                        native
                            .value
                            .get("index")
                            .and_then(Value::as_u64)
                            .and_then(|i| u32::try_from(i).ok())
                    })
                    .ok_or_else(|| err("unsupported native output"))?;
                if !matches!(
                    self.message_blocks.get(&index),
                    Some((MessageBlock::Thinking | MessageBlock::Redacted, _))
                ) {
                    return Err(err("unsupported native block fields"));
                }
                if native.value["type"] == "content_block_start"
                    && native.value["content_block"]["type"] == "redacted_thinking"
                {
                    if self.items.contains_key(&index) {
                        return Err(err("duplicate block"));
                    }
                    let id = format!(
                        "{}_{}",
                        self.response_id
                            .as_deref()
                            .ok_or_else(|| err("missing response start"))?,
                        index
                    );
                    if id.len() > 512 {
                        return Err(err("canonical item identity exceeds limit"));
                    }
                    let item = Item {
                        id,
                        kind: Block::Reasoning,
                        content: String::new(),
                        ended: false,
                        deferred: true,
                    };
                    self.defer_output = true;
                    self.items.insert(index, item);
                    self.redacted.insert(index);
                } else if native.value["type"] == "content_block_stop"
                    && self.redacted.contains(&index)
                {
                    let item = self
                        .items
                        .get_mut(&index)
                        .ok_or_else(|| err("missing redacted block"))?;
                    if item.ended {
                        return Err(err("duplicate block end"));
                    }
                    item.ended = true;
                } else if block_fields(&native.value).is_none() {
                    return Err(err("unsupported native output"));
                }
                self.native
                    .entry(index)
                    .or_default()
                    .push(&output)
                    .map_err(|_| err("invalid native reasoning"))?;
            }
            _ => return Err(err("unsupported output")),
        }
        Ok(())
    }
}
