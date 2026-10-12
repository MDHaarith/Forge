use serde_json::Value;

/// Checks a tool input against the subset of JSON Schema Forge tools use.
/// Tool inputs are streamed eagerly, so the API does not validate them: a truncated
/// or malformed input must be caught here before the tool runs.
/// Returns a list of problems; empty means valid.
pub fn validate(value: &Value, schema: &Value) -> Vec<String> {
    validate_at(value, schema, "input")
}

fn validate_at(value: &Value, schema: &Value, at: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            problems.push(format!(
                "{at} must be one of {}",
                choices
                    .iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    match schema["type"].as_str().unwrap_or("") {
        "object" => {
            let Some(record) = value.as_object() else {
                return vec![format!("{at} must be an object")];
            };
            if let Some(required) = schema["required"].as_array() {
                for key in required.iter().filter_map(Value::as_str) {
                    if !record.contains_key(key) {
                        problems.push(format!("{at}.{key} is required"));
                    }
                }
            }
            if let Some(properties) = schema["properties"].as_object() {
                for (key, sub) in properties {
                    if let Some(value) = record.get(key) {
                        problems.extend(validate_at(value, sub, &format!("{at}.{key}")));
                    }
                }
            }
        }
        "array" => {
            let Some(values) = value.as_array() else {
                return vec![format!("{at} must be an array")];
            };
            if let Some(min) = schema["minItems"].as_u64() {
                if values.len() < min as usize {
                    problems.push(format!("{at} needs at least {min} items"));
                }
            }
            if let Some(max) = schema["maxItems"].as_u64() {
                if values.len() > max as usize {
                    problems.push(format!("{at} allows at most {max} items"));
                }
            }
            if let Some(items) = schema.get("items") {
                for (i, value) in values.iter().enumerate() {
                    problems.extend(validate_at(value, items, &format!("{at}[{i}]")));
                }
            }
        }
        "string" if !value.is_string() => problems.push(format!("{at} must be a string")),
        "number" if !value.as_f64().is_some_and(f64::is_finite) => {
            problems.push(format!("{at} must be a number"))
        }
        "integer"
            if !value
                .as_f64()
                .is_some_and(|n| n.is_finite() && n.fract() == 0.0) =>
        {
            problems.push(format!("{at} must be an integer"))
        }
        "boolean" if !value.is_boolean() => problems.push(format!("{at} must be a boolean")),
        _ => {}
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reports_missing_types_and_enums_in_property_order() {
        let schema = json!({"type":"object", "properties":{
            "name":{"type":"string"}, "at":{"type":"array", "items":{"type":"number"}, "minItems":3,"maxItems":3},
            "shape":{"type":"string", "enum":["box"]}}, "required":["name"]});
        assert!(validate(&json!({"name":"x","at":[1,2,3],"shape":"box"}), &schema).is_empty());
        assert_eq!(validate(&json!({}), &schema), ["input.name is required"]);
        assert_eq!(
            validate(&json!({"name":1,"at":[1,"2"],"shape":"cone"}), &schema),
            [
                "input.name must be a string",
                "input.at needs at least 3 items",
                "input.at[1] must be a number",
                "input.shape must be one of \"box\""
            ]
        );
        assert_eq!(
            validate(&json!("nope"), &schema),
            ["input must be an object"]
        );
        assert!(validate(&json!(5.0), &json!({"type":"integer"})).is_empty());
        assert_eq!(
            validate(&json!([1, 2]), &json!({"type":"array","maxItems":1})),
            ["input allows at most 1 items"]
        );
    }
}
