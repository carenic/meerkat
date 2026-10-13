//! Root projection of a tool `input_schema` for the Anthropic request body.
//!
//! Anthropic refuses a tool whose `input_schema` root carries `oneOf`,
//! `anyOf` or `allOf`, or has no `"type"`, and one such tool fails the whole
//! request. [`project_tool_schema_root`] rewrites the root of the emitted copy
//! only; `ToolDef.input_schema` is never touched. Nothing below the root is
//! rewritten here: the `$ref` inlining and definition stripping that run first
//! are the only changes to nested schemas.
//!
//! These are Anthropic-specific rules; `meerkat-openai` keeps its own root fold
//! in `tool_schema.rs`. A schema outside them is refused, not projected:
//! - a root `required` that is present but not an array of strings is
//!   refused;
//! - a root combinator is folded only when every member is an object schema
//!   (`"type": "object"`, or no type with `properties` or `required`) that
//!   carries nothing beyond [`MEMBER_KEYWORDS`], with `required` an array of
//!   strings, `properties` an object and `additionalProperties` a boolean
//!   where present; anything else is refused with a typed error naming the
//!   tool and the branch;
//! - an empty root `oneOf` or `anyOf` admits nothing and is refused; an empty
//!   `allOf` admits everything and is removed;
//! - a property the members define identically is kept once; differing
//!   definitions become a property-level `anyOf` for `oneOf`/`anyOf`, where
//!   literal-only definitions (discriminators) merge into one `enum` instead,
//!   or a property-level `allOf` for `allOf`; a root definition that differs
//!   from the merged one becomes `allOf: [root, merged]`;
//! - `required` is the intersection over `oneOf`/`anyOf` members and the union
//!   over `allOf` members, added to the root's own;
//! - `additionalProperties: false` is set on the root only when every member
//!   sets it and the root does not set `additionalProperties` itself, and it
//!   then admits every property the root or any member defines; a member's
//!   `true` sets nothing;
//! - a root `not` or `enum` is left as it is: Anthropic accepts both.
//!
//! The projection is faithful only within these rules, and it loosens on
//! purpose: a folded `oneOf`/`anyOf` no longer pairs a variant's discriminator
//! with that variant's own `required` names and properties, and does not keep
//! `oneOf`'s exactly-one, so the folded root can admit an argument object that
//! no single variant admits; a members-wide `additionalProperties: false`
//! admits every property the root or any member defines.
//!
//! The projection changes only what the request body advertises to the model.
//! The factory-composed tool dispatch does not validate tool arguments against
//! `input_schema` at execution (only `meerkat_tools::ToolDispatcher` does, and
//! it reads the unchanged `ToolDef.input_schema`), so nothing changes at
//! execution either. These rules make no claim of general third-party schema
//! compatibility.

use meerkat_llm_core::LlmError;
use serde_json::{Map, Value, json};

/// Root keywords Anthropic refuses, folded in this order.
const ROOT_COMBINATORS: [(&str, Applies); 3] = [
    ("allOf", Applies::Every),
    ("oneOf", Applies::One),
    ("anyOf", Applies::One),
];

/// Keywords a root combinator member may carry, `additionalProperties` only as
/// a boolean. Any other keyword, or a schema-valued `additionalProperties`, is
/// a constraint a merged object root could not keep, so the member is refused.
const MEMBER_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "description",
    "title",
    "$comment",
];

/// Keywords that only describe non-object values: a root without `type`,
/// `properties` or `required` that carries one is not an object schema.
const NON_OBJECT_KEYWORDS: &[&str] = &[
    "items",
    "prefixItems",
    "additionalItems",
    "contains",
    "minContains",
    "maxContains",
    "minItems",
    "maxItems",
    "uniqueItems",
    "unevaluatedItems",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "contentEncoding",
    "contentMediaType",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
];

/// How the members of a folded combinator apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Applies {
    /// `allOf`: every member applies.
    Every,
    /// `oneOf`/`anyOf`: one member applies.
    One,
}

/// Whether `root` must go through [`project_tool_schema_root`]: it carries a
/// root combinator, a `type` other than `"object"`, or a `required` that is not
/// an array of strings (which the projection refuses). Read-only.
pub(crate) fn root_needs_projection(root: &Map<String, Value>) -> bool {
    ROOT_COMBINATORS
        .iter()
        .any(|(keyword, _)| root.contains_key(*keyword))
        || !matches!(root.get("type"), Some(Value::String(kind)) if kind == "object")
        || root
            .get("required")
            .is_some_and(|required| !is_name_list(required))
}

