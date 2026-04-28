//! Diagnostic snapshot and command facade for the Mob runtime surface.
//!
//! The runtime actor owns Mob authority; this module keeps the top-level
//! command/result surface plus the durable diagnostic snapshot shapes that
//! remain useful for inspection and follow-up work.

use crate::ids::{
    AgentIdentity, AgentRuntimeId, FenceToken, FlowId, MeerkatId, RunId, WorkRef, WorkSpec,
};
use crate::roster::{Roster, RosterEntry};
use crate::run::MobRun;
#[cfg(test)]
use crate::runtime::MobLifecycleSnapshot;
use crate::runtime::MobMemberListEntry;
#[cfg(test)]
use crate::runtime::MobOrchestratorSnapshot;
use crate::tasks::MobTask;
#[cfg(target_arch = "wasm32")]
use crate::tokio;
use indexmap::IndexSet;
use meerkat_machine_derive::CommandManifest;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Public Mob mutations route through this single top-level machine command
/// surface instead of each `MobHandle` method hand-sending actor commands.
#[derive(CommandManifest)]
pub(crate) enum MobMachineCommand {
    RunFlow {
        flow_id: FlowId,
        activation_params: serde_json::Value,
        scoped_event_tx: Option<tokio::sync::mpsc::Sender<meerkat_core::ScopedAgentEvent>>,
    },
    CancelFlow {
        run_id: RunId,
    },
    FlowStatus {
        run_id: RunId,
    },
    Spawn {
        spec: Box<crate::runtime::SpawnMemberSpec>,
        owner_context: Option<crate::runtime::CanonicalOpsOwnerContext>,
    },
    /// Declarative spawn-if-absent. Constructed by
    /// `MobHandle::ensure_member` (runtime/handle.rs:2291) and matched in
    /// `MobHandle::execute_machine_command` (runtime/handle.rs:864);
    /// surfaced on the RPC `mob.ensure_member` verb
    /// (meerkat-rpc/src/handlers/mob.rs:1707).
    EnsureMember {
        spec: Box<crate::runtime::SpawnMemberSpec>,
    },
    /// Declarative drive-toward-desired roster. Constructed by
    /// `MobHandle::reconcile` (runtime/handle.rs:2317) and matched at
    /// runtime/handle.rs:868.
    Reconcile {
        desired: Vec<crate::runtime::SpawnMemberSpec>,
        options: crate::runtime::ReconcileOptions,
    },
    /// Filtered roster listing. Constructed by
    /// `MobHandle::list_members_matching` (runtime/handle.rs:2336) and
    /// matched at runtime/handle.rs:872.
    ListMembersMatching {
        filter: Box<crate::runtime::MemberFilter>,
    },
    Retire {
        agent_identity: MeerkatId,
    },
    Respawn {
        agent_identity: MeerkatId,
        initial_message: Option<meerkat_core::types::ContentInput>,
    },
    RetireAll,
    /// Submit a unit of work to a mob member. Fence-token freshness is
    /// validated in the actor; work-origin legality (External vs Internal,
    /// external-addressability, live-runtime membership, phase gates) is
    /// owned by the `MobMachine` DSL — there is no shell-side branching on
    /// `spec.origin`. Boxed: `WorkSpec` already carries `ContentInput`, and
    /// adding render/handling metadata directly in the enum would widen the
    /// `MobMachineCommand` size for every other variant (every
    /// `MobHandle::execute_machine_command` call site captures this enum in
    /// a future).
    SubmitWork(Box<SubmitWorkCommand>),
    /// Cancel a previously submitted unit of work.
    CancelWork {
        work_ref: WorkRef,
    },
    /// Cancel all in-flight work for a mob member, validated by fence token.
    CancelAllWork {
        runtime_id: AgentRuntimeId,
        fence_token: FenceToken,
    },
    Stop,
    Resume,
    Complete,
    Reset,
    Destroy,
    TaskCreate {
        subject: String,
        description: String,
        blocked_by: Vec<crate::ids::TaskId>,
    },
    TaskUpdate {
        task_id: crate::ids::TaskId,
        status: crate::tasks::TaskStatus,
        owner: Option<AgentIdentity>,
    },
    TaskList,
    TaskGet {
        task_id: crate::ids::TaskId,
    },
    McpServerStates,
    RosterSnapshot,
    ListMembers,
    ListMembersIncludingRetiring,
    ListAllMembers,
    MemberStatus {
        agent_identity: MeerkatId,
    },
    SubscribeAgentEvents {
        agent_identity: MeerkatId,
    },
    SubscribeAllAgentEvents,
    SubscribeMobEvents {
        config: crate::runtime::MobEventRouterConfig,
    },
    PollEvents {
        after_cursor: u64,
        limit: usize,
    },
    ReplayAllEvents,
    RecordOperatorActionProvenance {
        tool_name: String,
        authority_context: meerkat_core::service::MobToolAuthorityContext,
    },
    GetMember {
        agent_identity: MeerkatId,
    },
    #[cfg(test)]
    FlowTrackerCounts,
    #[cfg(test)]
    OrchestratorSnapshot,
    #[cfg(test)]
    LifecycleSnapshot,
    #[cfg(test)]
    DslT2Snapshot,
    SetSpawnPolicy {
        policy: Option<Arc<dyn crate::runtime::SpawnPolicy>>,
    },
    Shutdown,
    ForceCancel {
        agent_identity: MeerkatId,
    },
    /// Wire a local member to a peer target. D-track-b (#14) lands the
    /// producer-wiring handler that authorizes and applies this command;
    /// until then the handler returns `MobError::Internal`. Carried in
    /// the command surface so the public `MobHandle::wire` method stays
    /// on the one top-level machine-command seam.
    Wire {
        local: MeerkatId,
        target: crate::runtime::PeerTarget,
    },
    /// Unwire a local member from a peer target. Mirror of `Wire`.
    Unwire {
        local: MeerkatId,
        target: crate::runtime::PeerTarget,
    },
}

