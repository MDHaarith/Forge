mod common;
use common::TempDir;
use forge_server::{
    tools::{all_tools, available_tools, describe_tools, validate::validate, ToolContext},
    Settings,
};
use serde_json::{json, Value};

#[tokio::test]
async fn workspace_checks_paths_read_limits_and_byte_counts() {
    let dir = TempDir::new("workspace");
    let ctx = ToolContext::new(dir.path.clone(), dir.path.clone());
    let tools = all_tools();
    let tool = |name| tools.iter().find(|t| t.name == name).unwrap();
    assert_eq!(
        (tool("list_files").run)(json!({}), ctx.clone())
            .await
            .unwrap()
            .text,
        "(the project is empty)"
    );
    let write =
        (tool("write_file").run)(json!({"path":"nested/a.txt","contents":"×"}), ctx.clone())
            .await
            .unwrap();
    assert_eq!(write.text, "wrote nested/a.txt (2 bytes)");
    assert_eq!(write.files, ["nested/a.txt"]);
    assert_eq!(
        (tool("read_file").run)(json!({"path":"nested/a.txt"}), ctx.clone())
            .await
            .unwrap()
            .text,
        "×"
    );
    for path in ["../outside", ".git/config"] {
        assert!(
            (tool("write_file").run)(json!({"path":path,"contents":"x"}), ctx.clone())
                .await
                .is_err()
        );
    }
    assert_eq!(
        (tool("read_file").run)(json!({"path":"missing"}), ctx.clone())
            .await
            .unwrap_err()
            .to_string(),
        "missing does not exist"
    );
    std::fs::write(dir.path.join("big"), vec![0u8; 200_001]).unwrap();
    assert_eq!(
        (tool("read_file").run)(json!({"path":"big"}), ctx.clone())
            .await
            .unwrap_err()
            .to_string(),
        "big is 200001 bytes; only files up to 200000 bytes can be read"
    );
    std::fs::create_dir(dir.path.join("node_modules")).unwrap();
    std::fs::write(dir.path.join("node_modules/skip"), "x").unwrap();
    std::fs::write(dir.path.join("A"), "x").unwrap();
    std::fs::write(dir.path.join("a"), "x").unwrap();
    assert_eq!(
        (tool("list_files").run)(json!({}), ctx).await.unwrap().text,
        "A\na\nbig\nnested/a.txt"
    );
}

#[tokio::test]
async fn commands_require_explicit_opt_in_in_context_and_registry() {
    let dir = TempDir::new("commands");
    let mut settings = Settings::new(dir.path.clone(), dir.path.clone());
    let tool = all_tools()
        .into_iter()
        .find(|t| t.name == "run_command")
        .unwrap();
    assert!(!available_tools(&settings)
        .iter()
        .any(|t| t.name == "run_command"));
    let input = json!({"program":"sh","args":["-c","printf result"]});
    assert_eq!(
        (tool.run)(
            input.clone(),
            ToolContext::from_settings(dir.path.clone(), &settings)
        )
        .await
        .unwrap_err()
        .to_string(),
        "running commands is disabled; start Forge with FORGE_ALLOW_COMMANDS=1 to allow it"
    );
    settings.allow_commands = true;
    assert!(available_tools(&settings)
        .iter()
        .any(|t| t.name == "run_command"));
    assert_eq!(
        describe_tools(&settings)
            .iter()
            .find(|t| t["name"] == "run_command")
            .unwrap()["available"],
        true
    );
    assert_eq!(
        (tool.run)(
            input,
            ToolContext::from_settings(dir.path.clone(), &settings)
        )
        .await
        .unwrap()
        .text,
        "exit code 0\nstdout:\nresult"
    );
}