/// Project an inlined, definition-stripped `input_schema` root onto one
/// `"type": "object"` schema for the request body.
///
/// # Errors
///
/// [`LlmError::InvalidRequest`] naming `tool_name` and the reason when the root
/// is not an object schema or a root combinator falls outside the rules in the
/// module docs.
pub(crate) fn project_tool_schema_root(
    tool_name: &str,
    root: &mut Map<String, Value>,
) -> Result<(), LlmError> {
    if let Some(declared) = root.get("type")
        && !is_object_type(declared)
    {
        return Err(tool_schema_refusal(
            tool_name,
            &format!("the input schema root declares type {declared}"),
        ));
    }
    if let Some(required) = root.get("required")
        && !is_name_list(required)
    {
        return Err(tool_schema_refusal(
            tool_name,
            "the input schema root's `required` is not an array of strings",
        ));
    }

    let root_sets_additional_properties = root.contains_key("additionalProperties");
    let mut every_member_closed = None;
    for (keyword, applies) in ROOT_COMBINATORS {
        if let Some(closed) = fold(tool_name, root, keyword, applies)? {
            every_member_closed = Some(every_member_closed.unwrap_or(true) && closed);
        }
    }
    if every_member_closed == Some(true) && !root_sets_additional_properties {
        root.insert("additionalProperties".to_string(), Value::Bool(false));
    }

    if !root.contains_key("type")
        && !root.contains_key("properties")
        && !root.contains_key("required")
        && let Some(keyword) = non_object_keyword(root)
    {
        return Err(tool_schema_refusal(
            tool_name,
            &format!(
                "the input schema root has no type, and its `{keyword}` describes a non-object \
                 value"
            ),
        ));
    }
    root.insert("type".to_string(), Value::String("object".to_string()));
    Ok(())
}

/// The refusal for a tool whose schema cannot be sent: `reason` names what
/// the projection does not cover.
pub(crate) fn tool_schema_refusal(tool_name: &str, reason: &str) -> LlmError {
    LlmError::InvalidRequest {
        message: format!(
            "tool `{tool_name}` cannot be sent to Anthropic: {reason}. Anthropic needs one \
             `\"type\": \"object\"` input schema root, and Meerkat's projection onto one does \
             not cover this schema, so the tool definition has to be fixed before it can be sent"
        ),
    }
}

/// The JSON kind of a value that is not an object schema, for refusals.
pub(crate) fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Whether `required` is an array of property names.
fn is_name_list(required: &Value) -> bool {
    matches!(required, Value::Array(names) if names.iter().all(Value::is_string))
}

fn is_object_type(declared: &Value) -> bool {
    match declared {
        Value::String(kind) => kind == "object",
        Value::Array(kinds) => !kinds.is_empty() && kinds.iter().all(|kind| kind == "object"),
        _ => false,
    }
}

/// A keyword, or a `const`/`enum` literal, that marks `schema` as describing a
/// non-object value.
fn non_object_keyword(schema: &Map<String, Value>) -> Option<&'static str> {
    if let Some(keyword) = NON_OBJECT_KEYWORDS
        .iter()
        .copied()
        .find(|keyword| schema.contains_key(*keyword))
    {
        return Some(keyword);
    }
    if schema
        .get("const")
        .is_some_and(|literal| !literal.is_object())
    {
        return Some("const");
    }
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|literals| literals.iter().any(|literal| !literal.is_object()))
    {
        return Some("enum");
    }
    None
}

/// Fold the root combinator `keyword` into `root`. Returns whether every
/// member sets `additionalProperties: false`, or `None` when there was no
/// member to fold.
fn fold(
    tool_name: &str,
    root: &mut Map<String, Value>,
    keyword: &str,
    applies: Applies,
) -> Result<Option<bool>, LlmError> {
    let Some(members) = root.remove(keyword) else {
        return Ok(None);
    };
    let Value::Array(members) = members else {
        return Err(tool_schema_refusal(
            tool_name,
            &format!("`{keyword}` is not an array"),
        ));
    };
    if members.is_empty() {
        return match applies {
            Applies::Every => Ok(None),
            Applies::One => Err(tool_schema_refusal(
                tool_name,
                &format!("`{keyword}` is empty, which no value satisfies"),
            )),
        };
    }

    // Each property's distinct definitions across the members, in order.
    let mut definitions: Vec<(String, Vec<Value>)> = Vec::new();
    let mut required: Option<Vec<Value>> = None;
    let mut every_member_closed = true;
    for (index, member) in members.into_iter().enumerate() {
        let branch = format!("{keyword}[{index}]");
        let mut member = object_member(tool_name, &branch, member)?;

        let member_required = match member.remove("required") {
            None => Vec::new(),
            Some(Value::Array(names)) if names.iter().all(Value::is_string) => names,
            Some(_) => {
                return Err(tool_schema_refusal(
                    tool_name,
                    &format!("`{branch}.required` is not an array of strings"),
                ));
            }
        };
        required = Some(match (required.take(), applies) {
            (None, _) => member_required,
            (Some(mut accumulated), Applies::Every) => {
                push_distinct(&mut accumulated, member_required);
                accumulated
            }
            (Some(accumulated), Applies::One) => accumulated
                .into_iter()
                .filter(|name| member_required.contains(name))
                .collect(),
        });
        every_member_closed &= match member.get("additionalProperties") {
            None => false,
            Some(Value::Bool(allowed)) => !*allowed,
            Some(_) => {
                return Err(tool_schema_refusal(
                    tool_name,
                    &format!("`{branch}.additionalProperties` is not a boolean"),
                ));
            }
        };

        match member.remove("properties") {
            None => {}
            Some(Value::Object(properties)) => {
                for (name, schema) in properties {
                    match definitions.iter_mut().find(|(known, _)| *known == name) {
                        Some((_, known)) => push_distinct(known, [schema]),
                        None => definitions.push((name, vec![schema])),
                    }
                }
            }
            Some(_) => {
                return Err(tool_schema_refusal(
                    tool_name,
                    &format!("`{branch}.properties` is not an object"),
                ));
            }
        }
    }

    if !definitions.is_empty() {
        let properties = root
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(properties) = properties else {
            return Err(tool_schema_refusal(
                tool_name,
                "the input schema root's `properties` is not an object",
            ));
        };
        for (name, member_definitions) in definitions {
            let merged = match applies {
                Applies::Every => all_of(member_definitions),
                Applies::One => any_of(member_definitions),
            };
            let projected = match properties.get_mut(&name).map(Value::take) {
                Some(existing) if existing != merged => all_of(vec![existing, merged]),
                Some(_) | None => merged,
            };
            properties.insert(name, projected);
        }
    }

    if let Some(required) = required.filter(|names| !names.is_empty()) {
        let parent = root
            .entry("required")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(parent) = parent {
            push_distinct(parent, required);
        }
    }
    Ok(Some(every_member_closed))
}

