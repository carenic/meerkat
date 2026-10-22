//! The planned context of a prompt-context fork
//! ([`MemberLaunchMode::Fork`](crate::launch::MemberLaunchMode::Fork)).
//!
//! A prompt-context fork reads its source member's history rows and renders
//! them as text into the child's first prompt. What of that history may reach
//! the child is decided here, once, by the rows' types and the host's
//! [`ForkInstructionPolicy`], never by their text:
//!
//! - The source's System rows without an activation identity (its system
//!   prompt, keyed prompt versions and host system-context appends) are never
//!   carried: the child has its own role prompt.
//! - Session-acquired instruction state - instruction activation rows and the
//!   results of the built-in `load_skill` tool - is stripped under
//!   `require_explicit_activation` (the default), refuses the fork under
//!   `refuse`, and is carried under `reactivate_inherited` (activation rows
//!   as each key's effective head only).
//! - Host-injected context rows (typed `injected_context` user rows, the
//!   carrier of per-turn memory recall) are carried only under
//!   `reactivate_inherited`.
//!
//! The rendered rows fit a byte budget, newest first. The block ends with a
//! notice line that says, by name only, what the child was and was not given.

use crate::error::MobError;
use crate::ids::AgentIdentity;
use meerkat_contracts::wire::{
    WireAssistantBlock, WireContentBlock, WireContentInput, WireHistoryRow, WireSessionMessage,
    WireToolResult, WireToolResultContent,
};
use meerkat_core::skills::{LOAD_SKILL_TOOL_NAME, SkillKey, SkillName, SourceUuid};
use meerkat_core::types::TranscriptUserRole;
use meerkat_core::{
    ForkInstructionPolicy, ForkInstructionRefusal, ForkInstructionRefusalCause,
    InstructionContentDigest, InstructionKey, InstructionNamespace,
};
use std::collections::{BTreeMap, BTreeSet};

/// The most a rendered fork context may take, and the budget for a child
/// whose model the catalog does not know.
pub(crate) const MAX_FORK_CONTEXT_BYTES: usize = 256 * 1024;

const FORK_CONTEXT_END: &str = "[End of forked context]";

/// The byte budget for a child running `model`: half its context window at
/// four bytes per token when the catalog knows it, never more than
/// [`MAX_FORK_CONTEXT_BYTES`].
pub(crate) fn fork_context_budget(model: &str) -> usize {
    meerkat_models::catalog()
        .iter()
        .find(|entry| entry.id == model)
        .and_then(|entry| entry.context_window)
        .map_or(MAX_FORK_CONTEXT_BYTES, |window| {
            usize::try_from(window)
                .unwrap_or(usize::MAX)
                .saturating_mul(2)
                .min(MAX_FORK_CONTEXT_BYTES)
        })
}

/// The skill a `load_skill` call named, from its typed arguments.
#[derive(serde::Deserialize)]
struct LoadSkillArgs {
    source_uuid: String,
    skill_name: String,
}

/// One `load_skill` call: the skill its arguments named, when they parse.
struct LoadSkillCall {
    key: Option<SkillKey>,
}

impl LoadSkillCall {
    fn name(&self) -> String {
        self.key
            .as_ref()
            .map_or_else(|| "unknown skill".to_string(), ToString::to_string)
    }
}

/// The rows a prompt-context fork renders, decided under the host's fork
/// instruction policy and fitted to the budget.
///
/// [`ForkContextPlan::new`] is the only constructor, so nothing renders rows
/// the policy did not decide.
pub(super) struct ForkContextPlan {
    source_member_id: AgentIdentity,
    /// Rendered lines of the carried rows, oldest first.
    lines: Vec<String>,
    carries_session_state: bool,
    system_rows: usize,
    instruction_keys: Vec<(InstructionNamespace, InstructionKey)>,
    /// Skills loaded through `load_skill`, as the notice names them.
    skills: Vec<String>,
    injected_rows: usize,
    omitted_rows: usize,
}

