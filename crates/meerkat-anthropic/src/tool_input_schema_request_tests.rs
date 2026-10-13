//! Tool input schemas projected onto an object root in the Anthropic request
//! body. Anthropic refuses a tool whose `input_schema` root carries
//! `oneOf`/`anyOf`/`allOf` or lacks `"type"`, and one such tool fails the
//! whole request. After `$ref` inlining and definition stripping, the request
//! builder folds such a root when every member fits the fold rules of
//! `tool_schema_root`, and otherwise refuses the tool locally with a typed
//! error naming it. The fold is faithful only within those rules, and it
//! loosens on purpose: a folded `oneOf`/`anyOf` no longer pairs a variant's
//! discriminator with that variant's own `required` names. Only the emitted
//! body changes: `ToolDef.input_schema` is untouched. These tests pin the
//! emitted form and the refusals for the shapes below; they make no claim
//! about third-party schemas in general.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use meerkat_core::{Message, ToolDef, UserMessage};
use meerkat_llm_core::{LlmError, LlmRequest};
use serde_json::{Value, json};

use crate::AnthropicClient;

const MODEL: &str = "claude-sonnet-4-6";

fn body_with_tool(name: &str, input_schema: Value) -> Result<Value, LlmError> {
    let tool = Arc::new(ToolDef::new(name, format!("{name} tool"), input_schema));
    let request =
        LlmRequest::new(MODEL, vec![Message::User(UserMessage::text("hi"))]).with_tools(vec![tool]);
    AnthropicClient::new("test-key".to_string())
        .unwrap()
        .build_request_body(&request)
}

fn emitted_input_schema(name: &str, input_schema: Value) -> Value {
    let body = body_with_tool(name, input_schema).expect("request body builds");
    let tools = body["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 1, "{body}");
    assert_eq!(tools[0]["name"], name);
    tools[0]["input_schema"].clone()
}

/// The folded form of the two `select_target` variants below: merged
/// properties, the literal discriminators merged into one `enum`, and only
/// the names every variant requires. `kind` no longer selects which id is
/// required: that is the pairing the fold loosens on purpose.
fn folded_select_target() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {"enum": ["session", "owner"]},
            "session_id": {"type": "string"},
            "owner_key": {"type": "string"}
        },
        "required": ["kind"]
    })
}

#[test]
fn root_one_of_object_variants_fold_into_an_object_root() {
    let schema = json!({
        "oneOf": [
            {"type": "object",
             "properties": {"kind": {"const": "session"}, "session_id": {"type": "string"}},
             "required": ["kind", "session_id"]},
            {"type": "object",
             "properties": {"kind": {"const": "owner"}, "owner_key": {"type": "string"}},
             "required": ["kind", "owner_key"]}
        ]
    });

    let emitted = emitted_input_schema("select_target", schema);

    assert_eq!(emitted, folded_select_target(), "{emitted}");
}

#[test]
fn root_one_of_local_refs_are_inlined_before_the_fold() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [{"$ref": "#/$defs/BySession"}, {"$ref": "#/$defs/ByOwner"}],
        "$defs": {
            "BySession": {"type": "object",
                "properties": {"kind": {"const": "session"}, "session_id": {"type": "string"}},
                "required": ["kind", "session_id"]},
            "ByOwner": {"type": "object",
                "properties": {"kind": {"const": "owner"}, "owner_key": {"type": "string"}},
                "required": ["kind", "owner_key"]}
        }
    });

    let emitted = emitted_input_schema("select_target", schema);

    assert_eq!(emitted, folded_select_target(), "{emitted}");
}

#[test]
fn root_any_of_object_variants_fold_into_an_object_root() {
    let schema = json!({
        "description": "Look up a record by id or by name.",
        "anyOf": [
            {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]},
            {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}
        ]
    });

    let emitted = emitted_input_schema("lookup", schema);

    assert_eq!(
        emitted,
        json!({
            "description": "Look up a record by id or by name.",
            "type": "object",
            "properties": {"id": {"type": "string"}, "name": {"type": "string"}}
        }),
        "no name is required by every variant, so none stays required: {emitted}"
    );
}

