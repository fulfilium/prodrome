use crate::event::{field, lower_hex};
use crate::literal::{Call, ProdromeError, Value};
use crate::payload::string_field;

/// `Genesis(label, nonce)`: the root of one prodrome, whose name is its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genesis {
    pub label: String,
    pub nonce: Nonce,
}

crate::newtype_str! {
    /// 32 lowercase hex digits, drawn once, so that two prodromes with the
    /// same label have different names.
    Nonce
}

impl Nonce {
    pub fn new(text: impl Into<String>) -> Result<Nonce, ProdromeError> {
        let text = text.into();
        if lower_hex(&text, 32) {
            Ok(Nonce(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "Genesis.nonce must be 32 lowercase hex, got {text:?}"
            )))
        }
    }
}

pub fn mk_genesis(label: &str, nonce: &str) -> Result<Genesis, ProdromeError> {
    Ok(Genesis {
        label: label.to_owned(),
        nonce: Nonce::new(nonce)?,
    })
}

impl Genesis {
    pub fn to_value(&self) -> Value {
        Value::call(
            "Genesis",
            vec![
                field("label", Value::str(self.label.as_str())),
                field("nonce", Value::str(self.nonce.as_str())),
            ],
        )
    }

    pub fn from_call(call: &Call) -> Result<Genesis, ProdromeError> {
        mk_genesis(&string_field(call, "label")?, &string_field(call, "nonce")?)
    }
}
