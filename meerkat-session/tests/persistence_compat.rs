//! Tripwire for wave-c (Section 1.5 #2). Flipped green by **C-3**
//! (fixtures 1-11) + **C-6r** (fixture #12, runtime-side snapshot
//! table).
//!
//! Invariant: every v0 session/runtime-store fixture from
//! `docs/wave-c-prep/persistence-migration.md` §5 must load under the
//! v1 typed schema with lossless round-trip semantics. The canary is
//! fixture #4 (Anthropic extended-thinking `thinking: {type:"enabled",
//! budget_tokens:32000}`), which is the production shape most at risk
//! of being silently dropped by an eager typed retype.
//!
//! Predicted failure today: no v0→v1 migration code path exists.
//! Loading any fixture via `SqliteSessionStore::load` or the
//! `runtime_session_snapshots` read path fails serde with
//! `SessionError::Agent(InternalError)` (per `persistent.rs:395` /
//! `:739`). The test asserts `Ok(Some(_))` for every fixture.
//!
//! Status at c.0: marked `#[ignore]` — `meerkat-session` depends on
//! `meerkat-core` which does not compile in the half-rebuilt c.0 tree,
//! so even the `#[test]` wouldn't run without the ignore.
//! Un-ignored by C-3 (the task that adds the migration module and
//! writes the per-fixture parse), verified fixture-by-fixture.
//!
//! Fixtures 1-11 are session-blob fixtures; #12 is a runtime-store
//! snapshot row (`runtime_session_snapshots`). See
//! `docs/wave-c-prep/persistence-migration.md` §5 for the full
//! definitions.

#![allow(clippy::unwrap_used, clippy::expect_used)]