#[test]
fn root_all_of_members_fold_with_the_union_of_required() {
    let schema = json!({
        "properties": {"path": {"type": "string"}},
        "required": ["path"],
        "allOf": [
            {"type": "object",
             "properties": {"mode": {"type": "string", "enum": ["read", "write"]}},
             "required": ["mode"]},
            {"properties": {"encoding": {"type": "string"}}, "required": ["encoding"]}
        ]
    });

    let emitted = emitted_input_schema("open_file", schema);

    assert_eq!(
        emitted,
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "mode": {"type": "string", "enum": ["read", "write"]},
                "encoding": {"type": "string"}
            },
            "required": ["path", "mode", "encoding"]
        }),
        "{emitted}"
    );
}

#[test]
fn root_without_type_is_declared_an_object() {
    let schema = json!({
        "properties": {"query": {"type": "string"}},
        "required": ["query"]
    });

    let emitted = emitted_input_schema("search", schema);

    assert_eq!(
        emitted,
        json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        }),
        "{emitted}"
    );
}

#[test]
fn non_object_root_is_refused_with_a_typed_error_naming_the_tool() {
    let string_root = json!({"type": "string", "enum": ["fast", "slow"]});
    match body_with_tool("set_speed", string_root) {
        Err(LlmError::InvalidRequest { message }) => {
            assert!(message.contains("`set_speed`"), "{message}");
            assert!(message.contains("\"string\""), "{message}");
            assert!(message.contains("Anthropic"), "{message}");
        }
        other => panic!("expected a typed InvalidRequest naming the tool, got {other:?}"),
    }

    assert!(
        matches!(
            body_with_tool("set_speed", json!(true)),
            Err(LlmError::InvalidRequest { .. })
        ),
        "a schema that is not a JSON object is refused the same way"
    );
}

#[test]
fn object_schema_already_in_shape_is_sent_unchanged() {
    let schema = json!({
        "type": "object",
        "title": "LookupParams",
        "description": "Parameters for lookup",
        "properties": {
            "id": {"type": "string", "description": "record id"},
            "mode": {"oneOf": [{"const": "fast"}, {"const": "exact"}]},
            "filter": {
                "type": "object",
                "properties": {"tag": {"type": "string"}},
                "additionalProperties": false
            }
        },
        "required": ["id"],
        "additionalProperties": false
    });

    let emitted = emitted_input_schema("lookup", schema.clone());

    assert_eq!(
        emitted, schema,
        "nested combinators and root annotations stay as authored"
    );
}

fn refusal_message(name: &str, input_schema: Value) -> String {
    match body_with_tool(name, input_schema) {
        Err(LlmError::InvalidRequest { message }) => message,
        other => panic!("expected a typed refusal for `{name}`, got {other:?}"),
    }
}

#[test]
fn variants_defining_a_property_differently_emit_a_property_level_any_of() {
    let schema = json!({
        "oneOf": [
            {"type": "object",
             "properties": {"source": {"const": "path"}, "target": {"type": "string"}},
             "required": ["source", "target"]},
            {"type": "object",
             "properties": {
                 "source": {"const": "inline"},
                 "target": {"type": "object", "properties": {"body": {"type": "string"}}}
             },
             "required": ["source", "target"]},
            {"type": "object",
             "properties": {"source": {"const": "url"}, "target": {"type": "string"}},
             "required": ["source", "target"]}
        ]
    });

    let emitted = emitted_input_schema("load_document", schema);

    assert_eq!(
        emitted,
        json!({
            "type": "object",
            "properties": {
                "source": {"enum": ["path", "inline", "url"]},
                "target": {"anyOf": [
                    {"type": "string"},
                    {"type": "object", "properties": {"body": {"type": "string"}}}
                ]}
            },
            "required": ["source", "target"]
        }),
        "every variant's definition stays reachable, deduplicated: {emitted}"
    );
}