impl ForkContextPlan {
    /// Decide `rows` under `policy` and fit them to `max_bytes`.
    ///
    /// Under [`ForkInstructionPolicy::Refuse`] a fork whose rows hold an
    /// activation row or a `load_skill` result is refused with the typed
    /// refusal a durable fork reports. A fork whose newest carried row alone
    /// does not fit the budget is refused with
    /// [`MobError::ForkContextTooLarge`].
    pub(super) fn new(
        source_member_id: &AgentIdentity,
        rows: Vec<WireHistoryRow>,
        policy: ForkInstructionPolicy,
        max_bytes: usize,
    ) -> Result<Self, MobError> {
        let carries_session_state = matches!(policy, ForkInstructionPolicy::ReactivateInherited);
        let load_skill_calls = load_skill_calls(&rows);

        // Each activation key's effective head: its latest row, unless that
        // row switches the key off (an empty body).
        let empty_body = InstructionContentDigest::for_body("");
        let mut heads: BTreeMap<(InstructionNamespace, InstructionKey), usize> = BTreeMap::new();
        let mut instruction_keys_seen = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            if let WireSessionMessage::System {
                instruction_activation: Some(identity),
                ..
            } = &row.0
            {
                let key = (
                    identity.revision.namespace.clone(),
                    identity.revision.key.clone(),
                );
                if !instruction_keys_seen.contains(&key) {
                    instruction_keys_seen.push(key.clone());
                }
                heads.insert(key, index);
            }
        }
        let effective_heads: BTreeSet<usize> = heads
            .values()
            .copied()
            .filter(|index| match &rows[*index].0 {
                WireSessionMessage::System {
                    instruction_activation: Some(identity),
                    ..
                } => identity.revision.content_sha256 != empty_body,
                _ => false,
            })
            .collect();
        let instruction_keys = instruction_keys_seen
            .iter()
            .filter(|key| {
                heads
                    .get(*key)
                    .is_some_and(|index| effective_heads.contains(index))
            })
            .cloned()
            .collect::<Vec<_>>();

        let mut skills = Vec::new();
        let mut skill_keys: Vec<SkillKey> = Vec::new();
        let mut system_rows = 0usize;
        let mut injected_rows = 0usize;
        let mut row_lines: Vec<Vec<String>> = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let lines = match &row.0 {
                WireSessionMessage::System {
                    instruction_activation: Some(_),
                    content,
                    ..
                } => {
                    if carries_session_state && effective_heads.contains(&index) {
                        vec![format!("[system]: {content}")]
                    } else {
                        Vec::new()
                    }
                }
                WireSessionMessage::System { .. } => {
                    system_rows += 1;
                    Vec::new()
                }
                WireSessionMessage::User {
                    content,
                    transcript_role,
                    ..
                } if *transcript_role == TranscriptUserRole::InjectedContext => {
                    injected_rows += 1;
                    if carries_session_state {
                        vec![format!("[user]: {}", content_input_text(content))]
                    } else {
                        Vec::new()
                    }
                }
                WireSessionMessage::User { content, .. } => {
                    vec![format!("[user]: {}", content_input_text(content))]
                }
                WireSessionMessage::SystemNotice { body, .. } => {
                    vec![format!(
                        "[system_notice]: {}",
                        body.clone().unwrap_or_default()
                    )]
                }
                WireSessionMessage::BlockAssistant { blocks, .. } => {
                    // Both `Text` (display) and `Transcript` (spoken) lanes
                    // project to the rendered text stream; the child sees the
                    // assistant's full visible output regardless of lane.
                    let text: String = blocks
                        .iter()
                        .filter_map(|block| match block {
                            WireAssistantBlock::Text { text, .. }
                            | WireAssistantBlock::Transcript { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![format!("[assistant]: {text}")]
                    }
                }
                WireSessionMessage::ToolResults { results, .. } => results
                    .iter()
                    .filter_map(|result| match load_skill_calls.get(&result.tool_use_id) {
                        // Every load_skill result is skill-bearing, a failed
                        // call or unreadable arguments included: fail closed.
                        Some(call) => {
                            let skill = call.name();
                            if !skills.contains(&skill) {
                                skills.push(skill.clone());
                            }
                            if let Some(key) = &call.key
                                && !skill_keys.contains(key)
                            {
                                skill_keys.push(key.clone());
                            }
                            if carries_session_state {
                                tool_result_line(result)
                            } else {
                                Some(format!(
                                    "[tool_result({})]: skill {skill} was loaded here; its \
                                     body is not carried into this fork",
                                    result.tool_use_id
                                ))
                            }
                        }
                        None => tool_result_line(result),
                    })
                    .collect(),
            };
            row_lines.push(lines);
        }

        if matches!(policy, ForkInstructionPolicy::Refuse)
            && (!instruction_keys_seen.is_empty() || !skills.is_empty())
        {
            let mut keys = instruction_keys_seen;
            keys.sort();
            skill_keys.sort();
            return Err(MobError::ForkInstructionRefused {
                source_member_id: source_member_id.clone(),
                refusal: ForkInstructionRefusal::new(
                    ForkInstructionRefusalCause::PolicyRefuses,
                    keys,
                )
                .with_skills(skill_keys),
            });
        }

        let mut plan = Self {
            source_member_id: source_member_id.clone(),
            lines: Vec::new(),
            carries_session_state,
            system_rows,
            instruction_keys,
            skills,
            injected_rows,
            omitted_rows: 0,
        };

        // Fit the rows newest first. The header, the notice (with the most an
        // omitted count can add) and the end marker are reserved up front.
        let carried_rows = row_lines.iter().filter(|lines| !lines.is_empty()).count();
        let reserved = plan.header().len()
            + plan.notice_with_omitted(carried_rows).len()
            + FORK_CONTEXT_END.len()
            + 3;
        let mut used = reserved;
        let mut kept: Vec<Vec<String>> = Vec::new();
        let mut omitted = 0usize;
        for lines in row_lines
            .into_iter()
            .rev()
            .filter(|lines| !lines.is_empty())
        {
            let bytes = lines.iter().map(|line| line.len() + 1).sum::<usize>();
            if omitted == 0 && used + bytes <= max_bytes {
                used += bytes;
                kept.push(lines);
            } else {
                if kept.is_empty() {
                    return Err(MobError::ForkContextTooLarge {
                        source_member_id: source_member_id.clone(),
                        max_bytes,
                        newest_row_bytes: reserved + bytes,
                    });
                }
                omitted += 1;
            }
        }
        kept.reverse();
        plan.lines = kept.into_iter().flatten().collect();
        plan.omitted_rows = omitted;
        Ok(plan)
    }

