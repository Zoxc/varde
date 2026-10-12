//! [`ToolReply`]: a tool's answer.

use super::*;

/// A tool's answer: text, and for data the same JSON as structured
/// content, or an error the model reads and recovers from.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolReply {
    pub text: String,
    /// Sent as `structuredContent`, its JSON also the text.
    pub structured: Option<Value>,
    pub is_error: bool,
    /// A PNG, sent as image content after the text, base64: bounded by
    /// [`ToolReply::IMAGE_BYTES`], apart from the text's cap.
    pub png: Option<Vec<u8>>,
}

impl ToolReply {
    /// The default [`ToolReply::result`] cap, in bytes of text: well
    /// under the 10k tokens Claude Code warns at, counting the
    /// structured copy too.
    pub const CAP: usize = 16 * 1024;

    /// The largest PNG a reply carries, in bytes: 2 MiB, under 3 MB as
    /// base64, well within what clients take of an image (Claude's 5 MB).
    pub const IMAGE_BYTES: usize = 2 * 1024 * 1024;

    /// A plain text answer.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: false,
            png: None,
        }
    }

    /// A refusal or failure, text alone.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: true,
            png: None,
        }
    }

    /// Data: `value` as structured content and as its JSON text.
    pub fn data(value: Value) -> Self {
        Self {
            // A `Value`: serializing can't fail.
            text: serde_json::to_string(&value).expect("a value serializes"),
            structured: Some(value),
            is_error: false,
            png: None,
        }
    }

    /// The same, with the PNG `png` as image content after the text.
    pub fn with_png(self, png: Vec<u8>) -> Self {
        Self {
            png: Some(png),
            ..self
        }
    }

    /// The same with `fields` added: into its structured content (and so
    /// its text, the PNG kept) when it's data, else as `key: value` lines
    /// after its text, strings bare. Nothing to add leaves it as it is.
    pub fn with_fields(self, fields: Vec<(&str, Value)>) -> Self {
        if fields.is_empty() {
            return self;
        }
        match self.structured {
            Some(Value::Object(mut data)) if !self.is_error => {
                for (key, value) in fields {
                    data.insert(key.to_owned(), value);
                }
                Self {
                    png: self.png,
                    ..Self::data(Value::Object(data))
                }
            }
            _ => {
                let mut text = self.text;
                text.push('\n');
                for (key, value) in fields {
                    let value = match value {
                        Value::String(value) => value,
                        value => value.to_string(),
                    };
                    text.push_str(&format!("\n{key}: {value}"));
                }
                Self { text, ..self }
            }
        }
    }

    /// The `tools/call` result, the text cut to `cap` bytes with a note
    /// saying so. Cut data can't be structured content any more, nor meet
    /// the tool's `outputSchema`, so it goes as text alone, with `isError`
    /// (clients check a result without it against the schema).
    pub fn result(&self, cap: usize) -> Value {
        let mut result = Map::new();
        let mut is_error = self.is_error;
        if self.text.len() <= cap {
            if let Some(structured) = &self.structured {
                result.insert("structuredContent".into(), structured.clone());
            }
            result.insert("content".into(), text_content(&self.text));
        } else {
            let kept = crate::cut(&self.text, cap);
            let note = format!(
                "\n[Cut: this reply was {} bytes, past the cap of {cap}. Ask for less, \
                 such as with filters or offset and limit.]",
                self.text.len()
            );
            let mut text = String::with_capacity(kept.len().saturating_add(note.len()));
            text.push_str(kept);
            text.push_str(&note);
            result.insert("content".into(), text_content(&text));
            is_error |= self.structured.is_some();
        }
        if let Some(png) = &self.png
            && let Some(Value::Array(content)) = result.get_mut("content")
        {
            if png.len() <= Self::IMAGE_BYTES {
                use base64::Engine;
                content.push(json!({
                    "type": "image",
                    "data": base64::engine::general_purpose::STANDARD.encode(png),
                    "mimeType": "image/png",
                }));
            } else {
                // The app keeps under the bound: a backstop.
                content.push(json!({
                    "type": "text",
                    "text": format!(
                        "[The image was {} bytes, past the bound of {}: ask for a smaller size.]",
                        png.len(),
                        Self::IMAGE_BYTES
                    ),
                }));
                is_error = true;
            }
        }
        result.insert("isError".into(), Value::Bool(is_error));
        Value::Object(result)
    }
}

fn text_content(text: &str) -> Value {
    json!([{ "type": "text", "text": text }])
}