#[test]
fn definitions_that_all_apply_emit_a_property_level_all_of() {
    let all_of_members = json!({
        "allOf": [
            {"properties": {"path": {"type": "string"}}, "required": ["path"]},
            {"properties": {"path": {"type": "string", "minLength": 1}}}
        ]
    });
    assert_eq!(
        emitted_input_schema("open_file", all_of_members),
        json!({
            "type": "object",
            "properties": {"path": {"allOf": [
                {"type": "string"},
                {"type": "string", "minLength": 1}
            ]}},
            "required": ["path"]
        })
    );

    let root_narrowed_by_variants = json!({
        "type": "object",
        "properties": {"kind": {"type": "string", "description": "what to select"}},
        "oneOf": [
            {"properties": {"kind": {"const": "session"}}},
            {"properties": {"kind": {"const": "owner"}}}
        ]
    });
    assert_eq!(
        emitted_input_schema("select_target", root_narrowed_by_variants),
        json!({
            "type": "object",
            "properties": {"kind": {"allOf": [
                {"type": "string", "description": "what to select"},
                {"enum": ["session", "owner"]}
            ]}}
        })
    );
}

#[test]
fn non_object_alternatives_are_refused_naming_the_tool() {
    for (schema, branch) in [
        (
            json!({"oneOf": [
                {"type": "object", "properties": {"path": {"type": "string"}}},
                {"type": "string"}
            ]}),
            "`oneOf[1]`",
        ),
        (
            json!({"anyOf": [{"type": "array", "items": {"type": "string"}}]}),
            "`anyOf[0]`",
        ),
        (json!({"allOf": [true]}), "`allOf[0]`"),
        (
            json!({"anyOf": [{"description": "anything at all"}]}),
            "`anyOf[0]`",
        ),
        (
            json!({"oneOf": [
                {"type": ["object", "null"], "properties": {"id": {"type": "string"}}}
            ]}),
            "`oneOf[0]`",
        ),
        (
            json!({"anyOf": [
                {"type": "object", "properties": {"id": {"type": "string"}}},
                {}
            ]}),
            "`anyOf[1]`",
        ),
    ] {
        let message = refusal_message("list_files", schema);
        assert!(message.contains("`list_files`"), "{message}");
        assert!(message.contains(branch), "{message}");
        assert!(message.contains("non-object alternative"), "{message}");
    }
}

#[test]
fn member_keywords_a_merged_root_cannot_keep_are_refused_naming_them() {
    for (schema, keyword) in [
        (
            json!({"oneOf": [{
                "type": "object",
                "properties": {"mode": {"type": "string"}},
                "if": {"properties": {"mode": {"const": "fast"}}},
                "then": {"required": ["budget"]}
            }]}),
            "`if`",
        ),
        (
            json!({"anyOf": [{"type": "object", "minProperties": 1}]}),
            "`minProperties`",
        ),
        (
            json!({"allOf": [
                {"type": "object", "patternProperties": {"^x-": {"type": "string"}}}
            ]}),
            "`patternProperties`",
        ),
        (
            json!({"anyOf": [{"type": "object", "dependentRequired": {"a": ["b"]}}]}),
            "`dependentRequired`",
        ),
        (
            json!({"oneOf": [
                {"type": "object", "oneOf": [{"required": ["a"]}, {"required": ["b"]}]}
            ]}),
            "`oneOf`",
        ),
    ] {
        let message = refusal_message("configure", schema);
        assert!(message.contains("`configure`"), "{message}");
        assert!(message.contains(keyword), "{message}");
    }
}

