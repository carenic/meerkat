//! Keyless guard for the live provider matrix.
//!
//! `live_client_provider_matrix.rs` needs provider keys and the
//! `integration-real-tests` feature, so no ordinary lane builds it. This test
//! needs neither: it reads the matrix source and fails when the matrix names a
//! provider model outside Core's catalog, so a retired id (the matrix once sent
//! `claude-3-haiku-20240307`, which the provider no longer serves) fails in the
//! ordinary test lanes instead of in a live run.

#![allow(clippy::expect_used)]

const MATRIX_SOURCE: &str = include_str!("live_client_provider_matrix.rs");

/// The quoted provider model ids `source` names.
fn named_model_ids(source: &str) -> Vec<String> {
    let mut named = Vec::new();
    for prefix in ["claude-", "gpt-", "gemini-", "dall-e-"] {
        let opening = format!("\"{prefix}");
        let mut rest = source;
        while let Some(at) = rest.find(&opening) {
            let literal = &rest[at + 1..];
            let end = literal.find('"').expect("a model id literal closes");
            let id = &literal[..end];
            if id.len() > prefix.len() {
                named.push(id.to_string());
            }
            rest = &literal[end + 1..];
        }
    }
    named
}

/// Every provider model id the matrix names is a text model or an
/// image-generation model of Core's catalog.
#[test]
fn live_matrix_names_only_catalog_models() {
    let named = named_model_ids(MATRIX_SOURCE);
    assert!(
        !named.is_empty(),
        "the scan must find the matrix's model ids"
    );
    let uncatalogued: Vec<&String> = named
        .iter()
        .filter(|id| {
            meerkat_models::infer_provider(id).is_none()
                && meerkat_models::image_generation_provider_for_model(id).is_none()
        })
        .collect();
    assert!(
        uncatalogued.is_empty(),
        "the live matrix names model ids outside Core's catalog: {uncatalogued:?}"
    );
}

/// The scan itself finds a quoted id and ignores an unquoted mention.
#[test]
fn the_scan_reads_quoted_model_ids_only() {
    let source = "let a = \"claude-3-haiku-20240307\"; // `gpt-5.2` \n let b = \"gemini-x\";";
    assert_eq!(
        named_model_ids(source),
        vec![
            "claude-3-haiku-20240307".to_string(),
            "gemini-x".to_string()
        ]
    );
}
