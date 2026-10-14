use serde::Serialize;

use super::JsonSchema;

#[derive(Clone, Debug, Serialize)]
pub struct StrictJsonSchema {
    #[serde(rename = "type")]
    kind: &'static str,
    strict: bool,
    name: Box<str>,
    schema: JsonSchema,
}

impl StrictJsonSchema {
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, schema: impl Into<JsonSchema>) -> Self {
        Self {
            kind: "json_schema",
            strict: true,
            name: name.into(),
            schema: schema.into(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn schema(&self) -> &JsonSchema {
        &self.schema
    }
}