/// `member` as an object schema a merged root can keep, or the refusal.
fn object_member(
    tool_name: &str,
    branch: &str,
    member: Value,
) -> Result<Map<String, Value>, LlmError> {
    let Value::Object(member) = member else {
        return Err(tool_schema_refusal(
            tool_name,
            &format!(
                "`{branch}` is a non-object alternative ({})",
                kind_of(&member)
            ),
        ));
    };
    let object_shaped = match member.get("type") {
        Some(declared) => is_object_type(declared),
        None => member.contains_key("properties") || member.contains_key("required"),
    };
    if !object_shaped {
        let shape = member.get("type").map_or_else(
            || "no type, properties or required".to_string(),
            |declared| format!("type {declared}"),
        );
        return Err(tool_schema_refusal(
            tool_name,
            &format!("`{branch}` is a non-object alternative ({shape})"),
        ));
    }

    let mut unsupported: Vec<&str> = member
        .keys()
        .map(String::as_str)
        .filter(|key| !MEMBER_KEYWORDS.contains(key))
        .collect();
    if !unsupported.is_empty() {
        unsupported.sort_unstable();
        let listed = unsupported
            .iter()
            .map(|key| format!("`{key}`"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(tool_schema_refusal(
            tool_name,
            &format!("`{branch}` carries {listed}, which a merged object root cannot keep"),
        ));
    }
    Ok(member)
}

/// A schema every one of `definitions` applies to.
fn all_of(definitions: Vec<Value>) -> Value {
    match <[Value; 1]>::try_from(definitions) {
        Ok([single]) => single,
        Err(definitions) => json!({ "allOf": definitions }),
    }
}

/// A schema admitting what any one of `definitions` admits: literal-only
/// definitions merge into one `enum`, anything else becomes an `anyOf`.
fn any_of(definitions: Vec<Value>) -> Value {
    let definitions = match <[Value; 1]>::try_from(definitions) {
        Ok([single]) => return single,
        Err(definitions) => definitions,
    };
    let Some(literals) = definitions
        .iter()
        .map(literal_value)
        .collect::<Option<Vec<_>>>()
    else {
        return json!({ "anyOf": definitions });
    };

    let mut merged = Vec::new();
    push_distinct(&mut merged, literals);
    let first_type = definitions.first().and_then(|first| first.get("type"));
    let shared_type = definitions
        .iter()
        .all(|definition| definition.get("type") == first_type);
    let mut schema = definitions
        .first()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    schema.remove("const");
    schema.insert("enum".to_string(), Value::Array(merged));
    if !shared_type {
        schema.remove("type");
    }
    Value::Object(schema)
}

/// The one literal a `const` or single-value `enum` schema admits, or `None`
/// when the schema is anything else.
fn literal_value(schema: &Value) -> Option<Value> {
    let obj = schema.as_object()?;
    if obj.keys().any(|key| {
        !matches!(
            key.as_str(),
            "const" | "enum" | "type" | "title" | "description"
        )
    }) {
        return None;
    }
    match (obj.get("const"), obj.get("enum")) {
        (Some(literal), None) => Some(literal.clone()),
        (None, Some(Value::Array(literals))) if literals.len() == 1 => literals.first().cloned(),
        _ => None,
    }
}

fn push_distinct(into: &mut Vec<Value>, values: impl IntoIterator<Item = Value>) {
    for value in values {
        if !into.contains(&value) {
            into.push(value);
        }
    }
}
