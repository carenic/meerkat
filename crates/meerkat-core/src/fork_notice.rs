//! The fork notice: what a fork child is told about its source.
//!
//! A fork child is given part of its source's state: a durable fork copies
//! the source's typed rows, a prompt-context fork gets a planned text of the
//! source's history. Neither carries everything the source had. When the
//! child is created, the forking side records a typed [`ForkSeed`] on the
//! child's session; at the child's first run the agent delivers it once,
//! before the run's own input rows: the planned context as one
//! injected-context row (prompt-context forks) and a typed
//! [`SystemNoticeBlock::ForkBoundary`](crate::types::SystemNoticeBlock)
//! notice that names the source and, by name only, the instructions, skills
//! and tools the child lost or gained.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::types::{InstructionKey, InstructionNamespace, SessionId, ToolName};

/// Session metadata key of a fork child's undelivered [`ForkSeed`].
pub const SESSION_FORK_SEED_KEY: &str = "session_fork_seed_v1";

/// The most names one tool list of a fork notice holds; the rest are
/// counted. Tool lists follow the whole tool catalog, so they are bounded.
/// Instruction keys and skills are named in full: a session holds each key
/// and each loaded skill once however often it activates or loads it, so
/// those lists are as long as the host's registered keys and the skills the
/// source loaded.
pub const FORK_NOTICE_MAX_NAMES: usize = 64;

/// Which kind of fork a fork notice describes.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkNoticeKind {
    /// A durable fork: the child holds the source's rows as copied typed
    /// history.
    Transcript,
    /// A prompt-context fork: the child holds a planned text of the
    /// source's history as one injected-context row.
    PromptContext,
}

/// The source a fork notice names: the source member when a mob forked one
/// of its members, and the source session when the forking side knows it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ForkNoticeSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<crate::MobMemberBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub session_id: Option<SessionId>,
}

/// What a fork did with its source's own system prompt and host system
/// context.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkNoticeSystemPrompt {
    /// A prompt-context fork never carries them: the child has its own role
    /// prompt.
    NotCarried,
    /// A durable fork keeps its copied system prompt as its own.
    NotApplicable,
}

/// One instruction key a fork notice names.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstructionKeyRef {
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub namespace: InstructionNamespace,
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub key: InstructionKey,
}

impl InstructionKeyRef {
    pub fn new(namespace: InstructionNamespace, key: InstructionKey) -> Self {
        Self { namespace, key }
    }
}

impl std::fmt::Display for InstructionKeyRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.namespace, self.key)
    }
}

/// Why a fork notice does not compare the source's tools with the child's.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkToolsNotComparedReason {
    /// The source session was not live when the child was created, so its
    /// visible tools could not be read.
    SourceNotLive,
    /// The source runs on a member host whose visible tools were not read.
    PlacedSource,
    /// The source's member host is too old to report its visible tools.
    MemberHostUnsupported,
}

/// The tools a fork notice names: the source's visible tool names at the fork
/// boundary against the child's at its first model call, by name only.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "comparison", rename_all = "snake_case")]
pub enum ForkNoticeTools {
    Compared {
        /// Tools the source could call that the child cannot, sorted.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        removed: Vec<ToolName>,
        /// Tools the child can call that the source could not, sorted.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        added: Vec<ToolName>,
        /// Names left out of the two lists to keep the notice bounded.
        #[serde(default, skip_serializing_if = "is_zero")]
        omitted: u32,
    },
    NotCompared {
        reason: ForkToolsNotComparedReason,
    },
}

/// What a fork child was given of its source, by name only: the typed
/// content of a [`SystemNoticeBlock::ForkBoundary`](crate::types::SystemNoticeBlock)
/// notice. `summary` is the notice as the model reads it; a reader that does
/// not know the block type decodes it as an unknown notice with that summary.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ForkBoundaryNotice {
    pub fork: ForkNoticeKind,
    /// The child this notice was delivered to. A copied notice that names
    /// another session is history, not the reader's own notice.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub child_session_id: SessionId,
    pub source: ForkNoticeSource,
    pub source_system_prompt: ForkNoticeSystemPrompt,
    /// Instruction keys the source had that the child does not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructions_removed: Vec<InstructionKeyRef>,
    /// Instruction keys the child has that the source did not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructions_added: Vec<InstructionKeyRef>,
    /// Instruction keys whose bodies the child's planned context carries
    /// (a prompt-context fork under `reactivate_inherited`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructions_carried: Vec<InstructionKeyRef>,
    /// Skills the source loaded with `load_skill` whose bodies the child
    /// was not given.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_removed: Vec<crate::skills::SkillKey>,
    /// Skills the source loaded whose bodies the child's planned context
    /// carries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_carried: Vec<crate::skills::SkillKey>,
    /// Host-injected context rows of the source the child was not given.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub injected_rows_removed: u32,
    /// Host-injected context rows of the source the child's planned context
    /// carries.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub injected_rows_carried: u32,
    /// Earlier rows of the source left out of the planned context to fit
    /// its budget.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub earlier_rows_omitted: u32,
    pub tools: ForkNoticeTools,
    pub summary: String,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// The source's visible tool names at the fork boundary, or why the forking
