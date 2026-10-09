//! Bounded, strict configuration independent of Unix host services.
use anyhow::{bail, Result};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};

pub fn read_bounded(path: &Path, bound: usize) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    File::open(path)?
        .take((bound + 1) as u64)
        .read_to_end(&mut raw)?;
    if raw.len() > bound {
        bail!("File exceeds capacity");
    }
    Ok(raw)
}
pub fn read_config<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_value(strict_json(&read_bounded(path, crate::MAX_CONFIG)?)?)
        .map_err(|_| anyhow::anyhow!("Invalid configuration fields"))
}
pub fn validate_identity(value: &str) -> Result<()> {
    if value.len() > 32
        || !value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        bail!("Invalid stable device identity");
    }
    Ok(())
}
pub fn validate_platform(value: &str) -> Result<()> {
    if !matches!(value, "linux" | "macos" | "windows" | "router" | "other") {
        bail!("Invalid device platform");
    }
    Ok(())
}
pub fn validate_text(value: &str, cap: usize) -> Result<()> {
    if value.chars().count() > cap || value.chars().any(char::is_control) {
        bail!("Invalid metadata text");
    }
    Ok(())
}
pub fn read_token(path: &Path, minimum: usize) -> Result<String> {
    let token = String::from_utf8(read_bounded(path, 512)?)
        .map_err(|_| anyhow::anyhow!("Invalid credential file"))?;
    let token = token.trim();
    if token.len() < minimum || token.len() > 256 || !token.bytes().all(|c| (33..=126).contains(&c))
    {
        bail!("Invalid credential format");
    }
    Ok(token.into())
}
pub fn strict_json(raw: &[u8]) -> Result<Value> {
    struct Strict(Value);
    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Strict;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("strict JSON")
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                    serde_json::Number::from_f64(v)
                        .map(|n| Strict(Value::Number(n)))
                        .ok_or_else(|| E::custom("nonfinite JSON"))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_string<E: serde::de::Error>(
                    self,
                    v: String,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Null))
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut a: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut v = Vec::new();
                    while let Some(x) = a.next_element::<Strict>()? {
                        v.push(x.0);
                    }
                    Ok(Strict(v.into()))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut a: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut v = serde_json::Map::new();
                    while let Some(k) = a.next_key::<String>()? {
                        if v.contains_key(&k) {
                            return Err(serde::de::Error::custom("duplicate configuration field"));
                        }
                        v.insert(k, a.next_value::<Strict>()?.0);
                    }
                    Ok(Strict(Value::Object(v)))
                }
            }
            de.deserialize_any(Visitor)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = Strict::deserialize(&mut deserializer)
        .map_err(|_| anyhow::anyhow!("Invalid or duplicate JSON fields"))?
        .0;
    deserializer
        .end()
        .map_err(|_| anyhow::anyhow!("Trailing JSON data"))?;
    Ok(value)
}
