use anyhow::Result;
use serde::Serialize;

use crate::config::Config;

pub fn show(config: &Config) -> Result<()> {
    let mut value = toml::Value::try_from(config)?;
    mask_sensitive(&mut value, None);

    println!("Config path: {}", Config::config_path()?.display());
    println!();
    println!("{}", toml::to_string_pretty(&value)?);
    Ok(())
}

fn mask_sensitive(value: &mut toml::Value, key: Option<&str>) {
    match value {
        toml::Value::Table(table) => {
            for (child_key, child_value) in table.iter_mut() {
                mask_sensitive(child_value, Some(child_key));
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                mask_sensitive(item, key);
            }
        }
        toml::Value::String(text) if key.is_some_and(is_sensitive_key) && !text.is_empty() => {
            *text = "***".to_string();
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("password")
        || key.contains("secret")
        || key.contains("api_key")
}

#[allow(dead_code)]
fn _assert_serializable<T: Serialize>(_value: &T) {}