/// side could not read them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForkSeedSourceTools {
    Read { names: Vec<ToolName> },
    NotRead { reason: ForkToolsNotComparedReason },
}

/// What a fork child is to be told at its first run, recorded on its session
/// (under [`SESSION_FORK_SEED_KEY`]) when the child is created and removed
/// when the agent delivers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ForkSeed {
    pub fork: ForkNoticeKind,
    pub source: ForkNoticeSource,
    /// The planned history a prompt-context fork child is given, delivered
    /// as one injected-context row. Absent for a durable fork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_context: Option<String>,
    /// Effective instruction keys of the source at the fork boundary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_instructions: Vec<InstructionKeyRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructions_carried: Vec<InstructionKeyRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_removed: Vec<crate::skills::SkillKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills_carried: Vec<crate::skills::SkillKey>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub injected_rows_removed: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub injected_rows_carried: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub earlier_rows_omitted: u32,
    pub source_tools: ForkSeedSourceTools,
}

impl ForkSeed {
    /// The notice for the child `child_session_id`, given its own effective
    /// instruction keys and visible tool names at its first model call.
    #[must_use]
    pub fn notice(
        &self,
        child_session_id: SessionId,
        child_instructions: &BTreeSet<InstructionKeyRef>,
        child_tools: &BTreeSet<ToolName>,
    ) -> ForkBoundaryNotice {
        let source_instructions = self
            .source_instructions
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut child_effective = child_instructions.clone();
        child_effective.extend(self.instructions_carried.iter().cloned());
        let instructions_removed = source_instructions
            .difference(&child_effective)
            .cloned()
            .collect::<Vec<_>>();
        let instructions_added = child_instructions
            .difference(&source_instructions)
            .cloned()
            .collect::<Vec<_>>();
        let tools = match &self.source_tools {
            ForkSeedSourceTools::Read { names } => {
                let source = names.iter().cloned().collect::<BTreeSet<_>>();
                let removed = source.difference(child_tools).cloned().collect::<Vec<_>>();
                let added = child_tools.difference(&source).cloned().collect::<Vec<_>>();
                let total = removed.len() + added.len();
                let removed = removed
                    .into_iter()
                    .take(FORK_NOTICE_MAX_NAMES)
                    .collect::<Vec<_>>();
                let added = added
                    .into_iter()
                    .take(FORK_NOTICE_MAX_NAMES.saturating_sub(removed.len()))
                    .collect::<Vec<_>>();
                let omitted = total.saturating_sub(removed.len() + added.len());
                ForkNoticeTools::Compared {
                    removed,
                    added,
                    omitted: u32::try_from(omitted).unwrap_or(u32::MAX),
                }
            }
            ForkSeedSourceTools::NotRead { reason } => {
                ForkNoticeTools::NotCompared { reason: *reason }
            }
        };
        let mut notice = ForkBoundaryNotice {
            fork: self.fork,
            child_session_id,
            source: self.source.clone(),
            source_system_prompt: match self.fork {
                ForkNoticeKind::PromptContext => ForkNoticeSystemPrompt::NotCarried,
                ForkNoticeKind::Transcript => ForkNoticeSystemPrompt::NotApplicable,
            },
            instructions_removed,
            instructions_added,
            instructions_carried: self.instructions_carried.clone(),
            skills_removed: self.skills_removed.clone(),
            skills_carried: self.skills_carried.clone(),
            injected_rows_removed: self.injected_rows_removed,
            injected_rows_carried: self.injected_rows_carried,
            earlier_rows_omitted: self.earlier_rows_omitted,
            tools,
            summary: String::new(),
        };
        notice.summary = notice.render_summary();
        notice
    }
}

