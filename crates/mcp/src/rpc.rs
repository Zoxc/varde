//! JSON-RPC 2.0, as MCP uses it: one message per POST body, read into a
//! [`Message`], and the replies to it written as bytes.
//!
//! A body comes off the network, so it's read within [`Limits`] beside
//! the size `http` already capped it at. Nesting is counted over the raw
//! bytes, a loop with no recursion, before `serde_json` sees them, so its
//! recursive parse (and dropping the value after) stays shallow; strings,
//! arrays and objects are then checked over the parsed value, again
//! without recursion. No batches: an array is -32600, as MCP since
//! `2025-06-18` has it.

use serde::Serialize;
use serde_json::{Map, Value};

/// The bounds a body is read within.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How deep arrays and objects nest; the top level is 1.
    pub depth: usize,
    /// A string's length (keys too), in bytes once unescaped.
    pub string_bytes: usize,
    /// How many elements in an array.
    pub array: usize,
    /// How many keys in an object.
    pub keys: usize,
}

impl Limits {
    /// The default [`Limits::depth`]: far under `serde_json`'s 128, and
    /// over any tool's arguments.
    pub const DEPTH: usize = 32;
    /// The default [`Limits::string_bytes`].
    pub const STRING_BYTES: usize = 1024 * 1024;
    /// The default [`Limits::array`].
    pub const ARRAY: usize = 10_000;
    /// The default [`Limits::keys`].
    pub const KEYS: usize = 1_000;
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: Self::DEPTH,
            string_bytes: Self::STRING_BYTES,
            array: Self::ARRAY,
            keys: Self::KEYS,
        }
    }
}

/// A request's id: an integer, or a string no longer than [`Id::STRING`].
/// Not a fraction, and not `null`, which only an error reply carries.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum Id {
    Number(i64),
    String(String),
}

impl Id {
    /// The longest string id, in bytes.
    pub const STRING: usize = 256;

    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().map(Id::Number),
            Value::String(s) if s.len() <= Self::STRING => Some(Id::String(s.clone())),
            _ => None,
        }
    }
}

/// A message read from a body.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Wants a reply.
    Request(Request),
    /// Wants none.
    Notification(Notification),
    /// A client's reply to a request of the server's. The server sends
    /// none, so it's ignored (202).
    Response,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: Id,
    pub method: String,
    /// An object or an array, if sent.
    pub params: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub method: String,
    /// An object or an array, if sent.
    pub params: Option<Value>,
}

/// The error codes the server replies with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    /// -32700: not JSON.
    Parse,
    /// -32600: JSON, but not a request, or past [`Limits`].
    InvalidRequest,
    /// -32601: no such method.
    MethodNotFound,
    /// -32602: the method's params are wrong.
    InvalidParams,
    /// -32603: the server failed.
    Internal,
    /// -32000: the server couldn't be reached or refused the message;
    /// only the stdio relay sends it.
    Unreachable,
}

impl Code {
    /// The number sent.
    pub const fn number(self) -> i64 {
        match self {
            Code::Parse => -32700,
            Code::InvalidRequest => -32600,
            Code::MethodNotFound => -32601,
            Code::InvalidParams => -32602,
            Code::Internal => -32603,
            Code::Unreachable => -32000,
        }
    }
}

/// Why a body isn't a message: the code and words to reply with, and the
/// id to reply to, if one could be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: Code,
    pub message: &'static str,
    pub id: Option<Id>,
}

impl Error {
    fn new(code: Code, message: &'static str) -> Self {
        Self {
            code,
            message,
            id: None,
        }
    }

    /// The error reply, serialized.
    pub fn reply(&self) -> Vec<u8> {
        error(self.id.as_ref(), self.code, self.message)
    }
}