    fn header(&self) -> String {
        format!(
            "[Forked conversation context from member '{}']",
            self.source_member_id
        )
    }

    fn notice_with_omitted(&self, omitted_rows: usize) -> String {
        let carried = |carried: bool| if carried { "carried" } else { "not carried" };
        let mut sentences = vec![format!(
            "You are a fork of member '{}'.",
            self.source_member_id
        )];
        if self.system_rows > 0 {
            sentences.push(
                "The source's system prompt and host system context were not carried.".to_string(),
            );
        }
        if !self.instruction_keys.is_empty() {
            let keys = self
                .instruction_keys
                .iter()
                .map(|(namespace, key)| format!("{namespace}/{key}"))
                .collect::<Vec<_>>()
                .join(", ");
            sentences.push(format!(
                "Activated instructions {}: {keys}.",
                carried(self.carries_session_state)
            ));
        }
        if !self.skills.is_empty() {
            sentences.push(format!(
                "Skills loaded by the source {}: {}.",
                carried(self.carries_session_state),
                self.skills.join(", ")
            ));
        }
        if self.injected_rows > 0 {
            sentences.push(format!(
                "Host-injected context rows {}: {}.",
                carried(self.carries_session_state),
                self.injected_rows
            ));
        }
        if omitted_rows > 0 {
            sentences.push(format!("Earlier rows omitted: {omitted_rows}."));
        }
        format!("[fork_notice]: {}", sentences.join(" "))
    }

    /// Render the planned rows as the fork-context text block. ONE renderer
    /// over the ONE page row vocabulary (`WireSessionMessage`, DEC-P6E-20):
    /// the local fork branch projects its domain page through
    /// `WireMemberHistoryPageBody::try_from_history_page` and the remote
    /// branch decodes the bridge page, so local and remote fork context are
    /// byte-identical by construction.
    pub(super) fn render(&self) -> String {
        let mut lines = Vec::with_capacity(self.lines.len() + 3);
        lines.push(self.header());
        lines.extend(self.lines.iter().cloned());
        lines.push(self.notice_with_omitted(self.omitted_rows));
        lines.push(FORK_CONTEXT_END.to_string());
        lines.join("\n")
    }
}

/// The `load_skill` calls in `rows`, by tool-use id, with the skill each
/// call's typed arguments named.
fn load_skill_calls(rows: &[WireHistoryRow]) -> BTreeMap<String, LoadSkillCall> {
    let mut calls = BTreeMap::new();
    for row in rows {
        let WireSessionMessage::BlockAssistant { blocks, .. } = &row.0 else {
            continue;
        };
        for block in blocks {
            if let WireAssistantBlock::ToolUse { id, name, args, .. } = block
                && name == LOAD_SKILL_TOOL_NAME
            {
                let key = serde_json::from_str::<LoadSkillArgs>(args.get())
                    .ok()
                    .and_then(|args| {
                        Some(SkillKey::new(
                            SourceUuid::parse(&args.source_uuid).ok()?,
                            SkillName::parse(&args.skill_name).ok()?,
                        ))
                    });
                calls.insert(id.clone(), LoadSkillCall { key });
            }
        }
    }
    calls
}