impl ForkBoundaryNotice {
    /// The notice as the model reads it: names only.
    fn render_summary(&self) -> String {
        let source = match (&self.source.member, &self.source.session_id) {
            (Some(member), _) => format!("member '{}'", member.member),
            (None, Some(session_id)) => format!("session {session_id}"),
            (None, None) => "another agent".to_string(),
        };
        let mut lines = vec![format!("Fork notice. You are a fork of {source}.")];
        lines.push(match self.fork {
            ForkNoticeKind::PromptContext => "The context above is a planned excerpt of its \
                 conversation; its system prompt and host system context were not carried."
                .to_string(),
            ForkNoticeKind::Transcript => "The conversation above was copied from it.".to_string(),
        });
        let names = |items: &[String]| items.join(", ");
        let keys = |keys: &[InstructionKeyRef]| {
            names(&keys.iter().map(ToString::to_string).collect::<Vec<_>>())
        };
        if !self.instructions_removed.is_empty() {
            lines.push(format!(
                "Instructions the source had that you do not: {}.",
                keys(&self.instructions_removed)
            ));
        }
        if !self.instructions_added.is_empty() {
            lines.push(format!(
                "Instructions you have that the source did not: {}.",
                keys(&self.instructions_added)
            ));
        }
        if !self.instructions_carried.is_empty() {
            lines.push(format!(
                "Instructions of the source carried in the context above: {}.",
                keys(&self.instructions_carried)
            ));
        }
        let skills = |skills: &[crate::skills::SkillKey]| {
            names(&skills.iter().map(ToString::to_string).collect::<Vec<_>>())
        };
        if !self.skills_removed.is_empty() {
            lines.push(format!(
                "Skills the source loaded that you were not given: {}.",
                skills(&self.skills_removed)
            ));
        }
        if !self.skills_carried.is_empty() {
            lines.push(format!(
                "Skills the source loaded that the context above carries: {}.",
                skills(&self.skills_carried)
            ));
        }
        if self.injected_rows_removed > 0 {
            lines.push(format!(
                "Host-injected context rows of the source not carried: {}.",
                self.injected_rows_removed
            ));
        }
        if self.injected_rows_carried > 0 {
            lines.push(format!(
                "Host-injected context rows of the source carried: {}.",
                self.injected_rows_carried
            ));
        }
        if self.earlier_rows_omitted > 0 {
            lines.push(format!(
                "Earlier rows of the source left out: {}.",
                self.earlier_rows_omitted
            ));
        }
        match &self.tools {
            ForkNoticeTools::Compared {
                removed,
                added,
                omitted,
            } => {
                let tools = |tools: &[ToolName]| {
                    names(&tools.iter().map(ToString::to_string).collect::<Vec<_>>())
                };
                if !removed.is_empty() {
                    lines.push(format!(
                        "Tools the source could call that you cannot: {}.",
                        tools(removed)
                    ));
                }
                if !added.is_empty() {
                    lines.push(format!(
                        "Tools you can call that the source could not: {}.",
                        tools(added)
                    ));
                }
                if *omitted > 0 {
                    lines.push(format!("Tool names not listed: {omitted}."));
                }
            }
            ForkNoticeTools::NotCompared { .. } => {
                lines.push("Your tools were not compared with the source's.".to_string());
            }
        }
        lines.join(" ")
    }
}

/// The effective instruction keys of the activation rows in `messages` whose
/// origin session satisfies `origin`: each key's last row, unless that row
/// switched the key off.
#[must_use]
pub fn effective_instruction_keys(
    messages: &[crate::types::Message],
    origin: impl Fn(&SessionId) -> bool,
) -> BTreeSet<InstructionKeyRef> {
    let switched_off = crate::types::InstructionContentDigest::for_body("");
    let mut heads = std::collections::BTreeMap::new();
    for message in messages {
        let crate::types::Message::System(system) = message else {
            continue;
        };
        let Some(identity) = system.instruction_activation.as_ref() else {
            continue;
        };
        if !origin(&identity.origin_session_id) {
            continue;
        }
        heads.insert(
            InstructionKeyRef::new(
                identity.revision.namespace.clone(),
                identity.revision.key.clone(),
            ),
            identity.revision.content_sha256 != switched_off,
        );
    }
    heads
        .into_iter()
        .filter_map(|(key, effective)| effective.then_some(key))
        .collect()
}

