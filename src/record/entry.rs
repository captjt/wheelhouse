use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One entry: what an actor said, or what the hub saw.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub v: u32,
    pub docket: String,
    pub kind: String,
    pub at: String,
    pub actor: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blobs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Actor {
    #[serde(rename = "type")]
    pub typ: String,
    pub id: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Actor {
    pub fn new(typ: &str, id: &str) -> Self {
        Actor { typ: typ.to_string(), id: id.to_string(), extra: Map::new() }
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        let v = value.into();
        if !v.is_null() {
            self.extra.insert(key.to_string(), v);
        }
        self
    }
}

impl Entry {
    pub fn new(docket: &str, kind: &str, at: &str, actor: Actor) -> Self {
        Entry {
            v: 1,
            docket: docket.to_string(),
            kind: kind.to_string(),
            at: at.to_string(),
            actor,
            body: None,
            data: Value::Null,
            blobs: Vec::new(),
            replaces: None,
        }
    }

    #[cfg(test)]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    #[cfg(test)]
    pub fn data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    /// The entry's bytes, as its writer signs them. Made once per entry.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an entry always serializes")
    }
}
