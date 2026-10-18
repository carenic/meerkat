//! SKILL.md frontmatter parser.
//!
//! Uses `serde_yaml` for robust YAML parsing of the frontmatter block.

use indexmap::IndexMap;
use meerkat_core::skills::{
    CapabilityId, SkillDescriptor, SkillDocument, SkillError, SkillExtensionKey, SkillKey,
    SkillName, SkillScope,
};
use serde_yaml::Value;

/// Parsed frontmatter from a SKILL.md file.
#[derive(Debug, serde::Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
    #[serde(default)]
    metadata: IndexMap<String, String>,
    #[serde(default)]
    requires_capabilities: Vec<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(flatten)]
    extensions: IndexMap<String, Value>,
}

/// Parse a SKILL.md file into a `SkillDocument`.
pub fn parse_skill_md(
    key: SkillKey,
    scope: SkillScope,
    content: &str,
    expected_skill_name: Option<&str>,
) -> Result<SkillDocument, SkillError> {
    let (frontmatter_str, body) = split_frontmatter(content)?;
    let fm: Frontmatter = serde_yaml::from_str(&frontmatter_str)
        .map_err(|e| SkillError::Parse(format!("frontmatter parse error: {e}").into()))?;
    validate_frontmatter(&fm, expected_skill_name)?;

    let mut metadata = fm.metadata;
    if let Some(version) = fm.version {
        metadata.insert("version".to_string(), version);
    }

    let mut capability_requirements: Vec<CapabilityId> = Vec::new();
    for raw in fm.requires_capabilities {
        capability_requirements.push(CapabilityId::parse(&raw)?);
    }

    let extensions = parse_extensions(fm.extensions)?;

    Ok(SkillDocument {
        descriptor: SkillDescriptor {
            key,
            name: fm.name,
            description: fm.description,
            scope,
            metadata,
            capability_requirements,
            source_name: String::new(),
        },
        body: body.to_string(),
        extensions,
    })
}

fn validate_frontmatter(
    fm: &Frontmatter,
    expected_skill_name: Option<&str>,
) -> Result<(), SkillError> {
    let skill_name = SkillName::parse(&fm.name)?;
    if let Some(expected) = expected_skill_name
        && skill_name.as_str() != expected
    {
        return Err(SkillError::Parse(
            format!(
                "frontmatter name '{}' must match directory slug '{}'",
                fm.name, expected
            )
            .into(),
        ));
    }

    Ok(())
}

/// Parse the raw flattened frontmatter extensions into a typed
/// `namespace.key` → value map.
///
/// Each key is parsed into a [`SkillExtensionKey`] once, fail-closed: any
/// non-namespaced or malformed key (the only frontmatter fields that reach the
/// `#[serde(flatten)]` catch-all) errors here rather than silently surviving as
/// a re-split string. Values stay open-ended author metadata; scalars are kept
/// verbatim and structured values are serialized. A structured (non-scalar)
/// value is policy-bearing and must declare a companion `<namespace>.version`,
/// enforced over the typed key set.
fn parse_extensions(
    raw: IndexMap<String, Value>,
) -> Result<IndexMap<SkillExtensionKey, String>, SkillError> {
    let mut typed: IndexMap<SkillExtensionKey, (Value, String)> =
        IndexMap::with_capacity(raw.len());
    for (k, value) in raw {
        let key = SkillExtensionKey::parse(&k)?;
        let serialized = if let Some(s) = value.as_str() {
            s.to_string()
        } else {
            serde_yaml::to_string(&value)
                .map_err(|e| SkillError::Parse(format!("extension serialize error: {e}").into()))?
                .trim()
                .to_string()
        };
        typed.insert(key, (value, serialized));
    }

    for (key, (value, _)) in &typed {
        if key.is_version() {
            continue;
        }
        let nontrivial = matches!(value, Value::Mapping(_) | Value::Sequence(_));
        if nontrivial && !typed.contains_key(&key.version_companion()) {
            return Err(SkillError::Parse(
                format!(
                    "extension '{key}' requires companion '{}' for nontrivial values",
                    key.version_companion()
                )
                .into(),
            ));
        }
    }

    Ok(typed
        .into_iter()
        .map(|(key, (_, serialized))| (key, serialized))
        .collect())
}