/// Reads `body` as one message, within `limits`.
pub fn parse(body: &[u8], limits: &Limits) -> Result<Message, Error> {
    let invalid = |message| Error::new(Code::InvalidRequest, message);
    check_depth(body, limits.depth)?;
    let value: Value =
        serde_json::from_slice(body).map_err(|_| Error::new(Code::Parse, "Parse error"))?;
    check_sizes(&value, limits)?;
    let object = match value {
        Value::Object(object) => object,
        Value::Array(_) => return Err(invalid("Batches aren't supported")),
        _ => return Err(invalid("Not a JSON-RPC message")),
    };
    // Read early, so the errors after can be replied to it.
    let id = match object.get("id") {
        None => None,
        Some(value) => match Id::from_value(value) {
            Some(id) => Some(id),
            None if value.is_null() => None,
            None => return Err(invalid("Bad id")),
        },
    };
    let with_id = |message| Error {
        code: Code::InvalidRequest,
        message,
        id: id.clone(),
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(with_id("jsonrpc must be \"2.0\""));
    }
    if !object.contains_key("method") {
        return response(&object).map_err(with_id);
    }
    let Some(Value::String(method)) = object.get("method") else {
        return Err(with_id("method must be a string"));
    };
    let params = match object.get("params") {
        None => None,
        Some(p @ (Value::Object(_) | Value::Array(_))) => Some(p.clone()),
        Some(_) => return Err(with_id("params must be an object or an array")),
    };
    if object.contains_key("result") || object.contains_key("error") {
        return Err(with_id("A request with a result or an error"));
    }
    let method = method.clone();
    match (object.get("id"), id) {
        (None, _) => Ok(Message::Notification(Notification { method, params })),
        (Some(_), Some(id)) => Ok(Message::Request(Request { id, method, params })),
        (Some(_), None) => Err(invalid("A request's id can't be null")),
    }
}

/// A client's reply: an id (`null` allowed, for an error), and a result
/// or an error, not both.
fn response(object: &Map<String, Value>) -> Result<Message, &'static str> {
    if !object.contains_key("id") {
        return Err("Neither a request nor a response");
    }
    match (object.get("result"), object.get("error")) {
        (Some(_), None) => Ok(Message::Response),
        (None, Some(Value::Object(_))) => Ok(Message::Response),
        _ => Err("Neither a request nor a response"),
    }
}

/// Refuses JSON nesting deeper than `limit`, counted over the bytes,
/// skipping strings. Bytes that aren't JSON are left to the parser.
fn check_depth(body: &[u8], limit: usize) -> Result<(), Error> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in body {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth = depth.saturating_add(1);
                if depth > limit {
                    return Err(Error::new(Code::InvalidRequest, "Nested too deep"));
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Refuses strings, arrays and objects over `limits`, walking the value
/// with a stack of its own.
fn check_sizes(value: &Value, limits: &Limits) -> Result<(), Error> {
    let too_large = || Error::new(Code::InvalidRequest, "Too large");
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::String(s) if s.len() > limits.string_bytes => return Err(too_large()),
            Value::Array(a) => {
                if a.len() > limits.array {
                    return Err(too_large());
                }
                stack.extend(a);
            }
            Value::Object(o) => {
                if o.len() > limits.keys || o.keys().any(|k| k.len() > limits.string_bytes) {
                    return Err(too_large());
                }
                stack.extend(o.values());
            }
            _ => {}
        }
    }
    Ok(())
}

/// A success reply to `id`, serialized.
pub fn success(id: &Id, result: Value) -> Vec<u8> {
    #[derive(Serialize)]
    struct Reply<'a> {
        jsonrpc: &'static str,
        id: &'a Id,
        result: Value,
    }
    let reply = Reply {
        jsonrpc: "2.0",
        id,
        result,
    };
    // A `Value` and plain fields: serializing can't fail.
    serde_json::to_vec(&reply).expect("a reply serializes")
}

/// An error reply to `id`, or to `null` if it couldn't be read, serialized.
pub fn error(id: Option<&Id>, code: Code, message: &str) -> Vec<u8> {
    #[derive(Serialize)]
    struct Reply<'a> {
        jsonrpc: &'static str,
        id: Option<&'a Id>,
        error: Body<'a>,
    }
    #[derive(Serialize)]
    struct Body<'a> {
        code: i64,
        message: &'a str,
    }
    let reply = Reply {
        jsonrpc: "2.0",
        id,
        error: Body {
            code: code.number(),
            message,
        },
    };
    serde_json::to_vec(&reply).expect("a reply serializes")
}

#[cfg(test)]
mod tests;