/// Inline fixture registry — kept here so helper drift in a shared
/// fixture dir cannot mask regressions. Each entry is
/// `(name, v0_json_blob)` reflecting the shapes described in
/// `persistence-migration.md` §5.
static FIXTURES: &[(&str, &str)] = &[
    // 1. session_empty_metadata: no `session_metadata` key.
    (
        "session_empty_metadata",
        r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "messages": []
        }"#,
    ),
    // 2. session_provider_params_openai: typed OpenAI params.
    (
        "session_provider_params_openai",
        r#"{
            "id": "00000000-0000-0000-0000-000000000002",
            "messages": [],
            "session_metadata": {
                "provider_params": {
                    "temperature": 0.2,
                    "reasoning": "silent",
                    "encrypted_content": "Zm9v"
                }
            }
        }"#,
    ),
    // 3. session_provider_params_anthropic_signature.
    (
        "session_provider_params_anthropic_signature",
        r#"{
            "id": "00000000-0000-0000-0000-000000000003",
            "messages": [],
            "session_metadata": {
                "provider_params": {
                    "signature": "abc123"
                }
            }
        }"#,
    ),
    // 4. session_provider_params_anthropic_thinking — CANARY.
    (
        "session_provider_params_anthropic_thinking",
        r#"{
            "id": "00000000-0000-0000-0000-000000000004",
            "messages": [],
            "session_metadata": {
                "provider_params": {
                    "thinking": {
                        "type": "enabled",
                        "budget_tokens": 32000
                    }
                }
            }
        }"#,
    ),
    // 5. session_provider_params_unknown: non-object scalar.
    (
        "session_provider_params_unknown",
        r#"{
            "id": "00000000-0000-0000-0000-000000000005",
            "messages": [],
            "session_metadata": {
                "provider_params": 42
            }
        }"#,
    ),
    // 6. session_connection_ref_slug_valid.
    (
        "session_connection_ref_slug_valid",
        r#"{
            "id": "00000000-0000-0000-0000-000000000006",
            "messages": [],
            "session_metadata": {
                "connection_ref": {
                    "realm_id": "dev",
                    "binding_id": "default_openai",
                    "profile": null
                }
            }
        }"#,
    ),
    // 7. session_connection_ref_slug_invalid: realm_id with space.
    (
        "session_connection_ref_slug_invalid",
        r#"{
            "id": "00000000-0000-0000-0000-000000000007",
            "messages": [],
            "session_metadata": {
                "connection_ref": {
                    "realm_id": "dev mode",
                    "binding_id": "default_openai",
                    "profile": null
                }
            }
        }"#,
    ),
    // 8. session_hot_swap_identity_mixed: v0 identity, v1 metadata.
    (
        "session_hot_swap_identity_mixed",
        r#"{
            "id": "00000000-0000-0000-0000-000000000008",
            "messages": [],
            "session_llm_identity": {
                "provider": "openai",
                "model": "gpt-4o-mini"
            },
            "session_metadata": {
                "connection_ref": {
                    "realm": "prod",
                    "binding": "openai_main",
                    "profile": null
                }
            }
        }"#,
    ),
    // 9. input_state_prompt_full_turn_metadata — runtime-store row.
    (
        "input_state_prompt_full_turn_metadata",
        r#"{
            "persisted_input": {
                "Prompt": {
                    "turn_metadata": {
                        "provider_params": {"temperature": 0.7},
                        "additional_instructions": ["foo", "bar"],
                        "model": "gpt-4o-mini"
                    }
                }
            }
        }"#,
    ),
    // 10. input_state_continuation_minimal: no turn_metadata.
    (
        "input_state_continuation_minimal",
        r#"{
            "persisted_input": {
                "Continuation": {}
            }
        }"#,
    ),
    // 11. input_state_provider_unknown_string.
    (
        "input_state_provider_unknown_string",
        r#"{
            "persisted_input": {
                "Prompt": {
                    "turn_metadata": {
                        "provider": "retired_backend_v0"
                    }
                }
            }
        }"#,
    ),
    // 12. runtime_session_snapshot_drift — drifting snapshot row
    // (runtime-store table, crash-recovery scenario).
    (
        "runtime_session_snapshot_drift",
        r#"{
            "session_id": "00000000-0000-0000-0000-000000000012",
            "updated_at": "2025-10-01T12:00:00Z",
            "snapshot": {
                "messages": [],
                "session_metadata": {
                    "provider_params": {
                        "thinking": {"type": "enabled", "budget_tokens": 32000}
                    }
                }
            }
        }"#,
    ),
];

#[test]
#[ignore = "un-ignore when C-3 lands the v0→v1 migration (\
            `meerkat-session/src/persistent.rs::migrations` submodule) \
            and C-6r lands the runtime snapshot migration for fixture #12"]
fn every_pre_wave_b_fixture_round_trips_losslessly() {
    // TODO(C-3 + C-6r): for each fixture:
    //   1. parse the v0 blob via the migration entry point
    //      (`meerkat_session::persistent::migrations::migrate(Value) ->
    //       Result<Session, SessionMigrationError>`).
    //   2. assert the typed result carries the expected fields —
    //      especially for fixture #4, assert the Anthropic thinking
    //      payload is preserved under
    //      `provider_tag.extension.thinking`.
    //   3. re-serialise the typed Session, re-load, assert the typed
    //      form is idempotent.
    //
    // For fixture #12, do the analogous round-trip through the
    // `runtime_session_snapshots` read path (C-6r).
    //
    // For fixture #7 (invalid slug), assert
    // `SessionMigrationError::Partial` with the legacy payload
    // retained under `legacy_connection_ref`.
    //
    // Catching assertion shape per fixture (pseudocode):
    //   for (name, blob) in FIXTURES {
    //       let migrated = migrate(serde_json::from_str(blob)?);
    //       assert!(migrated.is_ok(), "fixture {name} failed: {:?}", migrated);
    //   }
    assert!(
        !FIXTURES.is_empty(),
        "fixture registry is empty — did the inline table get deleted?"
    );
    // The real assertions land with C-3; ignored until then.
    unreachable!(
        "persistence_compat tripwire: un-ignore when C-3 migration lands"
    );
}