fn check_schema_node(tool_name: &str, path: &str, node: &Value, is_root: bool) {
    let obj = node.as_object().unwrap_or_else(|| {
        panic!("tool \"{tool_name}\" at {path}: node is not a JSON object");
    });

    // Rule 2: No {} nodes. Every schema node has `type` or a non-empty `anyOf`.
    if obj.is_empty() {
        panic!("tool \"{tool_name}\" at {path}: empty {{}} schema node");
    }
    let has_type = obj.get("type").and_then(Value::as_str).is_some();
    let has_any_of = obj
        .get("anyOf")
        .and_then(Value::as_array)
        .is_some_and(|arr| !arr.is_empty());
    assert!(
        has_type || has_any_of,
        "tool \"{tool_name}\" at {path}: node must have 'type' or a non-empty 'anyOf', found: {node:?}"
    );

    // Rule 3: No `$ref`, `oneOf`, `allOf`, `not`, `if`, `then` or `else`.
    for forbidden in ["$ref", "oneOf", "allOf", "not", "if", "then", "else"] {
        assert!(
            !obj.contains_key(forbidden),
            "tool \"{tool_name}\" at {path}: forbidden keyword \"{forbidden}\" present"
        );
    }

    // Rule 4: Every `type:"array"` node has `items`.
    if obj.get("type").and_then(Value::as_str) == Some("array") {
        assert!(
            obj.contains_key("items"),
            "tool \"{tool_name}\" at {path}: array node missing 'items'"
        );
    }

    // Rule 5: Every name in `required` exists in `properties`.
    if let Some(req) = obj.get("required") {
        let req_arr = req.as_array().unwrap_or_else(|| {
            panic!("tool \"{tool_name}\" at {path}: 'required' must be an array")
        });
        let props = obj.get("properties").and_then(Value::as_object);
        for item in req_arr {
            let key = item.as_str().unwrap_or_else(|| {
                panic!("tool \"{tool_name}\" at {path}: 'required' item must be a string")
            });
            assert!(
                props.is_some_and(|p| p.contains_key(key)),
                "tool \"{tool_name}\" at {path}: required key \"{key}\" does not exist in 'properties'"
            );
        }
    }

    // Rule 7: Every `enum` holds only strings.
    if let Some(en) = obj.get("enum") {
        let en_arr = en.as_array().unwrap_or_else(|| {
            panic!("tool \"{tool_name}\" at {path}: 'enum' must be an array")
        });
        for (i, val) in en_arr.iter().enumerate() {
            assert!(
                val.is_string(),
                "tool \"{tool_name}\" at {path}.enum[{i}]: enum item must be a string, found {val:?}"
            );
        }
    }

    // Recurse into properties
    if let Some(props) = obj.get("properties") {
        let props_obj = props.as_object().unwrap_or_else(|| {
            panic!("tool \"{tool_name}\" at {path}.properties: must be an object")
        });
        for (prop_name, prop_node) in props_obj {
            let prop_path = format!("{path}.properties.{prop_name}");
            if is_root {
                // Rule 6: Every top-level property has a non-empty `description`.
                let desc = prop_node.get("description").and_then(Value::as_str);
                assert!(
                    desc.is_some_and(|d| !d.trim().is_empty()),
                    "tool \"{tool_name}\" at {prop_path}: top-level property missing non-empty 'description'"
                );
            }
            check_schema_node(tool_name, &prop_path, prop_node, false);
        }
    }

    // Recurse into items
    if let Some(items) = obj.get("items") {
        if items.is_object() {
            check_schema_node(tool_name, &format!("{path}.items"), items, false);
        } else if let Some(arr) = items.as_array() {
            for (i, item_node) in arr.iter().enumerate() {
                check_schema_node(tool_name, &format!("{path}.items[{i}]"), item_node, false);
            }
        } else {
            panic!("tool \"{tool_name}\" at {path}.items: items must be a schema node");
        }
    }

    // Recurse into anyOf
    if let Some(any_of) = obj.get("anyOf") {
        let any_of_arr = any_of.as_array().unwrap_or_else(|| {
            panic!("tool \"{tool_name}\" at {path}.anyOf: must be an array")
        });
        for (i, branch) in any_of_arr.iter().enumerate() {
            check_schema_node(tool_name, &format!("{path}.anyOf[{i}]"), branch, false);
        }
    }
}

#[test]
#[ignore = "patch 0.15.1 not built yet"]
fn every_tool_schema_is_portable() {
    let tools = all_tools();
    assert!(!tools.is_empty(), "expected tools to be registered");

    for tool in &tools {
        // Rule 8: The tool name matches ^[a-z0-9_]{1,64}$
        assert!(
            !tool.name.is_empty()
                && tool.name.len() <= 64
                && tool.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "tool \"{}\" has invalid name; must match ^[a-z0-9_]{{1,64}}$",
            tool.name
        );

        let root = &tool.input_schema;
        let root_obj = root.as_object().unwrap_or_else(|| {
            panic!("tool \"{}\" at $: root is not an object", tool.name);
        });

        // Rule 1: The root is "type":"object" with a properties object
        assert_eq!(
            root_obj.get("type").and_then(Value::as_str),
            Some("object"),
            "tool \"{}\" at $: root must have type: \"object\"",
            tool.name
        );
        assert!(
            root_obj.get("properties").and_then(Value::as_object).is_some(),
            "tool \"{}\" at $: root must have a 'properties' object",
            tool.name
        );

        // Rule 9: The root has "additionalProperties": false
        assert_eq!(
            root_obj.get("additionalProperties").and_then(Value::as_bool),
            Some(false),
            "tool \"{}\" at $: root must have \"additionalProperties\": false",
            tool.name
        );

        check_schema_node(tool.name, "$", root, true);
    }
}

#[test]
#[ignore = "patch 0.15.1 not built yet"]
fn validator_handles_any_of_and_unknown_keys() {
    // 1. anyOf with 3 types: number, string, array
    let schema_three = json!({
        "type": "object",
        "properties": {
            "size": {
                "anyOf": [
                    { "type": "number" },
                    { "type": "string" },
                    { "type": "array", "items": { "type": "number" } },
                ]
            }
        }
    });

    let errs = validate(&json!({"size": true}), &schema_three);
    assert_eq!(
        errs,
        vec!["input.size must be a number, a string or an array"]
    );

    // Matching anyOf branches give no problems
    assert!(validate(&json!({"size": 42}), &schema_three).is_empty());
    assert!(validate(&json!({"size": "M3"}), &schema_three).is_empty());
    assert!(validate(&json!({"size": [10, 20]}), &schema_three).is_empty());

    // 2. anyOf with 2 types: number, string
    let schema_two = json!({
        "type": "object",
        "properties": {
            "size": {
                "anyOf": [
                    { "type": "number" },
                    { "type": "string" },
                ]
            }
        }
    });

    let errs_two = validate(&json!({"size": [1, 2]}), &schema_two);
    assert_eq!(
        errs_two,
        vec!["input.size must be a number or a string"]
    );

    // Matching anyOf branches give no problems
    assert!(validate(&json!({"size": 10}), &schema_two).is_empty());
    assert!(validate(&json!({"size": "small"}), &schema_two).is_empty());

    // 3. Unknown field when schema has additionalProperties: false
    let schema_unknown = json!({
        "type": "object",
        "properties": {
            "length": { "type": "number" }
        },
        "additionalProperties": false
    });

    let errs_unknown = validate(&json!({"lenght": 10}), &schema_unknown);
    assert_eq!(
        errs_unknown,
        vec!["input.lenght is not a known field"]
    );

    // Matching field gives no problems
    assert!(validate(&json!({"length": 10}), &schema_unknown).is_empty());
}