#[test]
fn malformed_members_and_empty_alternatives_are_refused_naming_the_branch() {
    for (schema, branch, reason) in [
        (
            json!({"oneOf": [{"type": "object", "required": "id"}]}),
            "`oneOf[0].required`",
            "not an array of strings",
        ),
        (
            json!({"anyOf": [{
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "required": ["id", 3]
            }]}),
            "`anyOf[0].required`",
            "not an array of strings",
        ),
        (
            json!({"allOf": [{"type": "object", "properties": ["id"]}]}),
            "`allOf[0].properties`",
            "not an object",
        ),
        (json!({"oneOf": []}), "`oneOf`", "empty"),
        (json!({"anyOf": []}), "`anyOf`", "empty"),
        (
            json!({"properties": {"id": {"type": "string"}}, "required": "id"}),
            "the input schema root's `required`",
            "not an array of strings",
        ),
        (
            json!({
                "required": ["id", 1],
                "oneOf": [{"type": "object", "properties": {"id": {"type": "string"}}}]
            }),
            "the input schema root's `required`",
            "not an array of strings",
        ),
    ] {
        let message = refusal_message("configure", schema);
        assert!(message.contains("`configure`"), "{message}");
        assert!(message.contains(branch), "{message}");
        assert!(message.contains(reason), "{message}");
    }

    assert_eq!(
        emitted_input_schema(
            "configure",
            json!({"type": "object", "properties": {"id": {"type": "string"}}, "allOf": []})
        ),
        json!({"type": "object", "properties": {"id": {"type": "string"}}}),
        "an empty allOf admits everything and is removed"
    );
}

#[test]
fn object_root_with_malformed_required_is_refused() {
    for required in [json!("id"), json!(["id", 1])] {
        let message = refusal_message(
            "lookup",
            json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": required}),
        );
        assert!(message.contains("`lookup`"), "{message}");
        assert!(
            message.contains("the input schema root's `required`"),
            "{message}"
        );
        assert!(message.contains("not an array of strings"), "{message}");
    }
}

#[test]
fn root_not_and_enum_are_left_as_they_are() {
    let with_not = json!({
        "oneOf": [
            {"type": "object",
             "properties": {"lease_seconds": {"type": "integer"}},
             "required": ["lease_seconds"]},
            {"type": "object",
             "properties": {"lease_expires_at": {"type": "string"}},
             "required": ["lease_expires_at"]}
        ],
        "not": {"required": ["lease_seconds", "lease_expires_at"]}
    });
    assert_eq!(
        emitted_input_schema("claim", with_not),
        json!({
            "type": "object",
            "properties": {
                "lease_seconds": {"type": "integer"},
                "lease_expires_at": {"type": "string"}
            },
            "not": {"required": ["lease_seconds", "lease_expires_at"]}
        })
    );

    let with_enum = json!({
        "properties": {"mode": {"type": "string"}},
        "enum": [{"mode": "fast"}, {"mode": "exact"}]
    });
    assert_eq!(
        emitted_input_schema("set_mode", with_enum),
        json!({
            "type": "object",
            "properties": {"mode": {"type": "string"}},
            "enum": [{"mode": "fast"}, {"mode": "exact"}]
        })
    );
}

