use serde_json::Value;

use crate::{CompileError, Compiler, Draft, OutputError, OutputUnit, SchemaIndex, Schemas};

pub struct Validator {
    schemas: Schemas,
    index: SchemaIndex,
}

pub struct ValidationIssue {
    instance_path: String,
    schema_path: String,
}

impl ValidationIssue {
    pub fn instance_path(&self) -> &str {
        &self.instance_path
    }

    pub fn schema_path(&self) -> &str {
        &self.schema_path
    }
}

pub fn validator_for(schema: &Value) -> Result<Validator, CompileError> {
    let mut compiler = Compiler::new();
    compiler.set_default_draft(Draft::V2020_12);
    let location = "json-schema:///";
    compiler.add_resource(location, schema.clone())?;
    let mut schemas = Schemas::new();
    let index = compiler.compile(location, &mut schemas)?;
    Ok(Validator { schemas, index })
}

impl Validator {
    pub fn is_valid(&self, value: &Value) -> bool {
        self.schemas.is_valid(value, self.index)
    }

    pub fn iter_errors(&self, value: &Value) -> std::vec::IntoIter<ValidationIssue> {
        let mut issues = Vec::new();
        if let Err(error) = self.schemas.validate(value, self.index) {
            collect_issues(error.basic_output(), &mut issues);
        }
        issues.into_iter()
    }
}

fn collect_issues(output: OutputUnit<'_, '_, '_>, issues: &mut Vec<ValidationIssue>) {
    match output.error {
        OutputError::Leaf(_) => issues.push(ValidationIssue {
            instance_path: output.instance_location.to_string(),
            schema_path: output.keyword_location,
        }),
        OutputError::Branch(children) => {
            for child in children {
                collect_issues(child, issues);
            }
        }
    }
}
