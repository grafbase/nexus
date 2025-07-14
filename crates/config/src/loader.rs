use std::{path::Path, str::FromStr};

use anyhow::bail;
use serde::Deserialize;
use serde_dynamic_string::DynamicString;
use std::fmt::Write;
use toml::Value;

use crate::Config;

pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let path = path.as_ref().to_path_buf();
    let content = std::fs::read_to_string(&path)?;
    let mut raw_config: Value = toml::from_str(&content)?;

    expand_dynamic_strings(&mut Vec::new(), &mut raw_config)?;

    Ok(Config::deserialize(raw_config)?)
}

fn expand_dynamic_strings<'a>(path: &mut Vec<Result<&'a str, usize>>, value: &'a mut Value) -> anyhow::Result<()> {
    match value {
        Value::String(s) => match DynamicString::<String>::from_str(s) {
            Ok(out) => *s = out.into_inner(),
            Err(err) => {
                let mut p = String::new();

                for segment in path {
                    match segment {
                        Ok(s) => {
                            p.push_str(s);
                            p.push('.');
                        }
                        Err(i) => write!(p, "[{i}]").unwrap(),
                    }
                }

                if p.ends_with('.') {
                    p.pop();
                }

                bail!("Failed to expand dynamic string at path '{p}': {err}");
            }
        },
        Value::Array(values) => {
            for (i, value) in values.iter_mut().enumerate() {
                path.push(Err(i));
                expand_dynamic_strings(path, value)?;
                path.pop();
            }
        }
        Value::Table(map) => {
            for (key, value) in map {
                path.push(Ok(key.as_str()));
                expand_dynamic_strings(path, value)?;
                path.pop();
            }
        }
        Value::Integer(_) | Value::Float(_) | Value::Boolean(_) | Value::Datetime(_) => (),
    }

    Ok(())
}