#[test]
fn additional_properties_false_is_set_only_when_every_member_sets_it() {
    let closed = json!({
        "anyOf": [
            {"type": "object",
             "properties": {"id": {"type": "string"}},
             "additionalProperties": false},
            {"type": "object",
             "properties": {"name": {"type": "string"}},
             "additionalProperties": false}
        ]
    });
    assert_eq!(
        emitted_input_schema("lookup", closed),
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}, "name": {"type": "string"}},
            "additionalProperties": false
        })
    );

    let one_open = json!({
        "anyOf": [
            {"type": "object",
             "properties": {"id": {"type": "string"}},
             "additionalProperties": false},
            {"type": "object", "properties": {"name": {"type": "string"}}}
        ]
    });
    assert_eq!(
        emitted_input_schema("lookup", one_open),
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}, "name": {"type": "string"}}
        })
    );

    let root_decides = json!({
        "additionalProperties": true,
        "anyOf": [
            {"type": "object",
             "properties": {"id": {"type": "string"}},
             "additionalProperties": false}
        ]
    });
    assert_eq!(
        emitted_input_schema("lookup", root_decides),
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "additionalProperties": true
        })
    );

    let all_open = json!({
        "anyOf": [
            {"type": "object",
             "properties": {"id": {"type": "string"}},
             "additionalProperties": true},
            {"type": "object",
             "properties": {"name": {"type": "string"}},
             "additionalProperties": true}
        ]
    });
    assert_eq!(
        emitted_input_schema("lookup", all_open),
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}, "name": {"type": "string"}}
        })
    );

    let mixed = json!({
        "anyOf": [
            {"type": "object",
             "properties": {"id": {"type": "string"}},
             "additionalProperties": false},
            {"type": "object",
             "properties": {"name": {"type": "string"}},
             "additionalProperties": true}
        ]
    });
    assert_eq!(
        emitted_input_schema("lookup", mixed),
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}, "name": {"type": "string"}}
        })
    );
}

#[test]
fn schema_valued_member_additional_properties_is_refused_naming_the_branch() {
    for (schema, branch) in [
        (
            json!({"allOf": [{"type": "object", "additionalProperties": {"type": "string"}}]}),
            "`allOf[0].additionalProperties`",
        ),
        (
            json!({"oneOf": [
                {"type": "object", "properties": {"kind": {"const": "plain"}}},
                {"type": "object",
                 "properties": {"kind": {"const": "labelled"}},
                 "additionalProperties": {"type": "string"}}
            ]}),
            "`oneOf[1].additionalProperties`",
        ),
    ] {
        let message = refusal_message("annotate", schema);
        assert!(message.contains("`annotate`"), "{message}");
        assert!(message.contains(branch), "{message}");
        assert!(message.contains("not a boolean"), "{message}");
    }

    assert_eq!(
        emitted_input_schema(
            "annotate",
            json!({"anyOf": [
                {"type": "object",
                 "properties": {"id": {"type": "string"}},
                 "additionalProperties": true},
                {"type": "object",
                 "properties": {"name": {"type": "string"}},
                 "additionalProperties": false}
            ]})
        ),
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}, "name": {"type": "string"}}
        }),
        "a boolean member additionalProperties is still accepted"
    );
}

#[test]
fn type_less_root_describing_a_non_object_value_is_refused() {
    let message = refusal_message("tags", json!({"items": {"type": "string"}, "minItems": 1}));
    assert!(message.contains("`tags`"), "{message}");
    assert!(message.contains("`items`"), "{message}");
}

#[test]
fn the_projection_rewrites_only_the_request_body() {
    let original = json!({
        "oneOf": [
            {"type": "object",
             "properties": {"kind": {"const": "session"}, "session_id": {"type": "string"}},
             "required": ["kind", "session_id"]},
            {"type": "object",
             "properties": {"kind": {"const": "owner"}, "owner_key": {"type": "string"}},
             "required": ["kind", "owner_key"]}
        ]
    });
    let tool = Arc::new(ToolDef::new(
        "select_target",
        "select_target tool",
        original.clone(),
    ));
    let request = LlmRequest::new(MODEL, vec![Message::User(UserMessage::text("hi"))])
        .with_tools(vec![Arc::clone(&tool)]);
    let body = AnthropicClient::new("test-key".to_string())
        .unwrap()
        .build_request_body(&request)
        .expect("request body builds");

    assert_eq!(body["tools"][0]["input_schema"], folded_select_target());
    assert_eq!(
        tool.input_schema, original,
        "the tool definition is not rewritten"
    );
    assert_eq!(request.tools[0].input_schema, original);
}
