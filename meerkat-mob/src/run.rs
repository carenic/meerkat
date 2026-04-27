//! Flow run data model and MobMachine-owned runtime projections.

use crate::definition::{
    DependencyMode, FlowNodeSpec, FlowSpec, FrameSpec, LimitsSpec, SupervisorSpec, TopologySpec,
};
use crate::error::MobError;
use crate::ids::{
    AgentIdentity, BranchId, FlowId, FlowNodeId, FrameId, LoopId, LoopInstanceId, MobId,
    ProfileName, RunId, StepId,
};
use crate::machines::mob_machine as mob_dsl;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub mod flow_frame;
pub mod flow_run;
pub mod loop_iteration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowProjectionKernelRole {
    MobMachineOwnedFailClosedProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowProjectionKernelAudit {
    pub module: &'static str,
    pub canonical_owner: &'static str,
    pub role: FlowProjectionKernelRole,
    pub canonical_machine: bool,
    pub owning_inputs: &'static [&'static str],
}

const FLOW_RUN_OWNING_INPUTS: &[&str] = &["CreateRunSeed", "AuthorizeFlowRunReducerCommand"];
const FLOW_FRAME_OWNING_INPUTS: &[&str] = &["CreateFrameSeed", "AuthorizeFlowFrameReducerCommand"];
const LOOP_ITERATION_OWNING_INPUTS: &[&str] = &[
    "CreateLoopSeed",
    "RecordLoopBodyFrameCompleted",
    "RecordLoopUntilConditionMet",
    "RecordLoopUntilConditionFailed",
    "AuthorizeLoopIterationReducerCommand",
];

const FLOW_PROJECTION_KERNEL_AUDIT: &[FlowProjectionKernelAudit] = &[
    FlowProjectionKernelAudit {
        module: "flow_run",
        canonical_owner: "MobMachine",
        role: FlowProjectionKernelRole::MobMachineOwnedFailClosedProjection,
        canonical_machine: false,
        owning_inputs: FLOW_RUN_OWNING_INPUTS,
    },
    FlowProjectionKernelAudit {
        module: "flow_frame",
        canonical_owner: "MobMachine",
        role: FlowProjectionKernelRole::MobMachineOwnedFailClosedProjection,
        canonical_machine: false,
        owning_inputs: FLOW_FRAME_OWNING_INPUTS,
    },
    FlowProjectionKernelAudit {
        module: "loop_iteration",
        canonical_owner: "MobMachine",
        role: FlowProjectionKernelRole::MobMachineOwnedFailClosedProjection,
        canonical_machine: false,
        owning_inputs: LOOP_ITERATION_OWNING_INPUTS,
    },
];