/// Whether `messages` hold a fork notice delivered to `session_id`.
#[must_use]
pub fn holds_fork_notice_for(messages: &[crate::types::Message], session_id: &SessionId) -> bool {
    messages.iter().any(|message| {
        matches!(
            message,
            crate::types::Message::SystemNotice(notice)
                if notice.blocks.iter().any(|block| matches!(
                    block,
                    crate::types::SystemNoticeBlock::ForkBoundary(fork)
                        if &fork.child_session_id == session_id
                ))
        )
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::types::SystemNoticeBlock;

    fn key(name: &str) -> InstructionKeyRef {
        InstructionKeyRef::new(
            InstructionNamespace::new("host.standing").unwrap(),
            InstructionKey::new(name).unwrap(),
        )
    }

    fn seed(fork: ForkNoticeKind, source_tools: ForkSeedSourceTools) -> ForkSeed {
        ForkSeed {
            fork,
            source: ForkNoticeSource {
                member: Some(crate::MobMemberBinding {
                    mob_id: "mob".to_string(),
                    role: "lead".to_string(),
                    member: "source-member".to_string(),
                }),
                session_id: None,
            },
            planned_context: None,
            source_instructions: vec![key("kept"), key("lost")],
            instructions_carried: Vec::new(),
            skills_removed: Vec::new(),
            skills_carried: Vec::new(),
            injected_rows_removed: 2,
            injected_rows_carried: 0,
            earlier_rows_omitted: 3,
            source_tools,
        }
    }

    fn tools(names: &[&str]) -> BTreeSet<ToolName> {
        names.iter().map(|name| ToolName::from(*name)).collect()
    }

    #[test]
    fn the_notice_compares_the_childs_own_keys_and_tools_by_name() {
        let seed = seed(
            ForkNoticeKind::Transcript,
            ForkSeedSourceTools::Read {
                names: vec![ToolName::from("shell"), ToolName::from("search")],
            },
        );
        let child_keys = [key("kept"), key("new")].into_iter().collect();
        let notice = seed.notice(SessionId::new(), &child_keys, &tools(&["search", "draw"]));
        assert_eq!(notice.instructions_removed, vec![key("lost")]);
        assert_eq!(notice.instructions_added, vec![key("new")]);
        assert_eq!(
            notice.tools,
            ForkNoticeTools::Compared {
                removed: vec![ToolName::from("shell")],
                added: vec![ToolName::from("draw")],
                omitted: 0,
            }
        );
        assert_eq!(
            notice.source_system_prompt,
            ForkNoticeSystemPrompt::NotApplicable
        );
        for expected in [
            "You are a fork of member 'source-member'.",
            "Instructions the source had that you do not: host.standing/lost.",
            "Instructions you have that the source did not: host.standing/new.",
            "Host-injected context rows of the source not carried: 2.",
            "Earlier rows of the source left out: 3.",
            "Tools the source could call that you cannot: shell.",
            "Tools you can call that the source could not: draw.",
        ] {
            assert!(
                notice.summary.contains(expected),
                "{expected}: {}",
                notice.summary
            );
        }
    }

    #[test]
    fn carried_keys_are_not_removed_and_unread_tools_are_not_compared() {
        let mut seed = seed(
            ForkNoticeKind::PromptContext,
            ForkSeedSourceTools::NotRead {
                reason: ForkToolsNotComparedReason::PlacedSource,
            },
        );
        seed.instructions_carried = vec![key("lost")];
        let notice = seed.notice(SessionId::new(), &BTreeSet::new(), &BTreeSet::new());
        assert_eq!(notice.instructions_removed, vec![key("kept")]);
        assert_eq!(
            notice.tools,
            ForkNoticeTools::NotCompared {
                reason: ForkToolsNotComparedReason::PlacedSource
            }
        );
        assert_eq!(
            notice.source_system_prompt,
            ForkNoticeSystemPrompt::NotCarried
        );
        assert!(
            notice
                .summary
                .contains("system prompt and host system context were not carried")
        );
        assert!(notice.summary.contains("were not compared"));
    }

    #[test]
    fn the_tool_lists_are_bounded_and_count_what_they_leave_out() {
        let names = (0..100)
            .map(|index| ToolName::from(format!("tool-{index:03}")))
            .collect::<Vec<_>>();
        let seed = seed(
            ForkNoticeKind::Transcript,
            ForkSeedSourceTools::Read { names },
        );
        let notice = seed.notice(SessionId::new(), &BTreeSet::new(), &BTreeSet::new());
        let ForkNoticeTools::Compared {
            removed,
            added,
            omitted,
        } = notice.tools
        else {
            panic!("compared");
        };
        assert_eq!(removed.len(), FORK_NOTICE_MAX_NAMES);
        assert!(added.is_empty());
        assert_eq!(omitted, 100 - u32::try_from(FORK_NOTICE_MAX_NAMES).unwrap());
    }

    #[test]
    fn the_block_round_trips_and_an_older_reader_keeps_its_summary() {
        let notice = seed(
            ForkNoticeKind::Transcript,
            ForkSeedSourceTools::NotRead {
                reason: ForkToolsNotComparedReason::SourceNotLive,
            },
        )
        .notice(SessionId::new(), &BTreeSet::new(), &BTreeSet::new());
        let block = SystemNoticeBlock::ForkBoundary(Box::new(notice.clone()));
        let wire = serde_json::to_value(&block).unwrap();
        assert_eq!(wire["type"], "fork_boundary");
        assert_eq!(wire["summary"], notice.summary.as_str());
        let decoded: SystemNoticeBlock = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, block);
        assert_eq!(block.model_projection_text(), notice.summary);

        // A reader that does not know the block type keeps it as Unknown
        // with this summary: the shape a 0.9.4 binary decodes.
        let mut renamed = wire;
        renamed["type"] = serde_json::Value::from("fork_boundary_from_a_newer_release");
        let unknown: SystemNoticeBlock = serde_json::from_value(renamed).unwrap();
        assert_eq!(unknown.summary(), Some(notice.summary.as_str()));
    }

    /// A fork_boundary block a later release writes, with a value nested in
    /// it that this release does not know, still decodes: as an unknown
    /// block that keeps its summary, so the transcript that holds it loads.
    #[test]
    fn a_later_releases_fork_boundary_block_decodes_as_unknown_with_its_summary() {
        let notice = seed(
            ForkNoticeKind::Transcript,
            ForkSeedSourceTools::NotRead {
                reason: ForkToolsNotComparedReason::SourceNotLive,
            },
        )
        .notice(SessionId::new(), &BTreeSet::new(), &BTreeSet::new());
        let wire = serde_json::to_value(SystemNoticeBlock::ForkBoundary(Box::new(notice.clone())))
            .unwrap();
        let mut later_reason = wire.clone();
        later_reason["tools"]["reason"] = serde_json::Value::from("a_reason_from_a_later_release");
        let mut later_kind = wire;
        later_kind["fork"] = serde_json::Value::from("a_kind_from_a_later_release");
        for later in [later_reason, later_kind] {
            let decoded: SystemNoticeBlock = serde_json::from_value(later.clone())
                .unwrap_or_else(|error| panic!("{later} decodes: {error}"));
            assert!(
                !matches!(decoded, SystemNoticeBlock::ForkBoundary(_)),
                "{later}"
            );
            assert_eq!(decoded.summary(), Some(notice.summary.as_str()), "{later}");
        }
    }

    #[test]
    fn effective_keys_follow_each_keys_last_row() {
        use crate::types::{
            InstructionActivationId, InstructionActivationIdentity, InstructionContentDigest,
            InstructionRevisionId, InstructionRevisionRef, Message, SystemMessage,
        };
        let child = SessionId::new();
        let row = |name: &str, body: &str, origin: &SessionId| {
            Message::System(SystemMessage {
                content: String::new(),
                created_at: crate::types::message_timestamp_now(),
                identity: None,
                prompt_version: None,
                instruction_activation: Some(InstructionActivationIdentity {
                    activation_id: InstructionActivationId::new(format!("{name}-{body}")).unwrap(),
                    revision: InstructionRevisionRef {
                        namespace: InstructionNamespace::new("host.standing").unwrap(),
                        key: InstructionKey::new(name).unwrap(),
                        revision_id: InstructionRevisionId::new("rev").unwrap(),
                        content_sha256: InstructionContentDigest::for_body(body),
                    },
                    supersedes: None,
                    origin_session_id: origin.clone(),
                    render_version: 1,
                }),
            })
        };
        let source = SessionId::new();
        let messages = vec![
            row("inherited", "body", &source),
            row("kept", "body", &child),
            row("dropped", "body", &child),
            row("dropped", "", &child),
        ];
        let own = effective_instruction_keys(&messages, |origin| origin == &child);
        assert_eq!(own, [key("kept")].into_iter().collect());
        let inherited = effective_instruction_keys(&messages, |origin| origin != &child);
        assert_eq!(inherited, [key("inherited")].into_iter().collect());
    }
}
