The session document Meerkat persists for a deferred session that `live/open`
materialized with a staged create-time prompt (`InitialTurnPolicy::Defer` with
`DeferredPromptPolicy::Stage`), before any turn ran: the default system prompt
row and `metadata.session_deferred_turn_state` with a `pending` first turn and
the staged prompt. All text is synthetic.

Minted by the persistent session service at Core main 9556e53c78, before the
staged-prompt lowering fix. `crates/meerkat-core/src/session.rs` (the deferred-turn state
format), `crates/meerkat-session/src/ephemeral.rs` (where it is staged) and
`crates/meerkat/src/session_runtime/live_orchestration.rs` (the realtime-open
materialization) are unchanged there since v0.9.4, so this is the document a
v0.9.4 process leaves behind. Used by
`service_factory::staged_prompt_tests::a_session_persisted_with_a_staged_prompt_by_0_9_4_delivers_it_once_after_upgrade`.