/// Payload for [`MobMachineCommand::SubmitWork`].
pub(crate) struct SubmitWorkCommand {
    pub runtime_id: AgentRuntimeId,
    pub fence_token: FenceToken,
    pub work_ref: WorkRef,
    pub spec: WorkSpec,
    pub handling_mode: meerkat_core::types::HandlingMode,
    pub render_metadata: Option<meerkat_core::types::RenderMetadata>,
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum MobMachineCommandResult {
    Unit,
    RunId(RunId),
    WorkReceipt {
        work_ref: WorkRef,
    },
    FlowStatus(Option<MobRun>),
    SpawnReceipt(crate::runtime::MemberSpawnReceipt),
    /// Result for `EnsureMember`. T4a seam.
    #[allow(dead_code)]
    EnsureMember(crate::runtime::EnsureMemberOutcome),
    /// Result for `Reconcile`. Boxed to keep the enum compact. T4a seam.
    #[allow(dead_code)]
    Reconcile(Box<crate::runtime::ReconcileReport>),
    Respawn(Result<crate::MemberRespawnReceipt, crate::MobRespawnError>),
    DestroyReport(crate::runtime::MobDestroyReport),
    TaskId(crate::ids::TaskId),
    TaskList(Vec<MobTask>),
    TaskGet(Option<MobTask>),
    McpServerStates(BTreeMap<String, bool>),
    RosterSnapshot(Roster),
    ListMembers(Vec<MobMemberListEntry>),
    ListMembersIncludingRetiring(Vec<MobMemberListEntry>),
    ListAllMembers(Vec<RosterEntry>),
    MemberStatus(crate::runtime::MobMemberSnapshot),
    #[allow(dead_code)]
    Bool(bool),
    EventStream(meerkat_core::EventStream),
    AllAgentEventStreams(Vec<(MeerkatId, meerkat_core::EventStream)>),
    MobEventRouter(crate::runtime::MobEventRouterHandle),
    MobEvents(Vec<crate::event::MobEvent>),
    GetMember(Option<RosterEntry>),
    #[cfg(test)]
    FlowTrackerCounts((usize, usize)),
    #[cfg(test)]
    OrchestratorSnapshot(MobOrchestratorSnapshot),
    #[cfg(test)]
    LifecycleSnapshot(MobLifecycleSnapshot),
    #[cfg(test)]
    DslT2Snapshot(crate::runtime::MobDslT2Snapshot),
}

#[doc(hidden)]
#[must_use]
pub fn canonical_mob_machine_command_manifest() -> IndexSet<&'static str> {
    canonical_mob_machine_command_classifications()
        .into_iter()
        .flat_map(|record| record.classification.catalog_input_names())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobMachineCommandClassification {
    CatalogInput(MobMachineCatalogInput),
    CatalogInputs(&'static [MobMachineCatalogInput]),
    ShellMechanic(MobMachineShellMechanicReason),
}

impl MobMachineCommandClassification {
    #[must_use]
    pub fn catalog_inputs(self) -> Vec<MobMachineCatalogInput> {
        match self {
            Self::CatalogInput(input) => vec![input],
            Self::CatalogInputs(inputs) => inputs.to_vec(),
            Self::ShellMechanic(_) => Vec::new(),
        }
    }

    #[must_use]
    pub fn catalog_input_names(self) -> Vec<&'static str> {
        self.catalog_inputs()
            .into_iter()
            .map(MobMachineCatalogInput::as_str)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobMachineCatalogInput {
    RunFlow,
    CancelFlow,
    FlowStatus,
    Spawn,
    EnsureMember,
    Reconcile,
    Retire,
    Respawn,
    RetireAll,
    WireMembers,
    UnwireMembers,
    WireExternalPeer,
    UnwireExternalPeer,
    SubmitWork,
    CancelWork,
    CancelAllWork,
    Stop,
    Resume,
    Complete,
    Reset,
    Destroy,
    TaskCreate,
    TaskUpdate,
    TaskList,
    TaskGet,
    McpServerStates,
    RosterSnapshot,
    ListMembers,
    ListMembersIncludingRetiring,
    ListAllMembers,
    MemberStatus,
    SubscribeAgentEvents,
    SubscribeAllAgentEvents,
    SubscribeMobEvents,
    PollEvents,
    ReplayAllEvents,
    RecordOperatorActionProvenance,
    GetMember,
    SetSpawnPolicy,
    Shutdown,
    ForceCancel,
    CreateRunSeed,
    CreateFrameSeed,
    CreateLoopSeed,
    RecordLoopBodyFrameCompleted,
    RecordLoopUntilConditionMet,
    RecordLoopUntilConditionFailed,
    AuthorizeFlowRunReducerCommand,
    AuthorizeFlowFrameReducerCommand,
    AuthorizeLoopIterationReducerCommand,
    BindMemberSession,
    RotateMemberSession,
    ReleaseMemberSession,
    SessionIngressDetachedForMobDestroy,
    SessionIngressDetachFailedForMobDestroy,
    KickoffMarkPending,
    KickoffMarkStarting,
    StartupMarkReady,
    KickoffResolveStarted,
    KickoffResolveCallbackPending,
    KickoffResolveFailed,
    KickoffResolveCancelled,
    KickoffCancelRequested,
    KickoffClear,
}

impl MobMachineCatalogInput {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RunFlow => "RunFlow",
            Self::CancelFlow => "CancelFlow",
            Self::FlowStatus => "FlowStatus",
            Self::Spawn => "Spawn",
            Self::EnsureMember => "EnsureMember",
            Self::Reconcile => "Reconcile",
            Self::Retire => "Retire",
            Self::Respawn => "Respawn",
            Self::RetireAll => "RetireAll",
            Self::WireMembers => "WireMembers",
            Self::UnwireMembers => "UnwireMembers",
            Self::WireExternalPeer => "WireExternalPeer",
            Self::UnwireExternalPeer => "UnwireExternalPeer",
            Self::SubmitWork => "SubmitWork",
            Self::CancelWork => "CancelWork",
            Self::CancelAllWork => "CancelAllWork",
            Self::Stop => "Stop",
            Self::Resume => "Resume",
            Self::Complete => "Complete",
            Self::Reset => "Reset",
            Self::Destroy => "Destroy",
            Self::TaskCreate => "TaskCreate",
            Self::TaskUpdate => "TaskUpdate",
            Self::TaskList => "TaskList",
            Self::TaskGet => "TaskGet",
            Self::McpServerStates => "McpServerStates",
            Self::RosterSnapshot => "RosterSnapshot",
            Self::ListMembers => "ListMembers",
            Self::ListMembersIncludingRetiring => "ListMembersIncludingRetiring",
            Self::ListAllMembers => "ListAllMembers",
            Self::MemberStatus => "MemberStatus",
            Self::SubscribeAgentEvents => "SubscribeAgentEvents",
            Self::SubscribeAllAgentEvents => "SubscribeAllAgentEvents",
            Self::SubscribeMobEvents => "SubscribeMobEvents",
            Self::PollEvents => "PollEvents",
            Self::ReplayAllEvents => "ReplayAllEvents",
            Self::RecordOperatorActionProvenance => "RecordOperatorActionProvenance",
            Self::GetMember => "GetMember",
            Self::SetSpawnPolicy => "SetSpawnPolicy",
            Self::Shutdown => "Shutdown",
            Self::ForceCancel => "ForceCancel",
            Self::CreateRunSeed => "CreateRunSeed",
            Self::CreateFrameSeed => "CreateFrameSeed",
            Self::CreateLoopSeed => "CreateLoopSeed",
            Self::RecordLoopBodyFrameCompleted => "RecordLoopBodyFrameCompleted",
            Self::RecordLoopUntilConditionMet => "RecordLoopUntilConditionMet",
            Self::RecordLoopUntilConditionFailed => "RecordLoopUntilConditionFailed",
            Self::AuthorizeFlowRunReducerCommand => "AuthorizeFlowRunReducerCommand",
            Self::AuthorizeFlowFrameReducerCommand => "AuthorizeFlowFrameReducerCommand",
            Self::AuthorizeLoopIterationReducerCommand => "AuthorizeLoopIterationReducerCommand",
            Self::BindMemberSession => "BindMemberSession",
            Self::RotateMemberSession => "RotateMemberSession",
            Self::ReleaseMemberSession => "ReleaseMemberSession",
            Self::SessionIngressDetachedForMobDestroy => "SessionIngressDetachedForMobDestroy",
            Self::SessionIngressDetachFailedForMobDestroy => {
                "SessionIngressDetachFailedForMobDestroy"
            }
            Self::KickoffMarkPending => "KickoffMarkPending",
            Self::KickoffMarkStarting => "KickoffMarkStarting",
            Self::StartupMarkReady => "StartupMarkReady",
            Self::KickoffResolveStarted => "KickoffResolveStarted",
            Self::KickoffResolveCallbackPending => "KickoffResolveCallbackPending",
            Self::KickoffResolveFailed => "KickoffResolveFailed",
            Self::KickoffResolveCancelled => "KickoffResolveCancelled",
            Self::KickoffCancelRequested => "KickoffCancelRequested",
            Self::KickoffClear => "KickoffClear",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobMachineShellMechanicReason {
    TestInspection,
    FilteredRosterProjection,
    ProducerWiringBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobMachineCommandClassificationRecord {
    pub command: &'static str,
    pub classification: MobMachineCommandClassification,
}

#[doc(hidden)]
#[must_use]
pub fn canonical_mob_machine_command_classifications() -> Vec<MobMachineCommandClassificationRecord>
{
    MobMachineCommand::command_variant_manifest()
        .iter()
        .copied()
        .map(|variant| MobMachineCommandClassificationRecord {
            command: variant.as_str(),
            classification: mob_machine_command_classification(variant),
        })
        .collect()
}

const fn mob_machine_command_classification(
    variant: MobMachineCommandVariant,
) -> MobMachineCommandClassification {
    match variant {
        #[cfg(test)]
        MobMachineCommandVariant::FlowTrackerCounts
        | MobMachineCommandVariant::OrchestratorSnapshot
        | MobMachineCommandVariant::LifecycleSnapshot
        | MobMachineCommandVariant::DslT2Snapshot => {
            MobMachineCommandClassification::ShellMechanic(
                MobMachineShellMechanicReason::TestInspection,
            )
        }
        MobMachineCommandVariant::ListMembersMatching => {
            MobMachineCommandClassification::ShellMechanic(
                MobMachineShellMechanicReason::FilteredRosterProjection,
            )
        }
        MobMachineCommandVariant::Wire => MobMachineCommandClassification::CatalogInputs(&[
            MobMachineCatalogInput::WireMembers,
            MobMachineCatalogInput::WireExternalPeer,
        ]),
        MobMachineCommandVariant::Unwire => MobMachineCommandClassification::CatalogInputs(&[
            MobMachineCatalogInput::UnwireMembers,
            MobMachineCatalogInput::UnwireExternalPeer,
        ]),
        MobMachineCommandVariant::RunFlow => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::RunFlow)
        }
        MobMachineCommandVariant::CancelFlow => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::CancelFlow)
        }
        MobMachineCommandVariant::FlowStatus => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::FlowStatus)
        }
        MobMachineCommandVariant::Spawn => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Spawn)
        }
        MobMachineCommandVariant::EnsureMember => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::EnsureMember)
        }
        MobMachineCommandVariant::Reconcile => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Reconcile)
        }
        MobMachineCommandVariant::Retire => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Retire)
        }
        MobMachineCommandVariant::Respawn => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Respawn)
        }
        MobMachineCommandVariant::RetireAll => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::RetireAll)
        }
        MobMachineCommandVariant::SubmitWork => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::SubmitWork)
        }
        MobMachineCommandVariant::CancelWork => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::CancelWork)
        }
        MobMachineCommandVariant::CancelAllWork => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::CancelAllWork)
        }
        MobMachineCommandVariant::Stop => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Stop)
        }
        MobMachineCommandVariant::Resume => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Resume)
        }
        MobMachineCommandVariant::Complete => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Complete)
        }
        MobMachineCommandVariant::Reset => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Reset)
        }
        MobMachineCommandVariant::Destroy => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Destroy)
        }
        MobMachineCommandVariant::TaskCreate => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::TaskCreate)
        }
        MobMachineCommandVariant::TaskUpdate => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::TaskUpdate)
        }
        MobMachineCommandVariant::TaskList => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::TaskList)
        }
        MobMachineCommandVariant::TaskGet => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::TaskGet)
        }
        MobMachineCommandVariant::McpServerStates => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::McpServerStates)
        }
        MobMachineCommandVariant::RosterSnapshot => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::RosterSnapshot)
        }
        MobMachineCommandVariant::ListMembers => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::ListMembers)
        }
        MobMachineCommandVariant::ListMembersIncludingRetiring => {
            MobMachineCommandClassification::CatalogInput(
                MobMachineCatalogInput::ListMembersIncludingRetiring,
            )
        }
        MobMachineCommandVariant::ListAllMembers => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::ListAllMembers)
        }
        MobMachineCommandVariant::MemberStatus => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::MemberStatus)
        }
        MobMachineCommandVariant::SubscribeAgentEvents => {
            MobMachineCommandClassification::CatalogInput(
                MobMachineCatalogInput::SubscribeAgentEvents,
            )
        }
        MobMachineCommandVariant::SubscribeAllAgentEvents => {
            MobMachineCommandClassification::CatalogInput(
                MobMachineCatalogInput::SubscribeAllAgentEvents,
            )
        }
        MobMachineCommandVariant::SubscribeMobEvents => {
            MobMachineCommandClassification::CatalogInput(
                MobMachineCatalogInput::SubscribeMobEvents,
            )
        }
        MobMachineCommandVariant::PollEvents => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::PollEvents)
        }
        MobMachineCommandVariant::ReplayAllEvents => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::ReplayAllEvents)
        }
        MobMachineCommandVariant::RecordOperatorActionProvenance => {
            MobMachineCommandClassification::CatalogInput(
                MobMachineCatalogInput::RecordOperatorActionProvenance,
            )
        }
        MobMachineCommandVariant::GetMember => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::GetMember)
        }
        MobMachineCommandVariant::SetSpawnPolicy => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::SetSpawnPolicy)
        }
        MobMachineCommandVariant::Shutdown => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::Shutdown)
        }
        MobMachineCommandVariant::ForceCancel => {
            MobMachineCommandClassification::CatalogInput(MobMachineCatalogInput::ForceCancel)
        }
    }
}
