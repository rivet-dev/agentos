//! RFC 7386 merge-patch decoding for typed action contracts.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Apply an RFC 7386 object patch and decode the resulting typed input.
///
/// Null removals are probed against the typed input so removing an unknown
/// field cannot bypass `deny_unknown_fields` merely because the field was
/// absent from the current document.
pub fn merge_typed<TCurrent, TInput>(current: &TCurrent, patch: Value) -> Result<TInput>
where
    TCurrent: Serialize,
    TInput: DeserializeOwned,
{
    if !patch.is_object() {
        anyhow::bail!("invalid_input: config.patch patch must be a JSON object");
    }
    let mut removals = Vec::new();
    collect_removals(&patch, &mut Vec::new(), &mut removals);
    let mut document = serde_json::to_value(current).context("serialize current desired config")?;
    apply(&mut document, patch);
    let input = serde_json::from_value(document.clone())
        .context("invalid_input: decode merged config input")?;

    for path in removals {
        parent_mut(&mut document, &path).insert(
            path.last().expect("removal has a field").clone(),
            Value::Null,
        );
        let probe = serde_json::from_value::<TInput>(document.clone());
        parent_mut(&mut document, &path).remove(path.last().expect("removal has a field"));
        if let Err(error) = probe {
            let message = error.to_string();
            if message.starts_with("unknown field ")
                || message.contains("did not match any variant of untagged enum")
            {
                return Err(error).context("invalid_input: validate config patch removal");
            }
        }
    }
    Ok(input)
}

fn collect_removals(value: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if let Value::Object(object) = value {
        for (key, value) in object {
            path.push(key.clone());
            if value.is_null() {
                out.push(path.clone());
            } else {
                collect_removals(value, path, out);
            }
            path.pop();
        }
    }
}

fn parent_mut<'a>(
    document: &'a mut Value,
    path: &[String],
) -> &'a mut serde_json::Map<String, Value> {
    let mut parent = document;
    for key in &path[..path.len() - 1] {
        parent = parent.get_mut(key).expect("merge patch created the parent");
    }
    parent
        .as_object_mut()
        .expect("merge patch parent is an object")
}

fn apply(target: &mut Value, patch: Value) {
    let Value::Object(patch) = patch else {
        *target = patch;
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Default::default());
    }
    let target = target
        .as_object_mut()
        .expect("target was replaced with a JSON object");
    for (key, value) in patch {
        if value.is_null() {
            target.remove(&key);
        } else {
            apply(target.entry(key).or_insert(Value::Null), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Serialize)]
    struct Current {
        nested: Nested,
    }

    #[derive(Debug, PartialEq, Serialize)]
    struct Nested {
        keep: String,
        remove: String,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        nested: InputNested,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct InputNested {
        keep: String,
        #[serde(default)]
        remove: Option<String>,
    }

    #[test]
    fn applies_nested_removals_and_rejects_unknown_removals() {
        let current = Current {
            nested: Nested {
                keep: "before".into(),
                remove: "value".into(),
            },
        };
        let merged: Input = merge_typed(
            &current,
            serde_json::json!({"nested": {"keep": "after", "remove": null}}),
        )
        .unwrap();
        assert_eq!(merged.nested.keep, "after");
        assert_eq!(merged.nested.remove, None);

        assert!(merge_typed::<_, Input>(
            &current,
            serde_json::json!({"nested": {"unknown": null}}),
        )
        .unwrap_err()
        .to_string()
        .starts_with("invalid_input:"));
    }
}