pub fn flow_projection_kernel_audit() -> &'static [FlowProjectionKernelAudit] {
    FLOW_PROJECTION_KERNEL_AUDIT
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MobMachineFlowAuthorityKind {
    FlowRun(mob_dsl::FlowRunReducerCommandKind),
    FlowFrame(mob_dsl::FlowFrameReducerCommandKind),
    LoopIteration(mob_dsl::LoopIterationReducerCommandKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MobMachineFlowAuthoritySource {
    MachineOwnedInput(&'static str),
    AuthorizationOnlyInput(&'static str),
    MachineOwnedSignal(&'static str),
    MachineOwnedEffect(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MobMachineFlowAuthorityToken {
    kind: MobMachineFlowAuthorityKind,
    source: MobMachineFlowAuthoritySource,
}

impl MobMachineFlowAuthorityToken {
    pub(crate) fn from_accepted_mob_machine_input(
        input: &mob_dsl::MobMachineInput,
    ) -> Result<Self, MobError> {
        match input {
            mob_dsl::MobMachineInput::CreateRunSeed { .. } => Ok(Self::new(
                MobMachineFlowAuthorityKind::FlowRun(mob_dsl::FlowRunReducerCommandKind::CreateRun),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            mob_dsl::MobMachineInput::AuthorizeFlowRunReducerCommand { command, .. } => {
                let source = match command {
                    mob_dsl::FlowRunReducerCommandKind::StartRun
                    | mob_dsl::FlowRunReducerCommandKind::DispatchStep
                    | mob_dsl::FlowRunReducerCommandKind::CancelStep
                    | mob_dsl::FlowRunReducerCommandKind::TerminalizeCompleted
                    | mob_dsl::FlowRunReducerCommandKind::TerminalizeFailed
                    | mob_dsl::FlowRunReducerCommandKind::TerminalizeCanceled => {
                        MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input))
                    }
                    _ => MobMachineFlowAuthoritySource::AuthorizationOnlyInput(input_name(input)),
                };
                Ok(Self::new(
                    MobMachineFlowAuthorityKind::FlowRun(*command),
                    source,
                ))
            }
            mob_dsl::MobMachineInput::CreateFrameSeed { frame_scope, .. } => Ok(Self::new(
                MobMachineFlowAuthorityKind::FlowFrame(match frame_scope {
                    mob_dsl::FrameScope::Root => {
                        mob_dsl::FlowFrameReducerCommandKind::StartRootFrame
                    }
                    mob_dsl::FrameScope::Body => {
                        mob_dsl::FlowFrameReducerCommandKind::StartBodyFrame
                    }
                }),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            mob_dsl::MobMachineInput::AuthorizeFlowFrameReducerCommand { command, .. } => {
                let source = match command {
                    mob_dsl::FlowFrameReducerCommandKind::SealFrame => {
                        MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input))
                    }
                    _ => MobMachineFlowAuthoritySource::AuthorizationOnlyInput(input_name(input)),
                };
                Ok(Self::new(
                    MobMachineFlowAuthorityKind::FlowFrame(*command),
                    source,
                ))
            }
            mob_dsl::MobMachineInput::CreateLoopSeed { .. } => Ok(Self::new(
                MobMachineFlowAuthorityKind::LoopIteration(
                    mob_dsl::LoopIterationReducerCommandKind::StartLoop,
                ),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            mob_dsl::MobMachineInput::RecordLoopBodyFrameCompleted { .. } => Ok(Self::new(
                MobMachineFlowAuthorityKind::LoopIteration(
                    mob_dsl::LoopIterationReducerCommandKind::BodyFrameCompleted,
                ),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            mob_dsl::MobMachineInput::RecordLoopUntilConditionMet { .. } => Ok(Self::new(
                MobMachineFlowAuthorityKind::LoopIteration(
                    mob_dsl::LoopIterationReducerCommandKind::UntilConditionMet,
                ),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            mob_dsl::MobMachineInput::RecordLoopUntilConditionFailed { .. } => Ok(Self::new(
                MobMachineFlowAuthorityKind::LoopIteration(
                    mob_dsl::LoopIterationReducerCommandKind::UntilConditionFailed,
                ),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            mob_dsl::MobMachineInput::AuthorizeLoopIterationReducerCommand { command, .. } => {
                let source = match command {
                    mob_dsl::LoopIterationReducerCommandKind::BodyFrameFailed
                    | mob_dsl::LoopIterationReducerCommandKind::BodyFrameCanceled
                    | mob_dsl::LoopIterationReducerCommandKind::CancelLoop => {
                        MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input))
                    }
                    _ => MobMachineFlowAuthoritySource::AuthorizationOnlyInput(input_name(input)),
                };
                Ok(Self::new(
                    MobMachineFlowAuthorityKind::LoopIteration(*command),
                    source,
                ))
            }
            _ => Err(MobError::Internal(format!(
                "MobMachine input {input:?} is not a flow reducer authority input"
            ))),
        }
    }

    pub(crate) fn from_accepted_mob_machine_body_frame_seed(
        input: &mob_dsl::MobMachineInput,
    ) -> Result<Self, MobError> {
        match input {
            mob_dsl::MobMachineInput::CreateFrameSeed {
                frame_scope: mob_dsl::FrameScope::Body,
                ..
            } => Ok(Self::new(
                MobMachineFlowAuthorityKind::LoopIteration(
                    mob_dsl::LoopIterationReducerCommandKind::BodyFrameStarted,
                ),
                MobMachineFlowAuthoritySource::MachineOwnedInput(input_name(input)),
            )),
            _ => Err(MobError::Internal(format!(
                "MobMachine input {input:?} is not a body-frame seed authority input"
            ))),
        }
    }

    pub(crate) fn from_accepted_mob_machine_signal(
        signal: &mob_dsl::MobMachineSignal,
    ) -> Result<Self, MobError> {
        match signal {
            mob_dsl::MobMachineSignal::StartFlow | mob_dsl::MobMachineSignal::StartRun => {
                Ok(Self::new(
                    MobMachineFlowAuthorityKind::FlowRun(
                        mob_dsl::FlowRunReducerCommandKind::StartRun,
                    ),
                    MobMachineFlowAuthoritySource::MachineOwnedSignal(signal_name(signal)),
                ))
            }
            _ => Err(MobError::Internal(format!(
                "MobMachine signal {signal:?} is not a flow reducer authority signal"
            ))),
        }
    }

    pub(crate) fn from_accepted_mob_machine_effect(
        effect: &mob_dsl::MobMachineEffect,
    ) -> Result<Self, MobError> {
        match effect {
            mob_dsl::MobMachineEffect::EmitFlowRunNotice
            | mob_dsl::MobMachineEffect::EmitRunLifecycleNotice => Ok(Self::new(
                MobMachineFlowAuthorityKind::FlowRun(
                    mob_dsl::FlowRunReducerCommandKind::TerminalizeCompleted,
                ),
                MobMachineFlowAuthoritySource::MachineOwnedEffect(effect_name(effect)),
            )),
            _ => Err(MobError::Internal(format!(
                "MobMachine effect {effect:?} is not a flow reducer authority effect"
            ))),
        }
    }

    fn new(kind: MobMachineFlowAuthorityKind, source: MobMachineFlowAuthoritySource) -> Self {
        Self { kind, source }
    }

    fn require(self, expected: MobMachineFlowAuthorityKind) -> Result<(), MobError> {
        if self.kind == expected {
            match self.source {
                MobMachineFlowAuthoritySource::MachineOwnedInput(_)
                | MobMachineFlowAuthoritySource::MachineOwnedSignal(_)
                | MobMachineFlowAuthoritySource::MachineOwnedEffect(_) => Ok(()),
                MobMachineFlowAuthoritySource::AuthorizationOnlyInput(input) => {
                    Err(MobError::Internal(format!(
                        "MobMachine input {input} only authorized {:?}; reducer-visible state \
                         changes for {:?} must be machine-owned and are fail-closed",
                        self.kind, expected
                    )))
                }
            }
        } else {
            Err(MobError::Internal(format!(
                "MobMachine flow authority token kind {:?} from {:?} cannot authorize {:?} reducer",
                self.kind, self.source, expected
            )))
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum MobMachineFlowRunCommand {
    CreateRun(flow_run::inputs::CreateRun),
    StartRun(flow_run::inputs::StartRun),
    DispatchStep(flow_run::inputs::DispatchStep),
    CompleteStep(flow_run::inputs::CompleteStep),
    RecordStepOutput(flow_run::inputs::RecordStepOutput),
    ConditionPassed(flow_run::inputs::ConditionPassed),
    ConditionRejected(flow_run::inputs::ConditionRejected),
    FailStep(flow_run::inputs::FailStep),
    SkipStep(flow_run::inputs::SkipStep),
    ProjectFrameStepStatus(flow_run::inputs::ProjectFrameStepStatus),
    CancelStep(flow_run::inputs::CancelStep),
    RegisterTargets(flow_run::inputs::RegisterTargets),
    RecordTargetSuccess(flow_run::inputs::RecordTargetSuccess),
    RecordTargetTerminalFailure(flow_run::inputs::RecordTargetTerminalFailure),
    RecordTargetCanceled(flow_run::inputs::RecordTargetCanceled),
    RecordTargetFailure(flow_run::inputs::RecordTargetFailure),
    RegisterReadyFrame(flow_run::inputs::RegisterReadyFrame),
    PumpNodeScheduler(flow_run::inputs::PumpNodeScheduler),
    RegisterPendingBodyFrame(flow_run::inputs::RegisterPendingBodyFrame),
    PumpFrameScheduler(flow_run::inputs::PumpFrameScheduler),
    NodeExecutionReleased(flow_run::inputs::NodeExecutionReleased),
    FrameTerminated(flow_run::inputs::FrameTerminated),
    TerminalizeCompleted(flow_run::inputs::TerminalizeCompleted),
    TerminalizeFailed(flow_run::inputs::TerminalizeFailed),
    TerminalizeCanceled(flow_run::inputs::TerminalizeCanceled),
}

impl MobMachineFlowRunCommand {
    pub(crate) fn authority_input(&self, run_id: &RunId) -> mob_dsl::MobMachineInput {
        mob_dsl::MobMachineInput::AuthorizeFlowRunReducerCommand {
            run_id: mob_dsl::RunId::from(run_id.to_string()),
            command: self.kind(),
            step_id: self
                .step_id()
                .map(|step_id| mob_dsl::StepId::from(step_id.as_str())),
            run_step_key: self.step_id().map(|step_id| {
                mob_dsl::RunStepKey::from(format!("{}\u{0}{}", run_id, step_id.as_str()))
            }),
            step_status: self.step_status(),
            target_count: self.target_count(),
            frame_id: self
                .frame_id()
                .map(|frame_id| mob_dsl::FrameId::from(frame_id.as_str())),
            loop_instance_id: self
                .loop_instance_id()
                .map(|loop_id| mob_dsl::LoopInstanceId::from(loop_id.as_str())),
            retry_key: self.retry_key().map(str::to_owned),
        }
    }

    pub(crate) fn kind(&self) -> mob_dsl::FlowRunReducerCommandKind {
        match self {
            Self::CreateRun(_) => mob_dsl::FlowRunReducerCommandKind::CreateRun,
            Self::StartRun(_) => mob_dsl::FlowRunReducerCommandKind::StartRun,
            Self::DispatchStep(_) => mob_dsl::FlowRunReducerCommandKind::DispatchStep,
            Self::CompleteStep(_) => mob_dsl::FlowRunReducerCommandKind::CompleteStep,
            Self::RecordStepOutput(_) => mob_dsl::FlowRunReducerCommandKind::RecordStepOutput,
            Self::ConditionPassed(_) => mob_dsl::FlowRunReducerCommandKind::ConditionPassed,
            Self::ConditionRejected(_) => mob_dsl::FlowRunReducerCommandKind::ConditionRejected,
            Self::FailStep(_) => mob_dsl::FlowRunReducerCommandKind::FailStep,
            Self::SkipStep(_) => mob_dsl::FlowRunReducerCommandKind::SkipStep,
            Self::ProjectFrameStepStatus(_) => {
                mob_dsl::FlowRunReducerCommandKind::ProjectFrameStepStatus
            }
            Self::CancelStep(_) => mob_dsl::FlowRunReducerCommandKind::CancelStep,
            Self::RegisterTargets(_) => mob_dsl::FlowRunReducerCommandKind::RegisterTargets,
            Self::RecordTargetSuccess(_) => mob_dsl::FlowRunReducerCommandKind::RecordTargetSuccess,
            Self::RecordTargetTerminalFailure(_) => {
                mob_dsl::FlowRunReducerCommandKind::RecordTargetTerminalFailure
            }
            Self::RecordTargetCanceled(_) => {
                mob_dsl::FlowRunReducerCommandKind::RecordTargetCanceled
            }
            Self::RecordTargetFailure(_) => mob_dsl::FlowRunReducerCommandKind::RecordTargetFailure,
            Self::RegisterReadyFrame(_) => mob_dsl::FlowRunReducerCommandKind::RegisterReadyFrame,
            Self::PumpNodeScheduler(_) => mob_dsl::FlowRunReducerCommandKind::PumpNodeScheduler,
            Self::RegisterPendingBodyFrame(_) => {
                mob_dsl::FlowRunReducerCommandKind::RegisterPendingBodyFrame
            }
            Self::PumpFrameScheduler(_) => mob_dsl::FlowRunReducerCommandKind::PumpFrameScheduler,
            Self::NodeExecutionReleased(_) => {
                mob_dsl::FlowRunReducerCommandKind::NodeExecutionReleased
            }
            Self::FrameTerminated(_) => mob_dsl::FlowRunReducerCommandKind::FrameTerminated,
            Self::TerminalizeCompleted(_) => {
                mob_dsl::FlowRunReducerCommandKind::TerminalizeCompleted
            }
            Self::TerminalizeFailed(_) => mob_dsl::FlowRunReducerCommandKind::TerminalizeFailed,
            Self::TerminalizeCanceled(_) => mob_dsl::FlowRunReducerCommandKind::TerminalizeCanceled,
        }
    }

    fn step_id(&self) -> Option<&StepId> {
        match self {
            Self::DispatchStep(payload) => Some(&payload.step_id),
            Self::CompleteStep(payload) => Some(&payload.step_id),
            Self::RecordStepOutput(payload) => Some(&payload.step_id),
            Self::ConditionPassed(payload) => Some(&payload.step_id),
            Self::ConditionRejected(payload) => Some(&payload.step_id),
            Self::FailStep(payload) => Some(&payload.step_id),
            Self::SkipStep(payload) => Some(&payload.step_id),
            Self::ProjectFrameStepStatus(payload) => Some(&payload.step_id),
            Self::CancelStep(payload) => Some(&payload.step_id),
            Self::RegisterTargets(payload) => Some(&payload.step_id),
            Self::RecordTargetSuccess(payload) => Some(&payload.step_id),
            Self::RecordTargetTerminalFailure(payload) => Some(&payload.step_id),
            Self::RecordTargetCanceled(payload) => Some(&payload.step_id),
            Self::RecordTargetFailure(payload) => Some(&payload.step_id),
            _ => None,
        }
    }

    fn step_status(&self) -> Option<mob_dsl::StepRunStatus> {
        let status = match self {
            Self::DispatchStep(_) => flow_run::StepRunStatus::Dispatched,
            Self::CompleteStep(_) => flow_run::StepRunStatus::Completed,
            Self::FailStep(_) => flow_run::StepRunStatus::Failed,
            Self::SkipStep(_) => flow_run::StepRunStatus::Skipped,
            Self::ProjectFrameStepStatus(payload) => payload.step_status,
            Self::CancelStep(_) => flow_run::StepRunStatus::Canceled,
            _ => return None,
        };
        Some(match status {
            flow_run::StepRunStatus::Dispatched => mob_dsl::StepRunStatus::Dispatched,
            flow_run::StepRunStatus::Completed => mob_dsl::StepRunStatus::Completed,
            flow_run::StepRunStatus::Failed => mob_dsl::StepRunStatus::Failed,
            flow_run::StepRunStatus::Skipped => mob_dsl::StepRunStatus::Skipped,
            flow_run::StepRunStatus::Canceled => mob_dsl::StepRunStatus::Canceled,
        })
    }

    fn target_count(&self) -> Option<u32> {
        match self {
            Self::RegisterTargets(payload) => Some(payload.target_count),
            _ => None,
        }
    }

    fn frame_id(&self) -> Option<&FrameId> {
        match self {
            Self::RegisterReadyFrame(payload) => Some(&payload.frame_id),
            Self::NodeExecutionReleased(payload) => Some(&payload.frame_id),
            Self::FrameTerminated(payload) => Some(&payload.frame_id),
            _ => None,
        }
    }

    fn loop_instance_id(&self) -> Option<&LoopInstanceId> {
        match self {
            Self::RegisterPendingBodyFrame(payload) => Some(&payload.loop_instance_id),
            _ => None,
        }
    }

    fn retry_key(&self) -> Option<&str> {
        match self {
            Self::RecordTargetFailure(payload) => Some(payload.retry_key.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum MobMachineFlowFrameCommand {
    StartRootFrame(flow_frame::inputs::StartRootFrame),
    StartBodyFrame(flow_frame::inputs::StartBodyFrame),
    AdmitNextReadyNode(flow_frame::inputs::AdmitNextReadyNode),
    CompleteNode(flow_frame::inputs::CompleteNode),
    RecordNodeOutput(flow_frame::inputs::RecordNodeOutput),
    FailNode(flow_frame::inputs::FailNode),
    SkipNode(flow_frame::inputs::SkipNode),
    CancelNode(flow_frame::inputs::CancelNode),
    SealFrame(flow_frame::inputs::SealFrame),
}

impl MobMachineFlowFrameCommand {
    pub(crate) fn authority_input(&self, frame_id: &FrameId) -> mob_dsl::MobMachineInput {
        mob_dsl::MobMachineInput::AuthorizeFlowFrameReducerCommand {
            frame_id: mob_dsl::FrameId::from(frame_id.as_str()),
            command: self.kind(),
            node_id: self
                .node_id()
                .map(|node_id| mob_dsl::FlowNodeId::from(node_id.as_str())),
            node_status: self.node_status(),
            ready_queue: None,
            terminal_status: self.terminal_status(),
        }
    }

    pub(crate) fn kind(&self) -> mob_dsl::FlowFrameReducerCommandKind {
        match self {
            Self::StartRootFrame(_) => mob_dsl::FlowFrameReducerCommandKind::StartRootFrame,
            Self::StartBodyFrame(_) => mob_dsl::FlowFrameReducerCommandKind::StartBodyFrame,
            Self::AdmitNextReadyNode(_) => mob_dsl::FlowFrameReducerCommandKind::AdmitNextReadyNode,
            Self::CompleteNode(_) => mob_dsl::FlowFrameReducerCommandKind::CompleteNode,
            Self::RecordNodeOutput(_) => mob_dsl::FlowFrameReducerCommandKind::RecordNodeOutput,
            Self::FailNode(_) => mob_dsl::FlowFrameReducerCommandKind::FailNode,
            Self::SkipNode(_) => mob_dsl::FlowFrameReducerCommandKind::SkipNode,
            Self::CancelNode(_) => mob_dsl::FlowFrameReducerCommandKind::CancelNode,
            Self::SealFrame(_) => mob_dsl::FlowFrameReducerCommandKind::SealFrame,
        }
    }

    fn node_id(&self) -> Option<&FlowNodeId> {
        match self {
            Self::CompleteNode(payload) => Some(&payload.node_id),
            Self::RecordNodeOutput(payload) => Some(&payload.node_id),
            Self::FailNode(payload) => Some(&payload.node_id),
            Self::SkipNode(payload) => Some(&payload.node_id),
            Self::CancelNode(payload) => Some(&payload.node_id),
            _ => None,
        }
    }

    fn node_status(&self) -> Option<mob_dsl::NodeRunStatus> {
        let status = match self {
            Self::CompleteNode(_) => flow_frame::NodeRunStatus::Completed,
            Self::FailNode(_) => flow_frame::NodeRunStatus::Failed,
            Self::SkipNode(_) => flow_frame::NodeRunStatus::Skipped,
            Self::CancelNode(_) => flow_frame::NodeRunStatus::Canceled,
            _ => return None,
        };
        Some(match status {
            flow_frame::NodeRunStatus::Pending => mob_dsl::NodeRunStatus::Pending,
            flow_frame::NodeRunStatus::Ready => mob_dsl::NodeRunStatus::Ready,
            flow_frame::NodeRunStatus::Running => mob_dsl::NodeRunStatus::Running,
            flow_frame::NodeRunStatus::Completed => mob_dsl::NodeRunStatus::Completed,
            flow_frame::NodeRunStatus::Failed => mob_dsl::NodeRunStatus::Failed,
            flow_frame::NodeRunStatus::Skipped => mob_dsl::NodeRunStatus::Skipped,
            flow_frame::NodeRunStatus::Canceled => mob_dsl::NodeRunStatus::Canceled,
        })
    }

    fn terminal_status(&self) -> Option<mob_dsl::FrameStatus> {
        match self {
            Self::SealFrame(payload) => Some(match payload.terminal_status {
                flow_frame::FrameTerminalStatus::Completed => mob_dsl::FrameStatus::Completed,
                flow_frame::FrameTerminalStatus::Failed => mob_dsl::FrameStatus::Failed,
                flow_frame::FrameTerminalStatus::Canceled => mob_dsl::FrameStatus::Canceled,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum MobMachineLoopIterationCommand {
    StartLoop(loop_iteration::inputs::StartLoop),
    BodyFrameStarted(loop_iteration::inputs::BodyFrameStarted),
    BodyFrameCompleted(loop_iteration::inputs::BodyFrameCompleted),
    BodyFrameFailed(loop_iteration::inputs::BodyFrameFailed),
    BodyFrameCanceled(loop_iteration::inputs::BodyFrameCanceled),
    UntilConditionMet(loop_iteration::inputs::UntilConditionMet),
    UntilConditionFailed(loop_iteration::inputs::UntilConditionFailed),
    CancelLoop(loop_iteration::inputs::CancelLoop),
}

impl MobMachineLoopIterationCommand {
    pub(crate) fn authority_input(
        &self,
        loop_instance_id: &LoopInstanceId,
    ) -> mob_dsl::MobMachineInput {
        let loop_instance_id = mob_dsl::LoopInstanceId::from(loop_instance_id.as_str());
        match self {
            Self::BodyFrameCompleted(payload) => {
                mob_dsl::MobMachineInput::RecordLoopBodyFrameCompleted {
                    loop_instance_id,
                    iteration: payload.iteration as u64,
                }
            }
            Self::UntilConditionMet(payload) => {
                mob_dsl::MobMachineInput::RecordLoopUntilConditionMet {
                    loop_instance_id,
                    iteration: payload.iteration as u64,
                }
            }
            Self::UntilConditionFailed(payload) => {
                mob_dsl::MobMachineInput::RecordLoopUntilConditionFailed {
                    loop_instance_id,
                    iteration: payload.iteration as u64,
                }
            }
            _ => mob_dsl::MobMachineInput::AuthorizeLoopIterationReducerCommand {
                loop_instance_id,
                command: self.kind(),
                body_frame_id: self
                    .body_frame_id()
                    .map(|frame_id| mob_dsl::FrameId::from(frame_id.as_str())),
                body_frame_iteration: self.body_frame_iteration(),
            },
        }
    }

    pub(crate) fn kind(&self) -> mob_dsl::LoopIterationReducerCommandKind {
        match self {
            Self::StartLoop(_) => mob_dsl::LoopIterationReducerCommandKind::StartLoop,
            Self::BodyFrameStarted(_) => mob_dsl::LoopIterationReducerCommandKind::BodyFrameStarted,
            Self::BodyFrameCompleted(_) => {
                mob_dsl::LoopIterationReducerCommandKind::BodyFrameCompleted
            }
            Self::BodyFrameFailed(_) => mob_dsl::LoopIterationReducerCommandKind::BodyFrameFailed,
            Self::BodyFrameCanceled(_) => {
                mob_dsl::LoopIterationReducerCommandKind::BodyFrameCanceled
            }
            Self::UntilConditionMet(_) => {
                mob_dsl::LoopIterationReducerCommandKind::UntilConditionMet
            }
            Self::UntilConditionFailed(_) => {
                mob_dsl::LoopIterationReducerCommandKind::UntilConditionFailed
            }
            Self::CancelLoop(_) => mob_dsl::LoopIterationReducerCommandKind::CancelLoop,
        }
    }

    fn body_frame_iteration(&self) -> Option<u64> {
        match self {
            Self::BodyFrameStarted(payload) => Some(payload.iteration as u64),
            Self::BodyFrameCompleted(payload) => Some(payload.iteration as u64),
            Self::BodyFrameFailed(payload) => Some(payload.iteration as u64),
            Self::BodyFrameCanceled(payload) => Some(payload.iteration as u64),
            _ => None,
        }
    }

    fn body_frame_id(&self) -> Option<&FrameId> {
        match self {
            Self::BodyFrameStarted(payload) => Some(&payload.frame_id),
            _ => None,
        }
    }
}

pub(crate) fn apply_mob_machine_flow_run_command(
    state: &flow_run::State,
    machine_state: &mob_dsl::MobMachineState,
    run_id: &RunId,
    command: MobMachineFlowRunCommand,
    authority: MobMachineFlowAuthorityToken,
) -> Result<flow_run::Outcome, MobError> {
    authority.require(MobMachineFlowAuthorityKind::FlowRun(command.kind()))?;
    match command {
        MobMachineFlowRunCommand::CreateRun(_) => {
            project_flow_run_state_from_machine(machine_state, run_id).map(|next_state| {
                flow_run::Outcome {
                    transition_id: flow_run::TransitionId::CreateRun,
                    next_state,
                    effects: vec![flow_run::Effect::EmitFlowRunNotice(
                        flow_run::effects::EmitFlowRunNotice {
                            run_status: flow_run::FlowRunStatus::Pending,
                        },
                    )],
                }
            })
        }
        MobMachineFlowRunCommand::StartRun(_) => {
            let next_state = project_flow_run_phase_from_machine(
                state,
                machine_state,
                run_id,
                flow_run::Phase::Running,
            )?;
            Ok(flow_run::Outcome {
                transition_id: flow_run::TransitionId::StartRun,
                next_state,
                effects: vec![flow_run::Effect::EmitFlowRunNotice(
                    flow_run::effects::EmitFlowRunNotice {
                        run_status: flow_run::FlowRunStatus::Running,
                    },
                )],
            })
        }
        MobMachineFlowRunCommand::DispatchStep(payload) => {
            project_flow_run_step_status_from_machine(
                state,
                machine_state,
                run_id,
                &payload.step_id,
                flow_run::StepRunStatus::Dispatched,
                flow_run::TransitionId::DispatchStep,
            )
        }
        MobMachineFlowRunCommand::CancelStep(payload) => project_flow_run_step_status_from_machine(
            state,
            machine_state,
            run_id,
            &payload.step_id,
            flow_run::StepRunStatus::Canceled,
            flow_run::TransitionId::CancelStep,
        ),
        MobMachineFlowRunCommand::TerminalizeCompleted(_) => {
            project_flow_run_terminal_from_machine(
                state,
                machine_state,
                run_id,
                flow_run::Phase::Completed,
                flow_run::FlowRunStatus::Completed,
                flow_run::TransitionId::TerminalizeCompleted,
            )
        }
        MobMachineFlowRunCommand::TerminalizeFailed(_) => project_flow_run_terminal_from_machine(
            state,
            machine_state,
            run_id,
            flow_run::Phase::Failed,
            flow_run::FlowRunStatus::Failed,
            flow_run::TransitionId::TerminalizeFailed,
        ),
        MobMachineFlowRunCommand::TerminalizeCanceled(_) => project_flow_run_terminal_from_machine(
            state,
            machine_state,
            run_id,
            flow_run::Phase::Canceled,
            flow_run::FlowRunStatus::Canceled,
            flow_run::TransitionId::TerminalizeCanceled,
        ),
        command => fail_closed_unmigrated_projection("flow_run", command.kind()),
    }
}

pub(crate) fn apply_mob_machine_flow_frame_command(
    state: &flow_frame::State,
    machine_state: &mob_dsl::MobMachineState,
    command: MobMachineFlowFrameCommand,
    authority: MobMachineFlowAuthorityToken,
) -> Result<flow_frame::Outcome, MobError> {
    authority.require(MobMachineFlowAuthorityKind::FlowFrame(command.kind()))?;
    match command {
        MobMachineFlowFrameCommand::StartRootFrame(payload) => {
            project_flow_frame_seed_from_machine(
                machine_state,
                &payload.frame_id,
                flow_frame::TransitionId::StartRootFrame,
            )
        }
        MobMachineFlowFrameCommand::StartBodyFrame(payload) => {
            project_flow_frame_seed_from_machine(
                machine_state,
                &payload.frame_id,
                flow_frame::TransitionId::StartBodyFrame,
            )
        }
        MobMachineFlowFrameCommand::SealFrame(payload) => {
            project_flow_frame_seal_from_machine(state, machine_state, payload.terminal_status)
        }
        command => fail_closed_unmigrated_projection("flow_frame", command.kind()),
    }
}

pub(crate) fn apply_mob_machine_loop_iteration_command(
    _state: &loop_iteration::State,
    machine_state: &mob_dsl::MobMachineState,
    command: MobMachineLoopIterationCommand,
    authority: MobMachineFlowAuthorityToken,
) -> Result<loop_iteration::Outcome, MobError> {
    authority.require(MobMachineFlowAuthorityKind::LoopIteration(command.kind()))?;
    match command {
        MobMachineLoopIterationCommand::StartLoop(payload) => project_loop_iteration_from_machine(
            machine_state,
            &payload.loop_instance_id,
            loop_iteration::TransitionId::StartLoop,
            vec![loop_iteration::Effect::RequestBodyFrameStart(
                loop_iteration::effects::RequestBodyFrameStart {
                    loop_instance_id: payload.loop_instance_id.clone(),
                    depth: payload.depth,
                },
            )],
        ),
        MobMachineLoopIterationCommand::BodyFrameStarted(payload) => {
            project_loop_iteration_from_machine(
                machine_state,
                &payload.loop_instance_id,
                loop_iteration::TransitionId::BodyFrameStarted,
                Vec::new(),
            )
        }
        MobMachineLoopIterationCommand::BodyFrameCompleted(payload) => {
            let projected = project_loop_iteration_state_from_machine(
                machine_state,
                &payload.loop_instance_id,
            )?;
            let effects = vec![loop_iteration::Effect::EvaluateUntilCondition(
                loop_iteration::effects::EvaluateUntilCondition {
                    loop_instance_id: payload.loop_instance_id,
                    iteration: payload.iteration,
                    parent_frame_id: projected.parent_frame_id.clone(),
                    parent_node_id: projected.parent_node_id.clone(),
                    loop_id: projected.loop_id.clone(),
                },
            )];
            Ok(loop_iteration::Outcome {
                transition_id: loop_iteration::TransitionId::BodyFrameCompleted,
                next_state: projected,
                effects,
            })
        }
        MobMachineLoopIterationCommand::UntilConditionMet(payload) => {
            let projected = project_loop_iteration_state_from_machine(
                machine_state,
                &payload.loop_instance_id,
            )?;
            let effects = vec![loop_iteration::Effect::LoopCompleted(
                loop_iteration::effects::LoopCompleted {
                    loop_instance_id: payload.loop_instance_id,
                    parent_frame_id: projected.parent_frame_id.clone(),
                    parent_node_id: projected.parent_node_id.clone(),
                },
            )];
            Ok(loop_iteration::Outcome {
                transition_id: loop_iteration::TransitionId::UntilConditionMet,
                next_state: projected,
                effects,
            })
        }
        MobMachineLoopIterationCommand::UntilConditionFailed(payload) => {
            let projected = project_loop_iteration_state_from_machine(
                machine_state,
                &payload.loop_instance_id,
            )?;
            let effects = match projected.phase {
                loop_iteration::Phase::Exhausted => vec![loop_iteration::Effect::LoopExhausted(
                    loop_iteration::effects::LoopExhausted {
                        loop_instance_id: payload.loop_instance_id,
                        parent_frame_id: projected.parent_frame_id.clone(),
                        parent_node_id: projected.parent_node_id.clone(),
                    },
                )],
                loop_iteration::Phase::Running => {
                    vec![loop_iteration::Effect::RequestBodyFrameStart(
                        loop_iteration::effects::RequestBodyFrameStart {
                            loop_instance_id: payload.loop_instance_id,
                            depth: projected.depth,
                        },
                    )]
                }
                _ => Vec::new(),
            };
            Ok(loop_iteration::Outcome {
                transition_id: loop_iteration::TransitionId::UntilConditionFailed,
                next_state: projected,
                effects,
            })
        }
        MobMachineLoopIterationCommand::BodyFrameFailed(payload) => {
            let projected = project_loop_iteration_state_from_machine(
                machine_state,
                &payload.loop_instance_id,
            )?;
            Ok(loop_iteration::Outcome {
                transition_id: loop_iteration::TransitionId::BodyFrameFailed,
                effects: vec![loop_iteration::Effect::LoopFailed(
                    loop_iteration::effects::LoopFailed {
                        loop_instance_id: payload.loop_instance_id,
                        parent_frame_id: projected.parent_frame_id.clone(),
                        parent_node_id: projected.parent_node_id.clone(),
                    },
                )],
                next_state: projected,
            })
        }
        MobMachineLoopIterationCommand::BodyFrameCanceled(payload) => {
            let projected = project_loop_iteration_state_from_machine(
                machine_state,
                &payload.loop_instance_id,
            )?;
            Ok(loop_iteration::Outcome {
                transition_id: loop_iteration::TransitionId::BodyFrameCanceled,
                effects: vec![loop_iteration::Effect::LoopCanceled(
                    loop_iteration::effects::LoopCanceled {
                        loop_instance_id: payload.loop_instance_id,
                        parent_frame_id: projected.parent_frame_id.clone(),
                        parent_node_id: projected.parent_node_id.clone(),
                    },
                )],
                next_state: projected,
            })
        }
        MobMachineLoopIterationCommand::CancelLoop(payload) => {
            let projected = project_loop_iteration_state_from_machine(
                machine_state,
                &payload.loop_instance_id,
            )?;
            Ok(loop_iteration::Outcome {
                transition_id: loop_iteration::TransitionId::CancelLoop,
                effects: vec![loop_iteration::Effect::LoopCanceled(
                    loop_iteration::effects::LoopCanceled {
                        loop_instance_id: payload.loop_instance_id,
                        parent_frame_id: projected.parent_frame_id.clone(),
                        parent_node_id: projected.parent_node_id.clone(),
                    },
                )],
                next_state: projected,
            })
        }
    }
}

pub(crate) fn project_flow_run_state_from_machine(
    machine_state: &mob_dsl::MobMachineState,
    run_id: &RunId,
) -> Result<flow_run::State, MobError> {
    let run_key = mob_dsl::RunId::from(run_id.to_string());
    let phase = match required_machine_value(&machine_state.run_status, &run_key, "run_status")? {
        mob_dsl::FlowRunStatus::Absent => flow_run::Phase::Absent,
        mob_dsl::FlowRunStatus::Pending => flow_run::Phase::Pending,
        mob_dsl::FlowRunStatus::Running => flow_run::Phase::Running,
        mob_dsl::FlowRunStatus::Completed => flow_run::Phase::Completed,
        mob_dsl::FlowRunStatus::Failed => flow_run::Phase::Failed,
        mob_dsl::FlowRunStatus::Canceled => flow_run::Phase::Canceled,
    };
    let tracked_steps = required_machine_value(
        &machine_state.run_tracked_steps,
        &run_key,
        "run_tracked_steps",
    )?
    .iter()
    .map(project_step_id)
    .collect::<BTreeSet<_>>();
    let ordered_steps = required_machine_value(
        &machine_state.run_ordered_steps,
        &run_key,
        "run_ordered_steps",
    )?
    .iter()
    .map(project_step_id)
    .collect::<Vec<_>>();

    let mut state = flow_run::initial_state();
    state.phase = phase;
    state.tracked_steps = tracked_steps;
    state.ordered_steps = ordered_steps;
    state.step_status = project_step_option_status_map(
        machine_state
            .run_step_status
            .get(&run_key)
            .cloned()
            .unwrap_or_default(),
    );
    state.output_recorded = project_step_map(
        machine_state
            .run_output_recorded
            .get(&run_key)
            .cloned()
            .unwrap_or_default(),
        |v| v,
    );
    state.step_condition_results = project_step_map(
        machine_state
            .run_step_condition_results
            .get(&run_key)
            .cloned()
            .unwrap_or_default(),
        |v| v,
    );
    state.step_has_conditions = project_step_map(
        required_machine_value(
            &machine_state.run_step_has_conditions,
            &run_key,
            "run_step_has_conditions",
        )?
        .clone(),
        |v| v,
    );
    state.step_dependencies = project_step_map(
        required_machine_value(
            &machine_state.run_step_dependencies,
            &run_key,
            "run_step_dependencies",
        )?
        .clone(),
        |deps| deps.iter().map(project_step_id).collect(),
    );
    state.step_dependency_modes = project_step_map(
        required_machine_value(
            &machine_state.run_step_dependency_modes,
            &run_key,
            "run_step_dependency_modes",
        )?
        .clone(),
        project_flow_run_dependency_mode,
    );
    state.step_branches = project_step_map(
        required_machine_value(
            &machine_state.run_step_branches,
            &run_key,
            "run_step_branches",
        )?
        .clone(),
        |branch| branch.as_ref().map(project_branch_id),
    );
    state.step_collection_policies = project_step_map(
        required_machine_value(
            &machine_state.run_step_collection_policies,
            &run_key,
            "run_step_collection_policies",
        )?
        .clone(),
        project_collection_policy,
    );
    state.step_quorum_thresholds = project_step_map(
        required_machine_value(
            &machine_state.run_step_quorum_thresholds,
            &run_key,
            "run_step_quorum_thresholds",
        )?
        .clone(),
        |v| v,
    );
    state.step_target_counts = project_step_map(
        machine_state
            .run_step_target_counts
            .get(&run_key)
            .cloned()
            .unwrap_or_default(),
        |v| v,
    );
    state.step_target_success_counts = project_step_map(
        machine_state
            .run_step_target_success_counts
            .get(&run_key)
            .cloned()
            .unwrap_or_default(),
        |v| v,
    );
    state.step_target_terminal_failure_counts = project_step_map(
        machine_state
            .run_step_target_terminal_failure_counts
            .get(&run_key)
            .cloned()
            .unwrap_or_default(),
        |v| v,
    );
    state.target_retry_counts = machine_state
        .run_target_retry_counts
        .get(&run_key)
        .cloned()
        .unwrap_or_default();
    state.escalation_threshold = *required_machine_value(
        &machine_state.run_escalation_threshold,
        &run_key,
        "run_escalation_threshold",
    )?;
    state.max_step_retries = *required_machine_value(
        &machine_state.run_max_step_retries,
        &run_key,
        "run_max_step_retries",
    )?;
    state.ready_frames = machine_state
        .run_ready_frames
        .get(&run_key)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(project_frame_id)
        .collect();
    state.ready_frame_membership = machine_state
        .run_ready_frame_membership
        .get(&run_key)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(project_frame_id)
        .collect();
    state.pending_body_frame_loops = machine_state
        .run_pending_body_frame_loops
        .get(&run_key)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(project_loop_instance_id)
        .collect();
    state.pending_body_frame_loop_membership = machine_state
        .run_pending_body_frame_loop_membership
        .get(&run_key)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(project_loop_instance_id)
        .collect();
    state.max_active_nodes = *required_machine_value(
        &machine_state.run_max_active_nodes,
        &run_key,
        "run_max_active_nodes",
    )?;
    state.max_active_frames = *required_machine_value(
        &machine_state.run_max_active_frames,
        &run_key,
        "run_max_active_frames",
    )?;
    state.max_frame_depth = *required_machine_value(
        &machine_state.run_max_frame_depth,
        &run_key,
        "run_max_frame_depth",
    )?;

    for step_id in state.tracked_steps.clone() {
        state.step_status.entry(step_id.clone()).or_insert(None);
        state
            .output_recorded
            .entry(step_id.clone())
            .or_insert(false);
        state
            .step_condition_results
            .entry(step_id.clone())
            .or_insert(None);
        state.step_dependencies.entry(step_id.clone()).or_default();
        state
            .step_dependency_modes
            .entry(step_id.clone())
            .or_insert(flow_run::DependencyMode::All);
        state.step_branches.entry(step_id.clone()).or_insert(None);
        state
            .step_collection_policies
            .entry(step_id.clone())
            .or_insert(flow_run::CollectionPolicyKind::All);
        state
            .step_quorum_thresholds
            .entry(step_id.clone())
            .or_insert(0);
        state.step_target_counts.entry(step_id.clone()).or_insert(0);
        state
            .step_target_success_counts
            .entry(step_id.clone())
            .or_insert(0);
        state
            .step_target_terminal_failure_counts
            .entry(step_id)
            .or_insert(0);
    }

    Ok(state)
}

fn project_flow_run_phase_from_machine(
    state: &flow_run::State,
    machine_state: &mob_dsl::MobMachineState,
    run_id: &RunId,
    expected_phase: flow_run::Phase,
) -> Result<flow_run::State, MobError> {
    let projected = project_flow_run_state_from_machine(machine_state, run_id)?;
    if projected.phase != expected_phase {
        return Err(MobError::Internal(format!(
            "MobMachine run '{run_id}' projected phase {:?}, expected {:?}",
            projected.phase, expected_phase
        )));
    }
    let mut next_state = state.clone();
    next_state.phase = expected_phase;
    Ok(next_state)
}

fn project_flow_run_terminal_from_machine(
    state: &flow_run::State,
    machine_state: &mob_dsl::MobMachineState,
    run_id: &RunId,
    expected_phase: flow_run::Phase,
    run_status: flow_run::FlowRunStatus,
    transition_id: flow_run::TransitionId,
) -> Result<flow_run::Outcome, MobError> {
    let next_state =
        project_flow_run_phase_from_machine(state, machine_state, run_id, expected_phase)?;
    Ok(flow_run::Outcome {
        transition_id,
        next_state,
        effects: vec![
            flow_run::Effect::EmitFlowRunNotice(flow_run::effects::EmitFlowRunNotice {
                run_status,
            }),
            flow_run::Effect::FlowTerminalized(flow_run::effects::FlowTerminalized { run_status }),
        ],
    })
}

fn project_flow_run_step_status_from_machine(
    state: &flow_run::State,
    machine_state: &mob_dsl::MobMachineState,
    run_id: &RunId,
    step_id: &StepId,
    expected_status: flow_run::StepRunStatus,
    transition_id: flow_run::TransitionId,
) -> Result<flow_run::Outcome, MobError> {
    let key = mob_dsl::RunStepKey::from(format!("{run_id}\u{0}{}", step_id.as_str()));
    let Some(status) = machine_state.run_step_status_flat.get(&key) else {
        return Err(MobError::Internal(format!(
            "MobMachine run_step_status_flat missing accepted projection for run '{run_id}' step '{step_id}'"
        )));
    };
    let projected_status = project_step_run_status(*status);
    if projected_status != expected_status {
        return Err(MobError::Internal(format!(
            "MobMachine run_step_status_flat projected {:?} for run '{run_id}' step '{step_id}', expected {:?}",
            projected_status, expected_status
        )));
    }
    let mut next_state = state.clone();
    next_state
        .step_status
        .insert(step_id.clone(), Some(projected_status));
    Ok(flow_run::Outcome {
        transition_id,
        next_state,
        effects: vec![flow_run::Effect::EmitStepNotice(
            flow_run::effects::EmitStepNotice {
                step_id: step_id.clone(),
                step_status: projected_status,
            },
        )],
    })
}

fn project_flow_frame_seed_from_machine(
    machine_state: &mob_dsl::MobMachineState,
    frame_id: &FrameId,
    transition_id: flow_frame::TransitionId,
) -> Result<flow_frame::Outcome, MobError> {
    let frame_key = mob_dsl::FrameId::from(frame_id.as_str());
    let phase = match required_machine_value(&machine_state.frame_phase, &frame_key, "frame_phase")?
    {
        mob_dsl::FrameStatus::Running => flow_frame::Phase::Running,
        mob_dsl::FrameStatus::Completed => flow_frame::Phase::Completed,
        mob_dsl::FrameStatus::Failed => flow_frame::Phase::Failed,
        mob_dsl::FrameStatus::Canceled => flow_frame::Phase::Canceled,
    };
    let frame_scope =
        match required_machine_value(&machine_state.frame_scope, &frame_key, "frame_scope")? {
            mob_dsl::FrameScope::Root => flow_frame::FrameScope::Root,
            mob_dsl::FrameScope::Body => flow_frame::FrameScope::Body,
        };
    let loop_instance_id = machine_state
        .frame_parent_loop
        .get(&frame_key)
        .and_then(Clone::clone)
        .map(|id| project_loop_instance_id(&id))
        .unwrap_or_else(|| LoopInstanceId::from(String::new()));
    let iteration = *required_machine_value(
        &machine_state.frame_iteration,
        &frame_key,
        "frame_iteration",
    )?;
    let tracked_nodes = required_machine_value(
        &machine_state.frame_tracked_nodes,
        &frame_key,
        "frame_tracked_nodes",
    )?
    .iter()
    .map(project_flow_node_id)
    .collect::<BTreeSet<_>>();
    let ordered_nodes = required_machine_value(
        &machine_state.frame_ordered_nodes,
        &frame_key,
        "frame_ordered_nodes",
    )?
    .iter()
    .map(project_flow_node_id)
    .collect::<Vec<_>>();
    let mut state = flow_frame::State {
        phase,
        frame_id: frame_id.clone(),
        frame_scope,
        loop_instance_id,
        iteration,
        last_admitted_node: FlowNodeId::from(String::new()),
        tracked_nodes,
        ordered_nodes,
        node_kind: project_node_map(
            required_machine_value(
                &machine_state.frame_node_kind,
                &frame_key,
                "frame_node_kind",
            )?
            .clone(),
            project_flow_node_kind,
        ),
        node_dependencies: project_node_map(
            required_machine_value(
                &machine_state.frame_node_dependencies,
                &frame_key,
                "frame_node_dependencies",
            )?
            .clone(),
            |deps| deps.iter().map(project_flow_node_id).collect(),
        ),
        node_dependency_modes: project_node_map(
            required_machine_value(
                &machine_state.frame_node_dependency_modes,
                &frame_key,
                "frame_node_dependency_modes",
            )?
            .clone(),
            project_flow_frame_dependency_mode,
        ),
        node_branches: project_node_map(
            required_machine_value(
                &machine_state.frame_node_branches,
                &frame_key,
                "frame_node_branches",
            )?
            .clone(),
            |branch| branch.as_ref().map(project_branch_id),
        ),
        branch_winners: BTreeSet::new(),
        node_status: project_node_map(
            machine_state
                .frame_node_status
                .get(&frame_key)
                .cloned()
                .unwrap_or_default(),
            project_node_run_status,
        ),
        ready_queue: machine_state
            .frame_ready_queue
            .get(&frame_key)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(project_flow_node_id)
            .collect(),
        output_recorded: project_node_map(
            machine_state
                .frame_output_recorded
                .get(&frame_key)
                .cloned()
                .unwrap_or_default(),
            |v| v,
        ),
        node_condition_results: project_node_map(
            machine_state
                .frame_node_condition_results
                .get(&frame_key)
                .cloned()
                .unwrap_or_default(),
            |v| v,
        ),
    };
    initialize_frame_projection_frontier(&mut state);
    Ok(flow_frame::Outcome {
        transition_id,
        next_state: state,
        effects: Vec::new(),
    })
}

fn project_flow_frame_seal_from_machine(
    state: &flow_frame::State,
    machine_state: &mob_dsl::MobMachineState,
    terminal_status: flow_frame::FrameTerminalStatus,
) -> Result<flow_frame::Outcome, MobError> {
    let frame_key = mob_dsl::FrameId::from(state.frame_id.as_str());
    let projected_phase =
        match required_machine_value(&machine_state.frame_phase, &frame_key, "frame_phase")? {
            mob_dsl::FrameStatus::Running => flow_frame::Phase::Running,
            mob_dsl::FrameStatus::Completed => flow_frame::Phase::Completed,
            mob_dsl::FrameStatus::Failed => flow_frame::Phase::Failed,
            mob_dsl::FrameStatus::Canceled => flow_frame::Phase::Canceled,
        };
    let expected_phase = match terminal_status {
        flow_frame::FrameTerminalStatus::Completed => flow_frame::Phase::Completed,
        flow_frame::FrameTerminalStatus::Failed => flow_frame::Phase::Failed,
        flow_frame::FrameTerminalStatus::Canceled => flow_frame::Phase::Canceled,
    };
    if projected_phase != expected_phase {
        return Err(MobError::Internal(format!(
            "MobMachine frame '{}' projected phase {:?}, expected {:?}",
            state.frame_id, projected_phase, expected_phase
        )));
    }

    let mut next_state = state.clone();
    next_state.phase = projected_phase;
    let effect = match (state.frame_scope, terminal_status) {
        (flow_frame::FrameScope::Root, flow_frame::FrameTerminalStatus::Completed) => {
            flow_frame::Effect::RootFrameCompleted(flow_frame::effects::RootFrameCompleted {
                frame_id: state.frame_id.clone(),
            })
        }
        (flow_frame::FrameScope::Root, flow_frame::FrameTerminalStatus::Failed) => {
            flow_frame::Effect::RootFrameFailed(flow_frame::effects::RootFrameFailed {
                frame_id: state.frame_id.clone(),
            })
        }
        (flow_frame::FrameScope::Root, flow_frame::FrameTerminalStatus::Canceled) => {
            flow_frame::Effect::RootFrameCanceled(flow_frame::effects::RootFrameCanceled {
                frame_id: state.frame_id.clone(),
            })
        }
        (flow_frame::FrameScope::Body, flow_frame::FrameTerminalStatus::Completed) => {
            flow_frame::Effect::BodyFrameCompleted(flow_frame::effects::BodyFrameCompleted {
                frame_id: state.frame_id.clone(),
                loop_instance_id: state.loop_instance_id.clone(),
                iteration: state.iteration,
            })
        }
        (flow_frame::FrameScope::Body, flow_frame::FrameTerminalStatus::Failed) => {
            flow_frame::Effect::BodyFrameFailed(flow_frame::effects::BodyFrameFailed {
                frame_id: state.frame_id.clone(),
                loop_instance_id: state.loop_instance_id.clone(),
                iteration: state.iteration,
            })
        }
        (flow_frame::FrameScope::Body, flow_frame::FrameTerminalStatus::Canceled) => {
            flow_frame::Effect::BodyFrameCanceled(flow_frame::effects::BodyFrameCanceled {
                frame_id: state.frame_id.clone(),
                loop_instance_id: state.loop_instance_id.clone(),
                iteration: state.iteration,
            })
        }
    };

    Ok(flow_frame::Outcome {
        transition_id: flow_frame::TransitionId::SealFrame,
        next_state,
        effects: vec![effect],
    })
}

fn initialize_frame_projection_frontier(state: &mut flow_frame::State) {
    for node_id in state.ordered_nodes.clone() {
        state
            .node_status
            .entry(node_id.clone())
            .or_insert(flow_frame::NodeRunStatus::Pending);
        state
            .output_recorded
            .entry(node_id.clone())
            .or_insert(false);
        state.node_condition_results.entry(node_id).or_insert(None);
    }
    state.ready_queue.clear();
    for node_id in state.ordered_nodes.clone() {
        if state.node_status.get(&node_id) != Some(&flow_frame::NodeRunStatus::Pending) {
            continue;
        }
        let deps = state
            .node_dependencies
            .get(&node_id)
            .cloned()
            .unwrap_or_default();
        let dep_mode = state
            .node_dependency_modes
            .get(&node_id)
            .copied()
            .unwrap_or(flow_frame::DependencyMode::All);
        let ready = if deps.is_empty() {
            true
        } else {
            match dep_mode {
                flow_frame::DependencyMode::All => deps.iter().all(|dep| {
                    state
                        .node_status
                        .get(dep)
                        .copied()
                        .is_some_and(|status| status == flow_frame::NodeRunStatus::Completed)
                }),
                flow_frame::DependencyMode::Any => deps.iter().any(|dep| {
                    state
                        .node_status
                        .get(dep)
                        .copied()
                        .is_some_and(|status| status == flow_frame::NodeRunStatus::Completed)
                }),
            }
        };
        if ready {
            state
                .node_status
                .insert(node_id.clone(), flow_frame::NodeRunStatus::Ready);
            state.ready_queue.push(node_id);
        }
    }
}

fn project_loop_iteration_from_machine(
    machine_state: &mob_dsl::MobMachineState,
    loop_instance_id: &LoopInstanceId,
    transition_id: loop_iteration::TransitionId,
    effects: Vec<loop_iteration::Effect>,
) -> Result<loop_iteration::Outcome, MobError> {
    Ok(loop_iteration::Outcome {
        transition_id,
        next_state: project_loop_iteration_state_from_machine(machine_state, loop_instance_id)?,
        effects,
    })
}

fn project_loop_iteration_state_from_machine(
    machine_state: &mob_dsl::MobMachineState,
    loop_instance_id: &LoopInstanceId,
) -> Result<loop_iteration::State, MobError> {
    let loop_key = mob_dsl::LoopInstanceId::from(loop_instance_id.as_str());
    let phase = match required_machine_value(&machine_state.loop_phase, &loop_key, "loop_phase")? {
        mob_dsl::LoopStatus::Running => loop_iteration::Phase::Running,
        mob_dsl::LoopStatus::Completed => loop_iteration::Phase::Completed,
        mob_dsl::LoopStatus::Exhausted => loop_iteration::Phase::Exhausted,
        mob_dsl::LoopStatus::Failed => loop_iteration::Phase::Failed,
        mob_dsl::LoopStatus::Canceled => loop_iteration::Phase::Canceled,
    };
    let stage = match required_machine_value(&machine_state.loop_stage, &loop_key, "loop_stage")? {
        mob_dsl::LoopIterationStage::AwaitingBodyFrame => {
            loop_iteration::LoopIterationStage::AwaitingBodyFrame
        }
        mob_dsl::LoopIterationStage::BodyFrameActive => {
            loop_iteration::LoopIterationStage::BodyFrameActive
        }
        mob_dsl::LoopIterationStage::AwaitingUntilEvaluation => {
            loop_iteration::LoopIterationStage::AwaitingUntilEvaluation
        }
    };
    Ok(loop_iteration::State {
        phase,
        loop_instance_id: loop_instance_id.clone(),
        parent_frame_id: project_frame_id(required_machine_value(
            &machine_state.loop_parent_frame,
            &loop_key,
            "loop_parent_frame",
        )?),
        parent_node_id: project_flow_node_id(required_machine_value(
            &machine_state.loop_parent_node,
            &loop_key,
            "loop_parent_node",
        )?),
        loop_id: project_loop_id(required_machine_value(
            &machine_state.loop_definition,
            &loop_key,
            "loop_definition",
        )?),
        depth: *required_machine_value(&machine_state.loop_depth, &loop_key, "loop_depth")?,
        stage,
        current_iteration: u32::try_from(*required_machine_value(
            &machine_state.loop_current_iteration,
            &loop_key,
            "loop_current_iteration",
        )?)
        .map_err(|_| MobError::Internal("loop current_iteration exceeds u32".to_string()))?,
        last_completed_iteration: u32::try_from(*required_machine_value(
            &machine_state.loop_last_completed_iteration,
            &loop_key,
            "loop_last_completed_iteration",
        )?)
        .map_err(|_| MobError::Internal("loop last_completed_iteration exceeds u32".to_string()))?,
        max_iterations: u32::try_from(*required_machine_value(
            &machine_state.loop_max_iterations,
            &loop_key,
            "loop_max_iterations",
        )?)
        .map_err(|_| MobError::Internal("loop max_iterations exceeds u32".to_string()))?,
        active_body_frame_id: machine_state
            .loop_active_body_frame
            .get(&loop_key)
            .cloned()
            .flatten()
            .map(|frame_id| project_frame_id(&frame_id)),
    })
}

fn required_machine_value<'a, K, V>(
    map: &'a BTreeMap<K, V>,
    key: &K,
    field: &'static str,
) -> Result<&'a V, MobError>
where
    K: Ord + std::fmt::Debug,
{
    map.get(key).ok_or_else(|| {
        MobError::Internal(format!(
            "MobMachine projection field {field} missing key {key:?}"
        ))
    })
}

fn project_step_map<T, U>(
    input: BTreeMap<mob_dsl::StepId, T>,
    mut f: impl FnMut(T) -> U,
) -> BTreeMap<StepId, U> {
    input
        .into_iter()
        .map(|(step_id, value)| (project_step_id(&step_id), f(value)))
        .collect()
}

fn project_node_map<T, U>(
    input: BTreeMap<mob_dsl::FlowNodeId, T>,
    mut f: impl FnMut(T) -> U,
) -> BTreeMap<FlowNodeId, U> {
    input
        .into_iter()
        .map(|(node_id, value)| (project_flow_node_id(&node_id), f(value)))
        .collect()
}

fn project_step_option_status_map(
    input: BTreeMap<mob_dsl::StepId, Option<mob_dsl::StepRunStatus>>,
) -> BTreeMap<StepId, Option<flow_run::StepRunStatus>> {
    project_step_map(input, |status| status.map(project_step_run_status))
}

fn project_step_id(step_id: &mob_dsl::StepId) -> StepId {
    StepId::from(step_id.as_str())
}

fn project_frame_id(frame_id: &mob_dsl::FrameId) -> FrameId {
    FrameId::from(frame_id.as_str())
}

fn project_loop_instance_id(loop_instance_id: &mob_dsl::LoopInstanceId) -> LoopInstanceId {
    LoopInstanceId::from(loop_instance_id.as_str())
}

fn project_loop_id(loop_id: &mob_dsl::LoopId) -> LoopId {
    LoopId::from(loop_id.0.as_str())
}

fn project_flow_node_id(node_id: &mob_dsl::FlowNodeId) -> FlowNodeId {
    FlowNodeId::from(node_id.0.as_str())
}

fn project_branch_id(branch_id: &mob_dsl::BranchId) -> BranchId {
    BranchId::from(branch_id.0.as_str())
}

fn project_flow_run_dependency_mode(mode: mob_dsl::DependencyMode) -> flow_run::DependencyMode {
    match mode {
        mob_dsl::DependencyMode::All => flow_run::DependencyMode::All,
        mob_dsl::DependencyMode::Any => flow_run::DependencyMode::Any,
    }
}

fn project_flow_frame_dependency_mode(mode: mob_dsl::DependencyMode) -> flow_frame::DependencyMode {
    match mode {
        mob_dsl::DependencyMode::All => flow_frame::DependencyMode::All,
        mob_dsl::DependencyMode::Any => flow_frame::DependencyMode::Any,
    }
}

fn project_collection_policy(
    policy: mob_dsl::CollectionPolicyKind,
) -> flow_run::CollectionPolicyKind {
    match policy {
        mob_dsl::CollectionPolicyKind::All => flow_run::CollectionPolicyKind::All,
        mob_dsl::CollectionPolicyKind::Any => flow_run::CollectionPolicyKind::Any,
        mob_dsl::CollectionPolicyKind::Quorum => flow_run::CollectionPolicyKind::Quorum,
    }
}

fn project_step_run_status(status: mob_dsl::StepRunStatus) -> flow_run::StepRunStatus {
    match status {
        mob_dsl::StepRunStatus::Dispatched => flow_run::StepRunStatus::Dispatched,
        mob_dsl::StepRunStatus::Completed => flow_run::StepRunStatus::Completed,
        mob_dsl::StepRunStatus::Failed => flow_run::StepRunStatus::Failed,
        mob_dsl::StepRunStatus::Skipped => flow_run::StepRunStatus::Skipped,
        mob_dsl::StepRunStatus::Canceled => flow_run::StepRunStatus::Canceled,
    }
}

fn project_flow_node_kind(kind: mob_dsl::FlowNodeKind) -> flow_frame::FlowNodeKind {
    match kind {
        mob_dsl::FlowNodeKind::Step => flow_frame::FlowNodeKind::Step,
        mob_dsl::FlowNodeKind::Loop => flow_frame::FlowNodeKind::Loop,
    }
}

fn project_node_run_status(status: mob_dsl::NodeRunStatus) -> flow_frame::NodeRunStatus {
    match status {
        mob_dsl::NodeRunStatus::Pending => flow_frame::NodeRunStatus::Pending,
        mob_dsl::NodeRunStatus::Ready => flow_frame::NodeRunStatus::Ready,
        mob_dsl::NodeRunStatus::Running => flow_frame::NodeRunStatus::Running,
        mob_dsl::NodeRunStatus::Completed => flow_frame::NodeRunStatus::Completed,
        mob_dsl::NodeRunStatus::Failed => flow_frame::NodeRunStatus::Failed,
        mob_dsl::NodeRunStatus::Skipped => flow_frame::NodeRunStatus::Skipped,
        mob_dsl::NodeRunStatus::Canceled => flow_frame::NodeRunStatus::Canceled,
    }
}

fn fail_closed_unmigrated_projection<T>(
    projection: &'static str,
    command: impl std::fmt::Debug,
) -> Result<T, MobError> {
    // Reducer transition tables are quarantined until MobMachine can hand back
    // typed projection outcomes; authorization alone must not compute effects.
    Err(MobError::Internal(format!(
        "MobMachine-owned {projection} command {command:?} has no typed transition outcome; \
         reducer-visible state changes are fail-closed"
    )))
}

fn input_name(input: &mob_dsl::MobMachineInput) -> &'static str {
    match input {
        mob_dsl::MobMachineInput::CreateRunSeed { .. } => "CreateRunSeed",
        mob_dsl::MobMachineInput::AuthorizeFlowRunReducerCommand { .. } => {
            "AuthorizeFlowRunReducerCommand"
        }
        mob_dsl::MobMachineInput::CreateFrameSeed { .. } => "CreateFrameSeed",
        mob_dsl::MobMachineInput::AuthorizeFlowFrameReducerCommand { .. } => {
            "AuthorizeFlowFrameReducerCommand"
        }
        mob_dsl::MobMachineInput::CreateLoopSeed { .. } => "CreateLoopSeed",
        mob_dsl::MobMachineInput::RecordLoopBodyFrameCompleted { .. } => {
            "RecordLoopBodyFrameCompleted"
        }
        mob_dsl::MobMachineInput::RecordLoopUntilConditionMet { .. } => {
            "RecordLoopUntilConditionMet"
        }
        mob_dsl::MobMachineInput::RecordLoopUntilConditionFailed { .. } => {
            "RecordLoopUntilConditionFailed"
        }
        mob_dsl::MobMachineInput::AuthorizeLoopIterationReducerCommand { .. } => {
            "AuthorizeLoopIterationReducerCommand"
        }
        _ => "non_flow_input",
    }
}

fn signal_name(signal: &mob_dsl::MobMachineSignal) -> &'static str {
    match signal {
        mob_dsl::MobMachineSignal::StartFlow => "StartFlow",
        mob_dsl::MobMachineSignal::StartRun => "StartRun",
        _ => "non_flow_signal",
    }
}

fn effect_name(effect: &mob_dsl::MobMachineEffect) -> &'static str {
    match effect {
        mob_dsl::MobMachineEffect::EmitFlowRunNotice => "EmitFlowRunNotice",
        mob_dsl::MobMachineEffect::EmitRunLifecycleNotice => "EmitRunLifecycleNotice",
        _ => "non_flow_effect",
    }
}

/// Snapshot of MobMachine-owned frame projection state stored per-frame in MobRun.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameSnapshot {
    pub kernel_state: flow_frame::State,
}

/// Snapshot of MobMachine-owned loop projection state stored per-loop in MobRun.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoopSnapshot {
    pub kernel_state: loop_iteration::State,
}

/// Ledger entry recording the mapping of a loop iteration to its body frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopIterationLedgerEntry {
    pub loop_instance_id: LoopInstanceId,
    pub iteration: u64,
    pub frame_id: FrameId,
}

/// Persisted collection-policy kind stored in the flow kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunCollectionPolicyKind {
    All,
    Any,
    Quorum,
}

/// Persisted flow run aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobRun {
    pub run_id: RunId,
    pub mob_id: MobId,
    pub flow_id: FlowId,
    pub status: MobRunStatus,
    pub flow_state: flow_run::State,
    pub activation_params: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub step_ledger: Vec<StepLedgerEntry>,
    pub failure_ledger: Vec<FailureLedgerEntry>,
    /// Per-frame kernel snapshots indexed by FrameId.
    #[serde(default)]
    pub frames: BTreeMap<FrameId, FrameSnapshot>,
    /// Per-loop kernel snapshots indexed by LoopInstanceId.
    #[serde(default)]
    pub loops: BTreeMap<LoopInstanceId, LoopSnapshot>,
    /// Ordered ledger of loop iteration → body frame mappings.
    #[serde(default)]
    pub loop_iteration_ledger: Vec<LoopIterationLedgerEntry>,
    /// Schema version: 0 for legacy runs, 4 for self-describing frame-aware runs.
    #[serde(default)]
    pub schema_version: u32,
    /// Root frame step outputs keyed by step_id string.
    #[serde(default)]
    pub root_step_outputs: IndexMap<StepId, serde_json::Value>,
    /// Loop iteration outputs: key=loop_id, value=per-iteration step outputs.
    ///
    /// Uses `BTreeMap` (not `IndexMap`) so that recovery reconciliation can iterate
    /// in stable key order without depending on insertion order.
    #[serde(default)]
    pub loop_iteration_outputs: BTreeMap<LoopId, Vec<IndexMap<StepId, serde_json::Value>>>,
}

impl MobRun {
    /// Read-only access to the run's current status.
    pub fn status(&self) -> &MobRunStatus {
        &self.status
    }

    /// Read-only access to the run's current flow state.
    pub fn flow_state(&self) -> &flow_run::State {
        &self.flow_state
    }

    /// Typed view of the kernel-owned ordered step sequence.
    pub fn ordered_steps(&self) -> Result<Vec<StepId>, MobError> {
        Ok(self.flow_state.ordered_steps.clone())
    }

    /// Typed view of the kernel-owned dependency map keyed by step id.
    pub fn step_dependencies(&self) -> Result<BTreeMap<StepId, Vec<StepId>>, MobError> {
        Ok(self
            .flow_state
            .step_dependencies
            .iter()
            .map(|(step_id, deps)| (step_id.clone(), deps.clone()))
            .collect())
    }

    /// Typed view of the kernel-owned dependency mode map keyed by step id.
    pub fn step_dependency_modes(&self) -> Result<BTreeMap<StepId, DependencyMode>, MobError> {
        self.flow_state
            .step_dependency_modes
            .iter()
            .map(|(step_id, mode)| {
                let mode = match mode.as_str() {
                    "All" => DependencyMode::All,
                    "Any" => DependencyMode::Any,
                    _ => {
                        return Err(MobError::Internal(format!(
                            "flow_run step_dependency_modes unknown DependencyMode variant `{:?}` for {} step '{}'",
                            mode, self.run_id, step_id
                        )));
                    }
                };
                Ok((step_id.clone(), mode))
            })
            .collect()
    }

    /// Typed view of the kernel-owned condition-presence map keyed by step id.
    pub fn step_has_conditions(&self) -> Result<BTreeMap<StepId, bool>, MobError> {
        Ok(self
            .flow_state
            .step_has_conditions
            .iter()
            .map(|(step_id, flag)| (step_id.clone(), *flag))
            .collect())
    }

    /// Typed view of the kernel-owned branch label map keyed by step id.
    pub fn step_branches(&self) -> Result<BTreeMap<StepId, Option<BranchId>>, MobError> {
        Ok(self
            .flow_state
            .step_branches
            .iter()
            .map(|(step_id, branch)| (step_id.clone(), branch.clone()))
            .collect())
    }

    /// Typed view of the kernel-owned collection policy kind map keyed by step id.
    pub fn step_collection_policy_kinds(
        &self,
    ) -> Result<BTreeMap<StepId, RunCollectionPolicyKind>, MobError> {
        self.flow_state
            .step_collection_policies
            .iter()
            .map(|(step_id, policy)| {
                let policy = match policy.as_str() {
                    "All" => RunCollectionPolicyKind::All,
                    "Any" => RunCollectionPolicyKind::Any,
                    "Quorum" => RunCollectionPolicyKind::Quorum,
                    _ => {
                        return Err(MobError::Internal(format!(
                            "flow_run step_collection_policies unknown CollectionPolicyKind variant `{:?}` for {} step '{}'",
                            policy, self.run_id, step_id
                        )));
                    }
                };
                Ok((step_id.clone(), policy))
            })
            .collect()
    }

    /// Typed view of the kernel-owned quorum-threshold map keyed by step id.
    pub fn step_quorum_thresholds(&self) -> Result<BTreeMap<StepId, u32>, MobError> {
        Ok(self
            .flow_state
            .step_quorum_thresholds
            .iter()
            .map(|(step_id, threshold)| (step_id.clone(), *threshold))
            .collect())
    }

    /// Typed view of the kernel-owned step status map, excluding `None` entries.
    pub fn step_status_snapshot(&self) -> Result<BTreeMap<StepId, StepRunStatus>, MobError> {
        let mut statuses = BTreeMap::new();
        for (step_key, value) in &self.flow_state.step_status {
            let Some(value) = value else {
                continue;
            };
            statuses.insert(
                step_key.clone(),
                StepRunStatus::from_flow_run_status(value.as_str(), &self.run_id)?,
            );
        }

        Ok(statuses)
    }

    /// Typed view of the kernel-owned cumulative failure counter.
    pub fn failure_count(&self) -> Result<u32, MobError> {
        Ok(self.flow_state.failure_count)
    }

    /// Typed view of the kernel-owned consecutive-failure counter.
    pub fn consecutive_failure_count(&self) -> Result<u32, MobError> {
        Ok(self.flow_state.consecutive_failure_count)
    }

    /// Typed view of the kernel-owned retry budget.
    pub fn max_step_retries(&self) -> Result<u32, MobError> {
        Ok(self.flow_state.max_step_retries)
    }

    /// Typed view of the kernel-owned supervisor escalation threshold.
    pub fn escalation_threshold(&self) -> Result<u32, MobError> {
        Ok(self.flow_state.escalation_threshold)
    }
}

impl MobRun {
    pub fn pending(
        mob_id: MobId,
        flow_id: FlowId,
        flow_state: flow_run::State,
        activation_params: serde_json::Value,
    ) -> Self {
        Self::pending_with_run_id(RunId::new(), mob_id, flow_id, flow_state, activation_params)
    }

    pub(crate) fn pending_with_run_id(
        run_id: RunId,
        mob_id: MobId,
        flow_id: FlowId,
        flow_state: flow_run::State,
        activation_params: serde_json::Value,
    ) -> Self {
        Self {
            run_id,
            mob_id,
            flow_id,
            status: MobRunStatus::Pending,
            flow_state,
            activation_params,
            created_at: Utc::now(),
            completed_at: None,
            step_ledger: Vec::new(),
            failure_ledger: Vec::new(),
            frames: BTreeMap::new(),
            loops: BTreeMap::new(),
            loop_iteration_ledger: Vec::new(),
            schema_version: 5,
            root_step_outputs: IndexMap::new(),
            loop_iteration_outputs: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub fn flow_state_for_config(config: &FlowRunConfig) -> Result<flow_run::State, MobError> {
        let run_id = RunId::new();
        let seed_input = Self::create_run_seed_input(&run_id, config)?;
        let mut authority = mob_dsl::MobMachineAuthority::new();
        authority.state.lifecycle_phase = mob_dsl::MobPhase::Running;
        mob_dsl::MobMachineMutator::apply(&mut authority, seed_input)
            .map_err(|error| MobError::Internal(format!("test CreateRunSeed rejected: {error}")))?;
        Self::flow_state_for_config_with_authority(&run_id, config, &authority.state)
    }

    pub(crate) fn flow_state_for_config_with_authority(
        run_id: &RunId,
        config: &FlowRunConfig,
        machine_state: &mob_dsl::MobMachineState,
    ) -> Result<flow_run::State, MobError> {
        let _ = config;
        project_flow_run_state_from_machine(machine_state, run_id)
    }

    pub(crate) fn create_run_seed_input(
        run_id: &RunId,
        config: &FlowRunConfig,
    ) -> Result<mob_dsl::MobMachineInput, MobError> {
        let ordered_steps = topological_steps(&config.flow_spec)?;
        Ok(mob_dsl::MobMachineInput::CreateRunSeed {
            run_id: mob_dsl::RunId::from(run_id.to_string()),
            step_ids: config
                .flow_spec
                .steps
                .keys()
                .map(|step_id| mob_dsl::StepId::from(step_id.as_str()))
                .collect(),
            ordered_steps: ordered_steps
                .iter()
                .map(|step_id| mob_dsl::StepId::from(step_id.as_str()))
                .collect(),
            step_has_conditions: config
                .flow_spec
                .steps
                .iter()
                .map(|(step_id, step)| {
                    (
                        mob_dsl::StepId::from(step_id.as_str()),
                        step.condition.is_some(),
                    )
                })
                .collect(),
            step_dependencies: config
                .flow_spec
                .steps
                .iter()
                .map(|(step_id, step)| {
                    (
                        mob_dsl::StepId::from(step_id.as_str()),
                        step.depends_on
                            .iter()
                            .map(|dep| mob_dsl::StepId::from(dep.as_str()))
                            .collect(),
                    )
                })
                .collect(),
            step_dependency_modes: config
                .flow_spec
                .steps
                .iter()
                .map(|(step_id, step)| {
                    (
                        mob_dsl::StepId::from(step_id.as_str()),
                        dependency_mode_seed_value(step.depends_on_mode.clone()),
                    )
                })
                .collect(),
            step_branches: config
                .flow_spec
                .steps
                .iter()
                .map(|(step_id, step)| {
                    (
                        mob_dsl::StepId::from(step_id.as_str()),
                        step.branch
                            .as_ref()
                            .map(|branch| mob_dsl::BranchId::from(branch.as_str())),
                    )
                })
                .collect(),
            step_collection_policies: config
                .flow_spec
                .steps
                .iter()
                .map(|(step_id, step)| {
                    (
                        mob_dsl::StepId::from(step_id.as_str()),
                        collection_policy_seed_value(&step.collection_policy),
                    )
                })
                .collect(),
            step_quorum_thresholds: config
                .flow_spec
                .steps
                .iter()
                .map(|(step_id, step)| {
                    let threshold = match step.collection_policy {
                        crate::definition::CollectionPolicy::Quorum { n } => u32::from(n),
                        _ => 0,
                    };
                    (mob_dsl::StepId::from(step_id.as_str()), threshold)
                })
                .collect(),
            escalation_threshold: config
                .supervisor
                .as_ref()
                .map_or(0, |supervisor| supervisor.escalation_threshold),
            max_step_retries: config
                .limits
                .as_ref()
                .and_then(|limits| limits.max_step_retries)
                .unwrap_or(0),
            max_active_nodes: config
                .limits
                .as_ref()
                .and_then(|l| l.max_active_nodes)
                .unwrap_or(0)
                .try_into()
                .map_err(|_| MobError::Internal("max_active_nodes exceeds u32".to_string()))?,
            max_active_frames: config
                .limits
                .as_ref()
                .and_then(|l| l.max_active_frames)
                .unwrap_or(0)
                .try_into()
                .map_err(|_| MobError::Internal("max_active_frames exceeds u32".to_string()))?,
            max_frame_depth: config
                .limits
                .as_ref()
                .and_then(|l| l.max_frame_depth)
                .unwrap_or(0)
                .try_into()
                .map_err(|_| MobError::Internal("max_frame_depth exceeds u32".to_string()))?,
        })
    }

    pub(crate) fn create_frame_seed_input(
        run_id: &RunId,
        frame_id: &FrameId,
        loop_instance_id: Option<&LoopInstanceId>,
        iteration: u32,
        frame_scope: mob_dsl::FrameScope,
        spec: &FrameSpec,
        ordered: &[FlowNodeId],
    ) -> Result<mob_dsl::MobMachineInput, MobError> {
        let tracked_nodes = ordered
            .iter()
            .map(|node_id| mob_dsl::FlowNodeId::from(node_id.as_str()))
            .collect();
        let ordered_nodes = ordered
            .iter()
            .map(|node_id| mob_dsl::FlowNodeId::from(node_id.as_str()))
            .collect();
        let mut node_kind = BTreeMap::new();
        let mut node_dependencies = BTreeMap::new();
        let mut node_dependency_modes = BTreeMap::new();
        let mut node_branches = BTreeMap::new();

        for (node_id, node_spec) in &spec.nodes {
            let key = mob_dsl::FlowNodeId::from(node_id.as_str());
            match node_spec {
                FlowNodeSpec::Step(step) => {
                    node_kind.insert(key.clone(), mob_dsl::FlowNodeKind::Step);
                    node_dependencies.insert(
                        key.clone(),
                        step.depends_on
                            .iter()
                            .map(|dep| mob_dsl::FlowNodeId::from(dep.as_str()))
                            .collect(),
                    );
                    node_dependency_modes.insert(
                        key.clone(),
                        dependency_mode_seed_value(step.depends_on_mode.clone()),
                    );
                    node_branches.insert(
                        key,
                        step.branch
                            .as_ref()
                            .map(|branch| mob_dsl::BranchId::from(branch.as_str())),
                    );
                }
                FlowNodeSpec::RepeatUntil(loop_spec) => {
                    node_kind.insert(key.clone(), mob_dsl::FlowNodeKind::Loop);
                    node_dependencies.insert(
                        key.clone(),
                        loop_spec
                            .depends_on
                            .iter()
                            .map(|dep| mob_dsl::FlowNodeId::from(dep.as_str()))
                            .collect(),
                    );
                    node_dependency_modes.insert(
                        key.clone(),
                        dependency_mode_seed_value(loop_spec.depends_on_mode.clone()),
                    );
                    node_branches.insert(key, None);
                }
            }
        }

        Ok(mob_dsl::MobMachineInput::CreateFrameSeed {
            run_id: mob_dsl::RunId::from(run_id.to_string()),
            frame_id: mob_dsl::FrameId::from(frame_id.as_str()),
            frame_scope,
            loop_instance_id: loop_instance_id.map(|id| mob_dsl::LoopInstanceId::from(id.as_str())),
            iteration,
            tracked_nodes,
            ordered_nodes,
            node_kind,
            node_dependencies,
            node_dependency_modes,
            node_branches,
        })
    }

    pub(crate) fn create_loop_seed_input(
        snapshot: &LoopSnapshot,
    ) -> Result<mob_dsl::MobMachineInput, MobError> {
        Ok(Self::create_loop_seed_input_for_start(
            &snapshot.kernel_state.loop_instance_id,
            &snapshot.kernel_state.parent_frame_id,
            &snapshot.kernel_state.parent_node_id,
            &snapshot.kernel_state.loop_id,
            snapshot.kernel_state.depth,
            snapshot.kernel_state.max_iterations,
        ))
    }

    pub(crate) fn create_loop_seed_input_for_start(
        loop_instance_id: &LoopInstanceId,
        parent_frame_id: &FrameId,
        parent_node_id: &FlowNodeId,
        loop_id: &LoopId,
        depth: u32,
        max_iterations: u32,
    ) -> mob_dsl::MobMachineInput {
        mob_dsl::MobMachineInput::CreateLoopSeed {
            loop_instance_id: mob_dsl::LoopInstanceId::from(loop_instance_id.as_str()),
            parent_frame_id: mob_dsl::FrameId::from(parent_frame_id.as_str()),
            parent_node_id: mob_dsl::FlowNodeId::from(parent_node_id.as_str()),
            loop_id: mob_dsl::LoopId::from(loop_id.as_str()),
            depth,
            max_iterations: max_iterations as u64,
        }
    }

    pub(crate) fn record_loop_body_frame_completed_input(
        loop_instance_id: &LoopInstanceId,
        iteration: u32,
    ) -> mob_dsl::MobMachineInput {
        mob_dsl::MobMachineInput::RecordLoopBodyFrameCompleted {
            loop_instance_id: mob_dsl::LoopInstanceId::from(loop_instance_id.as_str()),
            iteration: iteration as u64,
        }
    }

    pub(crate) fn record_loop_until_condition_feedback_input(
        loop_instance_id: &LoopInstanceId,
        iteration: u32,
        until_met: bool,
    ) -> mob_dsl::MobMachineInput {
        let loop_instance_id = mob_dsl::LoopInstanceId::from(loop_instance_id.as_str());
        if until_met {
            mob_dsl::MobMachineInput::RecordLoopUntilConditionMet {
                loop_instance_id,
                iteration: iteration as u64,
            }
        } else {
            mob_dsl::MobMachineInput::RecordLoopUntilConditionFailed {
                loop_instance_id,
                iteration: iteration as u64,
            }
        }
    }

    pub fn flow_state_for_steps<I>(step_ids: I) -> Result<flow_run::State, MobError>
    where
        I: IntoIterator<Item = StepId>,
    {
        let mut steps = IndexMap::new();
        for step_id in step_ids {
            steps.insert(
                step_id,
                crate::definition::FlowStepSpec {
                    role: ProfileName::from("worker"),
                    message: meerkat_core::types::ContentInput::from("placeholder"),
                    depends_on: Vec::new(),
                    dispatch_mode: crate::definition::DispatchMode::FanOut,
                    collection_policy: crate::definition::CollectionPolicy::All,
                    condition: None,
                    timeout_ms: None,
                    expected_schema_ref: None,
                    branch: None,
                    depends_on_mode: crate::definition::DependencyMode::All,
                    allowed_tools: None,
                    blocked_tools: None,
                    output_format: crate::definition::StepOutputFormat::Json,
                },
            );
        }
        let config = FlowRunConfig {
            flow_id: FlowId::from("placeholder"),
            flow_spec: FlowSpec {
                description: None,
                steps,
                root: None,
            },
            topology: None,
            supervisor: None,
            limits: None,
            orchestrator_role: None,
        };
        let run_id = RunId::new();
        let seed_input = Self::create_run_seed_input(&run_id, &config)?;
        let mut authority = mob_dsl::MobMachineAuthority::new();
        authority.state.lifecycle_phase = mob_dsl::MobPhase::Running;
        mob_dsl::MobMachineMutator::apply(&mut authority, seed_input)
            .map_err(|error| MobError::Internal(format!("test CreateRunSeed rejected: {error}")))?;
        Self::flow_state_for_config_with_authority(&run_id, &config, &authority.state)
    }
}

fn dependency_mode_value(mode: crate::definition::DependencyMode) -> flow_run::DependencyMode {
    match mode {
        crate::definition::DependencyMode::All => flow_run::DependencyMode::All,
        crate::definition::DependencyMode::Any => flow_run::DependencyMode::Any,
    }
}

fn dependency_mode_seed_value(mode: crate::definition::DependencyMode) -> mob_dsl::DependencyMode {
    match mode {
        crate::definition::DependencyMode::All => mob_dsl::DependencyMode::All,
        crate::definition::DependencyMode::Any => mob_dsl::DependencyMode::Any,
    }
}

fn collection_policy_kind_value(
    policy: &crate::definition::CollectionPolicy,
) -> flow_run::CollectionPolicyKind {
    match policy {
        crate::definition::CollectionPolicy::All => flow_run::CollectionPolicyKind::All,
        crate::definition::CollectionPolicy::Any => flow_run::CollectionPolicyKind::Any,
        crate::definition::CollectionPolicy::Quorum { .. } => {
            flow_run::CollectionPolicyKind::Quorum
        }
    }
}

fn collection_policy_seed_value(
    policy: &crate::definition::CollectionPolicy,
) -> mob_dsl::CollectionPolicyKind {
    match policy {
        crate::definition::CollectionPolicy::All => mob_dsl::CollectionPolicyKind::All,
        crate::definition::CollectionPolicy::Any => mob_dsl::CollectionPolicyKind::Any,
        crate::definition::CollectionPolicy::Quorum { .. } => mob_dsl::CollectionPolicyKind::Quorum,
    }
}

fn topological_steps(flow_spec: &FlowSpec) -> Result<Vec<StepId>, MobError> {
    let mut in_degree: BTreeMap<StepId, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<StepId, Vec<StepId>> = BTreeMap::new();

    for step_id in flow_spec.steps.keys() {
        in_degree.insert(step_id.clone(), 0);
        outgoing.entry(step_id.clone()).or_default();
    }

    for (step_id, step) in &flow_spec.steps {
        for dependency in &step.depends_on {
            // TLA+ NoSelfDependencyInvariant: a step cannot depend on itself.
            if dependency == step_id {
                return Err(MobError::Internal(format!(
                    "step '{step_id}' has a self-dependency"
                )));
            }
            if !in_degree.contains_key(dependency) {
                return Err(MobError::Internal(format!(
                    "step '{step_id}' depends on unknown step '{dependency}'"
                )));
            }
            *in_degree.entry(step_id.clone()).or_insert(0) += 1;
            outgoing
                .entry(dependency.clone())
                .or_default()
                .push(step_id.clone());
        }
    }

    let mut queue = VecDeque::new();
    for step_id in flow_spec.steps.keys() {
        if in_degree.get(step_id) == Some(&0) {
            queue.push_back(step_id.clone());
        }
    }

    let mut ordered = Vec::with_capacity(flow_spec.steps.len());
    while let Some(next) = queue.pop_front() {
        ordered.push(next.clone());
        if let Some(children) = outgoing.get(&next) {
            for child in children {
                if let Some(count) = in_degree.get_mut(child)
                    && *count > 0
                {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != flow_spec.steps.len() {
        return Err(MobError::Internal(
            "flow contains a cycle; cannot compute topological order".to_string(),
        ));
    }

    Ok(ordered)
}

/// Run lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Canceled,
}

impl MobRunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

/// Per-target step execution ledger entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepLedgerEntry {
    pub step_id: StepId,
    pub agent_identity: AgentIdentity,
    pub status: StepRunStatus,
    pub output: Option<serde_json::Value>,
    pub timestamp: DateTime<Utc>,
}

/// Step execution state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepRunStatus {
    Dispatched,
    Completed,
    Failed,
    Skipped,
    Canceled,
}

impl StepRunStatus {
    pub(crate) fn from_flow_run_status(value: &str, run_id: &RunId) -> Result<Self, MobError> {
        match value {
            "Dispatched" => Ok(Self::Dispatched),
            "Completed" => Ok(Self::Completed),
            "Failed" => Ok(Self::Failed),
            "Skipped" => Ok(Self::Skipped),
            "Canceled" => Ok(Self::Canceled),
            other => Err(MobError::Internal(format!(
                "unknown StepRunStatus variant `{other}` for {run_id}"
            ))),
        }
    }

    /// A step is terminal when it can no longer receive work dispatch or
    /// completion events. Only `Dispatched` is non-terminal.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Dispatched)
    }
}

/// Flow-level failure log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureLedgerEntry {
    pub step_id: StepId,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

/// Immutable per-run flow snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowRunConfig {
    pub flow_id: FlowId,
    pub flow_spec: FlowSpec,
    pub topology: Option<TopologySpec>,
    pub supervisor: Option<SupervisorSpec>,
    pub limits: Option<LimitsSpec>,
    pub orchestrator_role: Option<ProfileName>,
}

impl FlowRunConfig {
    pub fn from_definition(
        flow_id: FlowId,
        definition: &crate::definition::MobDefinition,
    ) -> Result<Self, MobError> {
        let flow_spec = definition
            .flows
            .get(&flow_id)
            .cloned()
            .ok_or_else(|| MobError::FlowNotFound(flow_id.clone()))?;
        let topology = definition.topology.clone();
        let orchestrator_role = definition
            .orchestrator
            .as_ref()
            .map(|orchestrator| orchestrator.profile.clone());
        if topology.is_some() && orchestrator_role.is_none() {
            return Err(MobError::Internal(
                "topology requires an orchestrator profile".to_string(),
            ));
        }
        Ok(Self {
            flow_id,
            flow_spec,
            topology,
            supervisor: definition.supervisor.clone(),
            limits: definition.limits.clone(),
            orchestrator_role,
        })
    }
}

/// Per-loop iteration output history, ordered by iteration index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LoopContextHistory {
    /// One entry per completed iteration, ordered by iteration index.
    pub iterations: Vec<IndexMap<StepId, serde_json::Value>>,
}

/// Runtime context available to condition evaluators.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowContext {
    pub run_id: RunId,
    pub activation_params: serde_json::Value,
    /// Root frame step outputs keyed by step_id.
    pub step_outputs: IndexMap<StepId, serde_json::Value>,
    /// Per-loop iteration history keyed by loop_id.
    #[serde(default)]
    pub loop_outputs: IndexMap<LoopId, LoopContextHistory>,
}

impl FlowContext {
    /// Rebuild a `FlowContext` from a persisted `MobRun` aggregate.
    pub fn from_run_aggregate(
        run: &MobRun,
        run_id: RunId,
        activation_params: serde_json::Value,
    ) -> Self {
        let loop_outputs = run
            .loop_iteration_outputs
            .iter()
            .map(|(loop_id, iterations)| {
                let history = LoopContextHistory {
                    iterations: iterations.clone(),
                };
                (loop_id.clone(), history)
            })
            .collect();

        // Seed step_outputs from root outputs, then project last-iteration
        // outputs from each completed loop into step_outputs (dogma Rule 13:
        // the projection must match what execute_frame_inner does at runtime —
        // the last iteration's body step outputs are merged into step_outputs
        // so that downstream steps/templates see them at steps.<id>).
        let mut step_outputs = run.root_step_outputs.clone();
        for iterations in run.loop_iteration_outputs.values() {
            if let Some(last_iter) = iterations.last() {
                for (sid, out) in last_iter {
                    step_outputs.insert(sid.clone(), out.clone());
                }
            }
        }

        FlowContext {
            run_id,
            activation_params,
            step_outputs,
            loop_outputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{
        BackendConfig, ConditionExpr, DispatchMode, FlowStepSpec, MobDefinition,
        OrchestratorConfig, WiringRules,
    };
    use crate::ids::{BranchId, ProfileName};
    use crate::profile::{Profile, ProfileBinding, ToolConfig};
    use meerkat_core::types::ContentInput;
    use std::collections::BTreeMap;

    #[test]
    fn flow_projection_audit_requires_fail_closed_mob_machine_authority() {
        for record in flow_projection_kernel_audit() {
            assert_eq!(record.canonical_owner, "MobMachine");
            assert_eq!(
                record.role,
                FlowProjectionKernelRole::MobMachineOwnedFailClosedProjection
            );
            assert!(!record.canonical_machine);
            assert!(
                record
                    .owning_inputs
                    .iter()
                    .any(|input| input.starts_with("Authorize") || input.ends_with("Seed")),
                "projection {} must name the MobMachine input that authorizes it",
                record.module
            );
        }
    }

    #[test]
    fn flow_reducer_apply_rejects_wrong_authority_token_family() {
        let run_state = flow_run::initial_state();
        let machine_state = mob_dsl::MobMachineState::default();
        let run_id = RunId::new();
        let frame_authority_input = mob_dsl::MobMachineInput::CreateFrameSeed {
            run_id: mob_dsl::RunId::from("run"),
            frame_id: mob_dsl::FrameId::from("frame"),
            frame_scope: crate::machines::mob_machine::FrameScope::Root,
            loop_instance_id: None,
            iteration: 0,
            tracked_nodes: Default::default(),
            ordered_nodes: Default::default(),
            node_kind: Default::default(),
            node_dependencies: Default::default(),
            node_dependency_modes: Default::default(),
            node_branches: Default::default(),
        };
        let frame_token =
            MobMachineFlowAuthorityToken::from_accepted_mob_machine_input(&frame_authority_input)
                .expect("frame seed input must authorize frame reducer family");
        let err = apply_mob_machine_flow_run_command(
            &run_state,
            &machine_state,
            &run_id,
            MobMachineFlowRunCommand::StartRun(flow_run::inputs::StartRun {}),
            frame_token,
        )
        .expect_err("flow_run reducer must reject frame authority");
        assert!(
            err.to_string().contains("cannot authorize FlowRun"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn flow_reducer_apply_fails_closed_with_matching_authority_until_typed_outcomes_exist() {
        let run_state = flow_run::initial_state();
        let machine_state = mob_dsl::MobMachineState::default();
        let run_id = RunId::new();
        let step_id = StepId::from("step");
        let run_authority_input = mob_dsl::MobMachineInput::AuthorizeFlowRunReducerCommand {
            run_id: mob_dsl::RunId::from(run_id.to_string()),
            command: mob_dsl::FlowRunReducerCommandKind::CompleteStep,
            step_id: Some(mob_dsl::StepId::from(step_id.as_str())),
            run_step_key: None,
            step_status: None,
            target_count: None,
            frame_id: None,
            loop_instance_id: None,
            retry_key: None,
        };
        let run_token =
            MobMachineFlowAuthorityToken::from_accepted_mob_machine_input(&run_authority_input)
                .expect("run command input must authorize run reducer family");
        let err = apply_mob_machine_flow_run_command(
            &run_state,
            &machine_state,
            &run_id,
            MobMachineFlowRunCommand::CompleteStep(flow_run::inputs::CompleteStep { step_id }),
            run_token,
        )
        .expect_err("authorization-only reducer authority must still fail closed");
        assert!(
            err.to_string().contains("only authorized"),
            "unexpected error: {err}"
        );
    }

    fn sample_definition() -> MobDefinition {
        let mut steps = IndexMap::new();
        steps.insert(
            StepId::from("s1"),
            FlowStepSpec {
                role: ProfileName::from("worker"),
                message: ContentInput::from("do it"),
                depends_on: Vec::new(),
                dispatch_mode: DispatchMode::FanOut,
                collection_policy: crate::definition::CollectionPolicy::All,
                condition: Some(ConditionExpr::Eq {
                    path: "params.ok".to_string(),
                    value: serde_json::json!(true),
                }),
                timeout_ms: Some(2000),
                expected_schema_ref: Some("schema.json".to_string()),
                branch: Some(BranchId::from("branch-a")),
                depends_on_mode: crate::definition::DependencyMode::All,
                allowed_tools: None,
                blocked_tools: None,
                output_format: crate::definition::StepOutputFormat::Json,
            },
        );

        let mut flows = BTreeMap::new();
        flows.insert(
            FlowId::from("flow-a"),
            FlowSpec {
                description: Some("demo flow".to_string()),
                steps,
                root: None,
            },
        );

        let mut profiles = BTreeMap::new();
        profiles.insert(
            ProfileName::from("lead"),
            ProfileBinding::Inline(Profile {
                model: "model".to_string(),
                skills: Vec::new(),
                tools: ToolConfig::default(),
                peer_description: "lead".to_string(),
                external_addressable: true,
                backend: None,
                runtime_mode: crate::MobRuntimeMode::AutonomousHost,
                max_inline_peer_notifications: None,
                output_schema: None,
                provider_params: None,
            }),
        );
        profiles.insert(
            ProfileName::from("worker"),
            ProfileBinding::Inline(Profile {
                model: "model".to_string(),
                skills: Vec::new(),
                tools: ToolConfig::default(),
                peer_description: "worker".to_string(),
                external_addressable: false,
                backend: None,
                runtime_mode: crate::MobRuntimeMode::AutonomousHost,
                max_inline_peer_notifications: None,
                output_schema: None,
                provider_params: None,
            }),
        );

        MobDefinition {
            id: MobId::from("mob"),
            orchestrator: Some(OrchestratorConfig {
                profile: ProfileName::from("lead"),
            }),
            profiles,
            mcp_servers: BTreeMap::new(),
            wiring: WiringRules::default(),
            skills: BTreeMap::new(),
            backend: BackendConfig::default(),
            flows,
            topology: Some(TopologySpec {
                mode: crate::definition::PolicyMode::Advisory,
                rules: vec![crate::definition::TopologyRule {
                    from_role: ProfileName::from("lead"),
                    to_role: ProfileName::from("worker"),
                    allowed: true,
                }],
            }),
            supervisor: Some(SupervisorSpec {
                role: ProfileName::from("lead"),
                escalation_threshold: 3,
            }),
            limits: Some(LimitsSpec {
                max_flow_duration_ms: Some(60_000),
                max_step_retries: Some(1),
                max_orphaned_turns: Some(8),
                cancel_grace_timeout_ms: None,
                ..Default::default()
            }),
            spawn_policy: None,
            event_router: None,
            owner_bridge_session_id: None,
            session_cleanup_policy: crate::definition::SessionCleanupPolicy::Manual,
            is_implicit: false,
        }
    }

    #[test]
    fn test_run_status_terminal() {
        assert!(MobRunStatus::Completed.is_terminal());
        assert!(MobRunStatus::Failed.is_terminal());
        assert!(MobRunStatus::Canceled.is_terminal());
        assert!(!MobRunStatus::Pending.is_terminal());
        assert!(!MobRunStatus::Running.is_terminal());
    }

    #[test]
    fn test_mob_run_kernel_readers_surface_ordered_steps_and_status_snapshot() {
        let mut run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a"), StepId::from("step-b")]).unwrap(),
            serde_json::json!({}),
        );
        run.flow_state.step_status = BTreeMap::from([
            (
                StepId::from("step-a"),
                Some(flow_run::StepRunStatus::Completed),
            ),
            (StepId::from("step-b"), None),
        ]);
        run.flow_state.failure_count = 3;
        run.flow_state.consecutive_failure_count = 2;
        run.flow_state.max_step_retries = 4;
        run.flow_state.escalation_threshold = 3;

        assert_eq!(
            run.ordered_steps().unwrap(),
            vec![StepId::from("step-a"), StepId::from("step-b")]
        );
        assert_eq!(
            run.step_dependencies().unwrap(),
            BTreeMap::from([
                (StepId::from("step-a"), Vec::new()),
                (StepId::from("step-b"), Vec::new()),
            ])
        );
        assert_eq!(
            run.step_dependency_modes().unwrap(),
            BTreeMap::from([
                (StepId::from("step-a"), DependencyMode::All),
                (StepId::from("step-b"), DependencyMode::All),
            ])
        );
        assert_eq!(
            run.step_has_conditions().unwrap(),
            BTreeMap::from([
                (StepId::from("step-a"), false),
                (StepId::from("step-b"), false)
            ])
        );
        assert_eq!(
            run.step_branches().unwrap(),
            BTreeMap::from([
                (StepId::from("step-a"), None),
                (StepId::from("step-b"), None)
            ])
        );
        assert_eq!(
            run.step_collection_policy_kinds().unwrap(),
            BTreeMap::from([
                (StepId::from("step-a"), RunCollectionPolicyKind::All),
                (StepId::from("step-b"), RunCollectionPolicyKind::All),
            ])
        );
        assert_eq!(
            run.step_quorum_thresholds().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), 0), (StepId::from("step-b"), 0)])
        );
        assert_eq!(
            run.step_status_snapshot().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), StepRunStatus::Completed)])
        );
        assert_eq!(run.failure_count().unwrap(), 3);
        assert_eq!(run.consecutive_failure_count().unwrap(), 2);
        assert_eq!(run.max_step_retries().unwrap(), 4);
        assert_eq!(run.escalation_threshold().unwrap(), 3);
    }

    #[test]
    fn test_mob_run_step_status_snapshot_accepts_typed_variant() {
        let mut run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        run.flow_state.step_status = BTreeMap::from([(
            StepId::from("step-a"),
            Some(flow_run::StepRunStatus::Completed),
        )]);
        assert_eq!(
            run.step_status_snapshot().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), StepRunStatus::Completed)])
        );
    }

    #[test]
    fn test_mob_run_step_status_snapshot_accepts_some_wrapped_variant() {
        let mut run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        run.flow_state.step_status = BTreeMap::from([(
            StepId::from("step-a"),
            Some(flow_run::StepRunStatus::Completed),
        )]);

        assert_eq!(
            run.step_status_snapshot().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), StepRunStatus::Completed)])
        );
    }

    #[test]
    fn test_mob_run_step_dependencies_reject_invalid_dependency_entry() {
        // Typed state makes invalid dependency payloads unrepresentable.
        let run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        assert_eq!(
            run.step_dependencies().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), Vec::new())])
        );
    }

    #[test]
    fn test_mob_run_step_dependency_modes_accept_typed_variant() {
        let mut run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        run.flow_state.step_dependency_modes =
            BTreeMap::from([(StepId::from("step-a"), flow_run::DependencyMode::All)]);

        assert_eq!(
            run.step_dependency_modes().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), DependencyMode::All)])
        );
    }

    #[test]
    fn test_mob_run_step_collection_policy_kinds_accept_typed_variant() {
        let mut run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        run.flow_state.step_collection_policies =
            BTreeMap::from([(StepId::from("step-a"), flow_run::CollectionPolicyKind::All)]);

        assert_eq!(
            run.step_collection_policy_kinds().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), RunCollectionPolicyKind::All)])
        );
    }

    #[test]
    fn test_mob_run_step_has_conditions_rejects_non_bool_entry() {
        let run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        assert_eq!(
            run.step_has_conditions().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), false)])
        );
    }

    #[test]
    fn test_mob_run_step_branches_reject_invalid_entry() {
        let run = MobRun::pending(
            MobId::from("mob"),
            FlowId::from("flow-a"),
            MobRun::flow_state_for_steps([StepId::from("step-a")]).unwrap(),
            serde_json::json!({}),
        );
        assert_eq!(
            run.step_branches().unwrap(),
            BTreeMap::from([(StepId::from("step-a"), None)])
        );
    }

    #[test]
    fn test_flow_run_config_from_definition() {
        let def = sample_definition();
        let config = FlowRunConfig::from_definition(FlowId::from("flow-a"), &def).unwrap();
        assert_eq!(config.flow_id, FlowId::from("flow-a"));
        assert_eq!(config.flow_spec.steps.len(), 1);
        assert_eq!(
            config.orchestrator_role.as_ref(),
            Some(&ProfileName::from("lead"))
        );
    }

    #[test]
    fn test_flow_run_config_from_definition_missing_flow() {
        let def = sample_definition();
        let error = FlowRunConfig::from_definition(FlowId::from("missing"), &def).unwrap_err();
        assert!(matches!(error, MobError::FlowNotFound(name) if name == "missing"));
    }

    #[test]
    fn test_flow_run_config_rejects_topology_without_orchestrator() {
        let mut def = sample_definition();
        def.orchestrator = None;
        let error = FlowRunConfig::from_definition(FlowId::from("flow-a"), &def).unwrap_err();
        assert!(
            matches!(error, MobError::Internal(message) if message.contains("topology requires")),
            "expected explicit topology/orchestrator configuration error"
        );
    }

    #[test]
    fn test_mob_run_roundtrip_json() {
        let now = Utc::now();
        let run = MobRun {
            run_id: RunId::new(),
            mob_id: MobId::from("mob"),
            flow_id: FlowId::from("flow-a"),
            status: MobRunStatus::Running,
            flow_state: MobRun::flow_state_for_steps([StepId::from("step-1")]).unwrap(),
            activation_params: serde_json::json!({"k":"v"}),
            created_at: now,
            completed_at: None,
            step_ledger: vec![StepLedgerEntry {
                step_id: StepId::from("step-1"),
                agent_identity: AgentIdentity::from("agent-1"),
                status: StepRunStatus::Completed,
                output: Some(serde_json::json!({"ok":true})),
                timestamp: now,
            }],
            failure_ledger: vec![FailureLedgerEntry {
                step_id: StepId::from("step-2"),
                reason: "boom".to_string(),
                timestamp: now,
            }],
            frames: BTreeMap::new(),
            loops: BTreeMap::new(),
            loop_iteration_ledger: Vec::new(),
            schema_version: 4,
            root_step_outputs: IndexMap::new(),
            loop_iteration_outputs: BTreeMap::new(),
        };

        let encoded = serde_json::to_string(&run).unwrap();
        let decoded: MobRun = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.flow_id, run.flow_id);
        assert_eq!(decoded.step_ledger.len(), 1);
        assert_eq!(decoded.failure_ledger.len(), 1);
    }

    #[test]
    fn test_flow_context_roundtrip_json() {
        let mut outputs = IndexMap::new();
        outputs.insert(StepId::from("step-1"), serde_json::json!({"a":1}));
        let context = FlowContext {
            run_id: RunId::new(),
            activation_params: serde_json::json!({"input":"x"}),
            step_outputs: outputs,
            loop_outputs: IndexMap::new(),
        };

        let encoded = serde_json::to_string(&context).unwrap();
        let decoded: FlowContext = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.step_outputs.len(), 1);
        assert_eq!(decoded.activation_params["input"], "x");
    }

    #[test]
    fn topological_steps_rejects_self_dependency() {
        let mut steps = IndexMap::new();
        steps.insert(
            StepId::from("s1"),
            FlowStepSpec {
                role: ProfileName::from("worker"),
                message: ContentInput::from("do it"),
                depends_on: vec![StepId::from("s1")],
                dispatch_mode: DispatchMode::FanOut,
                collection_policy: crate::definition::CollectionPolicy::All,
                condition: None,
                timeout_ms: None,
                expected_schema_ref: None,
                branch: None,
                depends_on_mode: crate::definition::DependencyMode::All,
                allowed_tools: None,
                blocked_tools: None,
                output_format: crate::definition::StepOutputFormat::Json,
            },
        );
        let spec = FlowSpec {
            description: None,
            steps,
            root: None,
        };
        let error = topological_steps(&spec).expect_err("self-dependency should be rejected");
        assert!(
            error.to_string().contains("self-dependency"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn step_run_status_terminal_classification() {
        assert!(StepRunStatus::Completed.is_terminal());
        assert!(StepRunStatus::Failed.is_terminal());
        assert!(StepRunStatus::Skipped.is_terminal());
        assert!(StepRunStatus::Canceled.is_terminal());
        assert!(!StepRunStatus::Dispatched.is_terminal());
    }
}