fn content_input_text(content: &WireContentInput) -> String {
    match content {
        WireContentInput::Text(text) => text.clone(),
        WireContentInput::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                WireContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

/// One tool result as a preview line, or nothing for an empty result.
fn tool_result_line(result: &WireToolResult) -> Option<String> {
    let text: String = match &result.content {
        WireToolResultContent::Text(text) => text.clone(),
        WireToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                WireContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    };
    if text.is_empty() {
        return None;
    }
    let preview = if text.len() > 200 {
        // Find a valid UTF-8 char boundary at or before byte 200
        let end = text
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= 200)
            .last()
            .unwrap_or(0);
        format!("{}...", &text[..end])
    } else {
        text
    };
    Some(format!("[tool_result({})]: {preview}", result.tool_use_id))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::types::{
        InstructionActivationId, InstructionActivationIdentity, InstructionRevisionId,
        InstructionRevisionRef, SessionId, SystemNoticeKind,
    };
    use serde_json::json;

    const AT: &str = "2026-10-22T00:00:00Z";
    const SKILL_SOURCE: &str = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b";

    fn system(content: &str) -> WireHistoryRow {
        WireHistoryRow(WireSessionMessage::System {
            content: content.to_string(),
            created_at: AT.to_string(),
            identity: None,
            prompt_version: None,
            instruction_activation: None,
        })
    }

    fn activation(key: &str, activation: &str, body: &str) -> WireHistoryRow {
        let identity = InstructionActivationIdentity {
            activation_id: InstructionActivationId::new(activation).unwrap(),
            revision: InstructionRevisionRef {
                namespace: InstructionNamespace::new("host.standing").unwrap(),
                key: InstructionKey::new(key).unwrap(),
                revision_id: InstructionRevisionId::new(format!("rev-{activation}")).unwrap(),
                content_sha256: InstructionContentDigest::for_body(body),
            },
            supersedes: None,
            origin_session_id: SessionId::new(),
            render_version: 1,
        };
        WireHistoryRow(WireSessionMessage::System {
            content: format!("[meerkat-instruction-activation-v1]\nkey={key}\n\n{body}"),
            created_at: AT.to_string(),
            identity: None,
            prompt_version: None,
            instruction_activation: Some(identity),
        })
    }

    fn user_row(text: &str, transcript_role: TranscriptUserRole) -> WireHistoryRow {
        WireHistoryRow(WireSessionMessage::User {
            content: WireContentInput::Text(text.to_string()),
            transcript_role,
            interaction_id: None,
            run_id: None,
            created_at: AT.to_string(),
        })
    }

    fn user(text: &str) -> WireHistoryRow {
        user_row(text, TranscriptUserRole::Conversational)
    }

    fn assistant(blocks: Vec<WireAssistantBlock>) -> WireHistoryRow {
        WireHistoryRow(WireSessionMessage::BlockAssistant {
            blocks,
            stop_reason: None,
            interaction_id: None,
            run_id: None,
            assistant_message_id: None,
            realtime_origin: None,
            created_at: AT.to_string(),
        })
    }

    fn load_skill_call(id: &str, args: serde_json::Value) -> WireHistoryRow {
        assistant(vec![WireAssistantBlock::ToolUse {
            id: id.to_string(),
            name: LOAD_SKILL_TOOL_NAME.to_string(),
            args: serde_json::value::to_raw_value(&args).unwrap(),
            meta: None,
        }])
    }

    fn tool_result(id: &str, text: &str) -> WireHistoryRow {
        WireHistoryRow(WireSessionMessage::ToolResults {
            results: vec![WireToolResult {
                tool_use_id: id.to_string(),
                content: WireToolResultContent::Text(text.to_string()),
                is_error: false,
                settlement_failures: Vec::new(),
            }],
            created_at: AT.to_string(),
        })
    }

    fn comms_notice(body: &str) -> WireHistoryRow {
        WireHistoryRow(WireSessionMessage::SystemNotice {
            kind: SystemNoticeKind::Comms,
            runtime_origin: None,
            body: Some(body.to_string()),
            blocks: Vec::new(),
            created_at: AT.to_string(),
        })
    }

    fn plan(
        rows: Vec<WireHistoryRow>,
        policy: ForkInstructionPolicy,
    ) -> Result<ForkContextPlan, MobError> {
        ForkContextPlan::new(
            &AgentIdentity::from("source"),
            rows,
            policy,
            MAX_FORK_CONTEXT_BYTES,
        )
    }

    fn rendered(rows: Vec<WireHistoryRow>, policy: ForkInstructionPolicy) -> String {
        plan(rows, policy).expect("the fork is planned").render()
    }

    const POLICIES: [ForkInstructionPolicy; 3] = [
        ForkInstructionPolicy::RequireExplicitActivation,
        ForkInstructionPolicy::Refuse,
        ForkInstructionPolicy::ReactivateInherited,
    ];

    #[test]
    fn the_source_system_prompt_and_host_context_are_never_carried() {
        for policy in POLICIES {
            let text = rendered(
                vec![system("ROLE-PROMPT"), system("HOST-APPEND"), user("hello")],
                policy,
            );
            assert!(
                !text.contains("ROLE-PROMPT") && !text.contains("HOST-APPEND"),
                "{text}"
            );
            assert!(text.contains("[user]: hello"), "{text}");
            assert!(
                text.contains("system prompt and host system context were not carried"),
                "{text}"
            );
        }
    }

    #[test]
    fn activated_instructions_are_stripped_by_default_and_named() {
        let text = rendered(
            vec![
                system("ROLE-PROMPT"),
                activation("member", "a1", "BODY-1"),
                user("hi"),
            ],
            ForkInstructionPolicy::RequireExplicitActivation,
        );
        assert!(!text.contains("BODY-1"), "{text}");
        assert!(
            text.contains("Activated instructions not carried: host.standing/member."),
            "{text}"
        );
    }

    #[test]
    fn refuse_refuses_a_fork_whose_rows_hold_an_activation_row() {
        let refused = plan(
            vec![activation("member", "a1", "BODY-1"), user("hi")],
            ForkInstructionPolicy::Refuse,
        );
        match refused {
            Err(MobError::ForkInstructionRefused { refusal, .. }) => {
                assert_eq!(refusal.cause, ForkInstructionRefusalCause::PolicyRefuses);
                assert_eq!(refusal.keys.len(), 1);
                assert!(refusal.skills.is_empty());
            }
            other => panic!(
                "expected a typed refusal, got {:?}",
                other.map(|p| p.render())
            ),
        }
    }

    #[test]
    fn reactivate_inherited_carries_each_key_effective_head_only() {
        let text = rendered(
            vec![
                activation("member", "a1", "OLD-BODY"),
                activation("member", "a2", "NEW-BODY"),
                user("hi"),
            ],
            ForkInstructionPolicy::ReactivateInherited,
        );
        assert!(
            text.contains("NEW-BODY") && !text.contains("OLD-BODY"),
            "{text}"
        );
        assert!(
            text.contains("Activated instructions carried: host.standing/member."),
            "{text}"
        );
    }

    #[test]
    fn a_switched_off_key_carries_no_body_at_all() {
        let text = rendered(
            vec![
                activation("member", "a1", "OLD-BODY"),
                activation("member", "a2", ""),
                user("hi"),
            ],
            ForkInstructionPolicy::ReactivateInherited,
        );
        assert!(!text.contains("OLD-BODY"), "{text}");
        assert!(!text.contains("host.standing/member"), "{text}");
    }

    #[test]
    fn a_load_skill_result_follows_the_three_way_policy() {
        let rows = || {
            vec![
                load_skill_call(
                    "call-1",
                    json!({"source_uuid": SKILL_SOURCE, "skill_name": "pdf-processing"}),
                ),
                tool_result("call-1", "{\"body\":\"SKILL-BODY\"}"),
                user("hi"),
            ]
        };
        let stripped = rendered(rows(), ForkInstructionPolicy::RequireExplicitActivation);
        assert!(!stripped.contains("SKILL-BODY"), "{stripped}");
        assert!(
            stripped.contains(&format!(
                "skill {SKILL_SOURCE}/pdf-processing was loaded here"
            )) && stripped.contains(&format!(
                "Skills loaded by the source not carried: {SKILL_SOURCE}/pdf-processing."
            )),
            "{stripped}"
        );
        match plan(rows(), ForkInstructionPolicy::Refuse) {
            Err(MobError::ForkInstructionRefused { refusal, .. }) => {
                assert!(refusal.keys.is_empty());
                assert_eq!(refusal.skills.len(), 1);
                assert_eq!(
                    refusal.skills[0].to_string(),
                    format!("{SKILL_SOURCE}/pdf-processing")
                );
            }
            other => panic!(
                "expected a typed refusal, got {:?}",
                other.map(|p| p.render())
            ),
        }
        let kept = rendered(rows(), ForkInstructionPolicy::ReactivateInherited);
        assert!(kept.contains("SKILL-BODY"), "{kept}");
    }

    #[test]
    fn unreadable_load_skill_arguments_fail_closed() {
        let rows = || {
            vec![
                load_skill_call("call-1", json!({"bogus": 1})),
                tool_result("call-1", "SKILL-BODY"),
            ]
        };
        let stripped = rendered(rows(), ForkInstructionPolicy::RequireExplicitActivation);
        assert!(!stripped.contains("SKILL-BODY"), "{stripped}");
        assert!(stripped.contains("unknown skill"), "{stripped}");
        assert!(matches!(
            plan(rows(), ForkInstructionPolicy::Refuse),
            Err(MobError::ForkInstructionRefused { .. })
        ));
    }

    #[test]
    fn host_injected_context_is_carried_only_under_reactivate_inherited() {
        let rows = || {
            vec![
                user_row("RECALL-ROW", TranscriptUserRole::InjectedContext),
                user("hi"),
            ]
        };
        for policy in [
            ForkInstructionPolicy::RequireExplicitActivation,
            ForkInstructionPolicy::Refuse,
        ] {
            let text = rendered(rows(), policy);
            assert!(!text.contains("RECALL-ROW"), "{text}");
            assert!(
                text.contains("Host-injected context rows not carried: 1."),
                "{text}"
            );
        }
        let kept = rendered(rows(), ForkInstructionPolicy::ReactivateInherited);
        assert!(kept.contains("RECALL-ROW"), "{kept}");
        assert!(
            kept.contains("Host-injected context rows carried: 1."),
            "{kept}"
        );
    }

    #[test]
    fn system_notices_and_conversation_rows_are_kept() {
        let text = rendered(
            vec![
                comms_notice("PEER-SAID-HI"),
                user("hi"),
                assistant(vec![WireAssistantBlock::Text {
                    text: "ANSWER".to_string(),
                    meta: None,
                }]),
            ],
            ForkInstructionPolicy::RequireExplicitActivation,
        );
        assert!(
            text.contains("[system_notice]: PEER-SAID-HI")
                && text.contains("[user]: hi")
                && text.contains("[assistant]: ANSWER"),
            "{text}"
        );
    }

    #[test]
    fn the_budget_keeps_the_newest_rows_and_counts_the_rest() {
        let rows = (0..10)
            .map(|index| user(&format!("ROW-{index}-{}", "x".repeat(1_000))))
            .collect::<Vec<_>>();
        let plan = ForkContextPlan::new(
            &AgentIdentity::from("source"),
            rows,
            ForkInstructionPolicy::RequireExplicitActivation,
            4_000,
        )
        .expect("the newest rows fit");
        let text = plan.render();
        assert!(text.len() <= 4_000, "{} bytes", text.len());
        assert!(
            text.contains("ROW-9-") && !text.contains("ROW-0-"),
            "{text}"
        );
        assert!(text.contains("Earlier rows omitted: "), "{text}");
    }

    #[test]
    fn a_newest_row_larger_than_the_budget_is_refused_typed() {
        let refused = ForkContextPlan::new(
            &AgentIdentity::from("source"),
            vec![user(&"x".repeat(10_000))],
            ForkInstructionPolicy::RequireExplicitActivation,
            4_000,
        );
        match refused {
            Err(MobError::ForkContextTooLarge {
                max_bytes,
                newest_row_bytes,
                ..
            }) => {
                assert_eq!(max_bytes, 4_000);
                assert!(newest_row_bytes > 10_000);
            }
            other => panic!(
                "expected ForkContextTooLarge, got {:?}",
                other.map(|p| p.render())
            ),
        }
    }

    #[test]
    fn the_budget_follows_the_child_model_window() {
        assert_eq!(fork_context_budget("no-such-model"), MAX_FORK_CONTEXT_BYTES);
        for entry in meerkat_models::catalog() {
            let budget = fork_context_budget(entry.id);
            assert!(budget <= MAX_FORK_CONTEXT_BYTES, "{}", entry.id);
            if let Some(window) = entry.context_window {
                assert!(
                    budget <= usize::try_from(window).unwrap() * 2,
                    "{}",
                    entry.id
                );
            }
        }
    }
}
