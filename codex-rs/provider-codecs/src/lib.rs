//! Provider codec compatibility components.

mod chat_history;
mod chat_normalizer;
mod metadata;
mod native;
mod requests;
mod tools;

pub use chat_normalizer::ChatNormalizer;
pub use chat_normalizer::NormalizeError;
pub use native::AnthropicNativeState;
pub use requests::CodecError;
pub use requests::EncodedRequest;
pub use requests::NativeRecord;
pub use requests::Protocol;
pub use requests::encode_request;
pub use tools::RestoredToolCall;
pub use tools::ToolIdentity;

mod stream;
pub use stream::CodecStream;
pub use stream::StreamBatch;
