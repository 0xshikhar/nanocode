use std::{error::Error, fs, path::PathBuf};

use nanocode_schema::{Compiler, Draft, Schemas, UrlLoader};
use serde_json::Value;

struct Resources(PathBuf);

impl UrlLoader for Resources {
    fn load(&self, url: &str) -> Result<Value, Box<dyn Error>> {
        let path = url
            .strip_prefix("http://localhost:1234/")
            .ok_or("unsupported test resource")?;
        Ok(serde_json::from_slice(&fs::read(self.0.join(path))?)?)
    }
}

#[test]
#[ignore = "set JSON_SCHEMA_TEST_SUITE to a checkout of the official suite"]
fn required_json_schema_suite() {
    let root =
        PathBuf::from(std::env::var_os("JSON_SCHEMA_TEST_SUITE").expect("JSON_SCHEMA_TEST_SUITE"));
    let mut cases = 0;
    for (directory, draft) in [
        ("draft4", Draft::V4),
        ("draft6", Draft::V6),
        ("draft7", Draft::V7),
        ("draft2019-09", Draft::V2019_09),
        ("draft2020-12", Draft::V2020_12),
    ] {
        for entry in fs::read_dir(root.join("tests").join(directory)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|s| s != "json") {
                continue;
            }
            let groups: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            for group in groups.as_array().unwrap() {
                let mut compiler = Compiler::new();
                compiler.set_default_draft(draft);
                compiler.use_loader(Box::new(Resources(root.join("remotes"))));
                compiler
                    .add_resource("http://localhost:1234/root-schema", group["schema"].clone())
                    .unwrap();
                let mut schemas = Schemas::new();
                let index = compiler
                    .compile("http://localhost:1234/root-schema", &mut schemas)
                    .unwrap();
                for test in group["tests"].as_array().unwrap() {
                    assert_eq!(
                        schemas.validate(&test["data"], index).is_ok(),
                        test["valid"].as_bool().unwrap(),
                        "{}: {}: {}",
                        path.display(),
                        group["description"],
                        test["description"]
                    );
                    assert_eq!(
                        schemas.is_valid(&test["data"], index),
                        test["valid"].as_bool().unwrap(),
                        "boolean validation: {}: {}",
                        path.display(),
                        test["description"]
                    );
                    cases += 1;
                }
            }
        }
    }
    println!("validated {cases} official suite cases");
}