/// Split content into frontmatter and body, separated by `---` delimiters.
fn split_frontmatter(content: &str) -> Result<(String, &str), SkillError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let trimmed = content.trim_start();
    let Some(first_line_end) = trimmed.find('\n') else {
        return Err(SkillError::Parse(
            "missing closing frontmatter delimiter".into(),
        ));
    };
    let first_line = trimmed[..first_line_end].trim_end_matches('\r').trim();
    if first_line != "---" {
        return Err(SkillError::Parse("missing frontmatter delimiter".into()));
    }

    let after_first = &trimmed[first_line_end + 1..];
    let mut offset = 0;
    for line in after_first.split_inclusive('\n') {
        let delimiter_candidate = line.trim_end_matches(['\r', '\n']).trim_end();
        if delimiter_candidate == "---" {
            let frontmatter = &after_first[..offset];
            let body = &after_first[offset + line.len()..];
            return Ok((
                frontmatter.trim_end_matches(['\r', '\n']).to_string(),
                body.trim_start_matches(['\r', '\n']),
            ));
        }
        offset += line.len();
    }

    Err(SkillError::Parse(
        "missing closing frontmatter delimiter".into(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use meerkat_core::skills::SourceUuid;

    fn test_key(skill: &str) -> SkillKey {
        SkillKey {
            source_uuid: SourceUuid::builtin(),
            skill_name: SkillName::parse(skill).unwrap(),
        }
    }

    #[test]
    fn test_parse_simple_skill() {
        let content = r"---
name: shell-patterns
description: Background job workflows
requires_capabilities: [builtins, shell]
---

# shell-patterns

When running background jobs...";

        let doc = parse_skill_md(
            test_key("shell-patterns"),
            SkillScope::Builtin,
            content,
            None,
        )
        .unwrap();

        assert_eq!(doc.descriptor.name, "shell-patterns");
        assert_eq!(doc.descriptor.description, "Background job workflows");
        let caps: Vec<&str> = doc
            .descriptor
            .capability_requirements
            .iter()
            .map(meerkat_core::skills::CapabilityId::as_str)
            .collect();
        assert_eq!(caps, vec!["builtins", "shell"]);
        assert!(doc.body.contains("# shell-patterns"));
    }

    #[test]
    fn test_parse_no_capabilities() {
        let content =
            "---\nname: mcp-setup\ndescription: Configure MCP servers\n---\n\nContent here";
        let doc =
            parse_skill_md(test_key("mcp-setup"), SkillScope::Project, content, None).unwrap();
        assert!(doc.descriptor.capability_requirements.is_empty());
    }

    #[test]
    fn test_parse_description_with_colon() {
        let content = "---\nname: test\ndescription: \"Use: this tool carefully\"\n---\n\nBody";
        let doc = parse_skill_md(test_key("test"), SkillScope::Builtin, content, None).unwrap();
        assert_eq!(doc.descriptor.description, "Use: this tool carefully");
    }

    #[test]
    fn test_parse_accepts_bom_and_consumes_full_closing_delimiter_line() {
        let content = "\u{feff}---\r\nname: test-skill\r\ndescription: d\r\n---   \r\nBody";
        let doc =
            parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None).unwrap();
        assert_eq!(doc.body, "Body");
    }

    #[test]
    fn test_parse_keeps_indented_delimiter_in_yaml_block_scalar() {
        let content = r"---
name: test-skill
description: |
  first line
  ---
  still description
---
Body";

        let doc =
            parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None).unwrap();

        assert_eq!(
            doc.descriptor.description,
            "first line\n---\nstill description"
        );
        assert_eq!(doc.body, "Body");
    }

    #[test]
    fn test_missing_frontmatter() {
        let content = "# No frontmatter";
        let result = parse_skill_md(test_key("test"), SkillScope::Builtin, content, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_invalid_capability_slug() {
        let content = r"---
name: test-skill
description: d
requires_capabilities: [Invalid]
---
body";
        let result = parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_unnamespaced_unknown_field() {
        let content = r"---
name: test-skill
description: d
custom_field: true
---
body";
        let result = parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_name_directory_mismatch() {
        let content = r"---
name: wrong-name
description: d
---
body";
        let result = parse_skill_md(
            test_key("correct-name"),
            SkillScope::Builtin,
            content,
            Some("correct-name"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_structured_extension_with_version_parses_into_typed_key() {
        let content = r"---
name: test-skill
description: d
vendor.version: '1'
vendor.policy:
  allow: [a, b]
  deny: [c]
---
body";
        let doc =
            parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None).unwrap();

        // Keys are typed `SkillExtensionKey` owners with namespace/key splits
        // parsed once, not re-split strings.
        let policy = doc
            .extensions
            .keys()
            .find(|k| k.key() == "policy")
            .expect("policy extension present");
        assert_eq!(policy.namespace(), "vendor");
        assert_eq!(policy.as_str(), "vendor.policy");

        // The structured value round-trips into its serialized form rather than
        // being silently dropped.
        let serialized = &doc.extensions[policy];
        assert!(serialized.contains("allow"));
        assert!(serialized.contains("deny"));
    }

    #[test]
    fn test_structured_extension_without_version_fails_closed() {
        let content = r"---
name: test-skill
description: d
vendor.policy:
  allow: [a]
---
body";
        let result = parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None);
        // Policy-bearing (structured) extension without a companion
        // `vendor.version` fails at parse rather than silently stringifying.
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_extension_key_with_empty_segment() {
        let content = r"---
name: test-skill
description: d
vendor.: value
---
body";
        let result = parse_skill_md(test_key("test-skill"), SkillScope::Builtin, content, None);
        assert!(result.is_err());
    }

    fn parse_with_frontmatter(extra: &str) -> Result<SkillDocument, SkillError> {
        let content = format!("---\nname: test-skill\ndescription: d\n{extra}---\nbody");
        parse_skill_md(test_key("test-skill"), SkillScope::Builtin, &content, None)
    }

    fn extension_value<'a>(doc: &'a SkillDocument, key: &str) -> Option<&'a str> {
        doc.extensions
            .iter()
            .find(|(candidate, _)| candidate.as_str() == key)
            .map(|(_, value)| value.as_str())
    }

    /// Unquoted scalars in string fields keep their source text: no YAML type
    /// resolution reaches them.
    #[test]
    fn test_string_fields_keep_unquoted_scalar_source_text() {
        let doc = parse_with_frontmatter(
            "version: 1.10\nmetadata:\n  float: 1.10\n  hex: 0x1F\n  word: yes\n  flag: true\n  exp: 1e3\n",
        )
        .unwrap();
        let metadata = &doc.descriptor.metadata;
        for (key, expected) in [
            ("version", "1.10"),
            ("float", "1.10"),
            ("hex", "0x1F"),
            ("word", "yes"),
            ("flag", "true"),
            ("exp", "1e3"),
        ] {
            assert_eq!(
                metadata.get(key).map(String::as_str),
                Some(expected),
                "{key}"
            );
        }
    }

    /// Extension scalars resolve booleans as YAML 1.2 does: only
    /// true/True/TRUE and false/False/FALSE, so YAML 1.1 words stay strings.
    #[test]
    fn test_extension_scalars_resolve_booleans_as_yaml_1_2() {
        let doc = parse_with_frontmatter(
            "vendor.yes: yes\nvendor.on: on\nvendor.off: off\nvendor.y: y\nvendor.n: n\nvendor.no: No\nvendor.lower: true\nvendor.title: True\nvendor.upper: TRUE\nvendor.false: false\n",
        )
        .unwrap();
        for (key, expected) in [
            ("vendor.yes", "yes"),
            ("vendor.on", "on"),
            ("vendor.off", "off"),
            ("vendor.y", "y"),
            ("vendor.n", "n"),
            ("vendor.no", "No"),
            ("vendor.lower", "true"),
            ("vendor.title", "true"),
            ("vendor.upper", "true"),
            ("vendor.false", "false"),
        ] {
            assert_eq!(extension_value(&doc, key), Some(expected), "{key}");
        }
    }

    /// A non-string extension scalar is stored as the YAML text of its
    /// resolved value, not its source text: `1.10` is the float 1.1.
    #[test]
    fn test_extension_non_string_scalars_store_resolved_value_text() {
        let doc = parse_with_frontmatter(
            "vendor.float: 1.10\nvendor.hex: 0x1F\nvendor.octal: 0o17\nvendor.exp: 1e3\nvendor.frac: .5\nvendor.plus: +12\nvendor.minus: -12\nvendor.whole: 1.0\nvendor.max: 18446744073709551615\nvendor.inf: .inf\nvendor.neginf: -.inf\nvendor.nan: .nan\nvendor.tilde: ~\nvendor.empty:\nvendor.null: null\n",
        )
        .unwrap();
        for (key, expected) in [
            ("vendor.float", "1.1"),
            ("vendor.hex", "31"),
            ("vendor.octal", "15"),
            ("vendor.exp", "1000.0"),
            ("vendor.frac", "0.5"),
            ("vendor.plus", "12"),
            ("vendor.minus", "-12"),
            ("vendor.whole", "1.0"),
            ("vendor.max", "18446744073709551615"),
            ("vendor.inf", ".inf"),
            ("vendor.neginf", "-.inf"),
            ("vendor.nan", ".nan"),
            ("vendor.tilde", "null"),
            ("vendor.empty", "null"),
            ("vendor.null", "null"),
        ] {
            assert_eq!(extension_value(&doc, key), Some(expected), "{key}");
        }
    }

    /// A structured extension value is stored as block YAML, keys in source
    /// order, aliases expanded.
    #[test]
    fn test_structured_extension_stores_block_yaml_in_source_order() {
        let doc = parse_with_frontmatter(concat!(
            "vendor.version: '1'\n",
            "vendor.policy:\n",
            "  rules:\n",
            "    - name: r1\n",
            "      paths: [a, b]\n",
            "    - name: r2\n",
            "      when: {x: 1}\n",
            "  empty_map: {}\n",
            "  empty_seq: []\n",
            "  nothing: ~\n",
            "  enabled: true\n",
            "  ratio: 1.5\n",
            "  note: |\n",
            "    line one\n",
            "    line two\n",
            "vendor.list: &shared [a, 1, true]\n",
            "vendor.alias: *shared\n",
        ))
        .unwrap();
        assert_eq!(
            extension_value(&doc, "vendor.policy"),
            Some(concat!(
                "rules:\n",
                "- name: r1\n",
                "  paths:\n",
                "  - a\n",
                "  - b\n",
                "- name: r2\n",
                "  when:\n",
                "    x: 1\n",
                "empty_map: {}\n",
                "empty_seq: []\n",
                "nothing: null\n",
                "enabled: true\n",
                "ratio: 1.5\n",
                "note: |\n",
                "  line one\n",
                "  line two",
            ))
        );
        assert_eq!(
            extension_value(&doc, "vendor.list"),
            Some("- a\n- 1\n- true")
        );
        assert_eq!(
            extension_value(&doc, "vendor.alias"),
            Some("- a\n- 1\n- true")
        );
    }

    /// Frontmatter the parser refuses outright: a custom tag, a duplicated
    /// known field or nested key, a second YAML document, and an alias
    /// expansion bomb.
    #[test]
    fn test_rejects_tags_duplicates_documents_and_alias_bombs() {
        let bomb = concat!(
            "vendor.version: '1'\n",
            "vendor.a: &a [x, x, x, x, x, x, x, x, x]\n",
            "vendor.b: &b [*a, *a, *a, *a, *a, *a, *a, *a, *a]\n",
            "vendor.c: &c [*b, *b, *b, *b, *b, *b, *b, *b, *b]\n",
            "vendor.d: &d [*c, *c, *c, *c, *c, *c, *c, *c, *c]\n",
            "vendor.e: &e [*d, *d, *d, *d, *d, *d, *d, *d, *d]\n",
            "vendor.f: &f [*e, *e, *e, *e, *e, *e, *e, *e, *e]\n",
            "vendor.g: &g [*f, *f, *f, *f, *f, *f, *f, *f, *f]\n",
            "vendor.h: &h [*g, *g, *g, *g, *g, *g, *g, *g, *g]\n",
            "vendor.i: &i [*h, *h, *h, *h, *h, *h, *h, *h, *h]\n",
        );
        for (case, extra) in [
            ("custom tag", "vendor.tagged: !custom value\n"),
            (
                "custom tag on a mapping",
                "vendor.version: '1'\nvendor.tagged: !custom {x: 1}\n",
            ),
            ("duplicate known field", "description: again\n"),
            (
                "duplicate nested key",
                "vendor.version: '1'\nvendor.policy:\n  a: 1\n  a: 2\n",
            ),
            ("second document", "--- extra: 1\n"),
            ("alias bomb", bomb),
        ] {
            assert!(parse_with_frontmatter(extra).is_err(), "{case}");
        }
    }

    /// `<<` is an ordinary key, never a merge: at the top level it reaches the
    /// extension-key parse and fails there.
    #[test]
    fn test_merge_key_is_an_ordinary_key() {
        let error = parse_with_frontmatter("<<: {vendor.merged: 1}\n").unwrap_err();
        assert!(
            error.to_string().contains("invalid extension key '<<'"),
            "{error}"
        );
    }
}
