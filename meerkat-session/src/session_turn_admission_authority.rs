use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTurnAdmissionPhase {
    Idle,
    Admitted,
    Running,
    Completing,
    ShuttingDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTurnAdmissionInput {
    RequestStartTurn,
    AbortAdmittedTurn,
    BeginRun,
    ResolveRun,
    FinalizeTurn,
    RequestInterrupt,
    RequestShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTurnAdmissionEffect {
    WakeInterrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTurnAdmissionTransition {
    pub from_phase: SessionTurnAdmissionPhase,
    pub next_phase: SessionTurnAdmissionPhase,
    pub effects: Vec<SessionTurnAdmissionEffect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionTurnAdmissionFields {
    interrupt_pending: bool,
    shutdown_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTurnAdmissionError {
    pub from: SessionTurnAdmissionPhase,
    pub input: SessionTurnAdmissionInput,
}

impl fmt::Display for SessionTurnAdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "illegal session turn admission transition: {:?} in phase {:?}",
            self.input, self.from
        )
    }
}

impl std::error::Error for SessionTurnAdmissionError {}

#[derive(Debug, Clone)]
pub struct SessionTurnAdmissionAuthority {
    phase: SessionTurnAdmissionPhase,
    fields: SessionTurnAdmissionFields,
}

impl SessionTurnAdmissionAuthority {
    pub fn new() -> Self {
        Self {
            phase: SessionTurnAdmissionPhase::Idle,
            fields: SessionTurnAdmissionFields {
                interrupt_pending: false,
                shutdown_pending: false,
            },
        }
    }

    pub fn phase(&self) -> SessionTurnAdmissionPhase {
        self.phase
    }

    pub fn interrupt_pending(&self) -> bool {
        self.fields.interrupt_pending
    }

    pub fn shutdown_pending(&self) -> bool {
        self.fields.shutdown_pending
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            SessionTurnAdmissionPhase::Admitted
                | SessionTurnAdmissionPhase::Running
                | SessionTurnAdmissionPhase::Completing
        )
    }

    pub fn apply(
        &mut self,
        input: SessionTurnAdmissionInput,
    ) -> Result<SessionTurnAdmissionTransition, SessionTurnAdmissionError> {
        let from_phase = self.phase;
        let mut fields = self.fields;
        let mut effects = Vec::new();
        let next_phase = match (self.phase, input) {
            (SessionTurnAdmissionPhase::Idle, SessionTurnAdmissionInput::RequestStartTurn) => {
                fields.interrupt_pending = false;
                fields.shutdown_pending = false;
                SessionTurnAdmissionPhase::Admitted
            }
            (SessionTurnAdmissionPhase::Admitted, SessionTurnAdmissionInput::AbortAdmittedTurn) => {
                fields.interrupt_pending = false;
                fields.shutdown_pending = false;
                SessionTurnAdmissionPhase::Idle
            }
            (SessionTurnAdmissionPhase::Admitted, SessionTurnAdmissionInput::BeginRun) => {
                SessionTurnAdmissionPhase::Running
            }
            (SessionTurnAdmissionPhase::Admitted, SessionTurnAdmissionInput::RequestShutdown) => {
                fields.interrupt_pending = false;
                fields.shutdown_pending = true;
                SessionTurnAdmissionPhase::ShuttingDown
            }
            (SessionTurnAdmissionPhase::Running, SessionTurnAdmissionInput::ResolveRun) => {
                SessionTurnAdmissionPhase::Completing
            }
            (SessionTurnAdmissionPhase::Running, SessionTurnAdmissionInput::RequestInterrupt) => {
                fields.interrupt_pending = true;
                effects.push(SessionTurnAdmissionEffect::WakeInterrupt);
                SessionTurnAdmissionPhase::Running
            }
            (SessionTurnAdmissionPhase::Running, SessionTurnAdmissionInput::RequestShutdown) => {
                fields.shutdown_pending = true;
                SessionTurnAdmissionPhase::Running
            }
            (SessionTurnAdmissionPhase::Completing, SessionTurnAdmissionInput::RequestShutdown) => {
                fields.shutdown_pending = true;
                SessionTurnAdmissionPhase::Completing
            }
            (SessionTurnAdmissionPhase::Completing, SessionTurnAdmissionInput::FinalizeTurn) => {
                fields.interrupt_pending = false;
                if fields.shutdown_pending {
                    SessionTurnAdmissionPhase::ShuttingDown
                } else {
                    fields.shutdown_pending = false;
                    SessionTurnAdmissionPhase::Idle
                }
            }
            (SessionTurnAdmissionPhase::Idle, SessionTurnAdmissionInput::RequestShutdown) => {
                fields.interrupt_pending = false;
                fields.shutdown_pending = true;
                SessionTurnAdmissionPhase::ShuttingDown
            }
            _ => {
                return Err(SessionTurnAdmissionError {
                    from: self.phase,
                    input,
                });
            }
        };

        self.phase = next_phase;
        self.fields = fields;
        Ok(SessionTurnAdmissionTransition {
            from_phase,
            next_phase,
            effects,
        })
    }
}

impl Default for SessionTurnAdmissionAuthority {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn start_turn_claims_slot() {
        let mut auth = SessionTurnAdmissionAuthority::new();
        let transition = auth
            .apply(SessionTurnAdmissionInput::RequestStartTurn)
            .expect("idle session should admit a turn");
        assert_eq!(transition.next_phase, SessionTurnAdmissionPhase::Admitted);
        assert!(auth.is_active());
    }

    #[test]
    fn interrupt_only_allowed_while_running() {
        let mut auth = SessionTurnAdmissionAuthority::new();
        let err = auth
            .apply(SessionTurnAdmissionInput::RequestInterrupt)
            .expect_err("idle session cannot be interrupted");
        assert_eq!(err.from, SessionTurnAdmissionPhase::Idle);

        auth.apply(SessionTurnAdmissionInput::RequestStartTurn)
            .unwrap();
        auth.apply(SessionTurnAdmissionInput::BeginRun).unwrap();
        let transition = auth
            .apply(SessionTurnAdmissionInput::RequestInterrupt)
            .expect("running session should accept interrupt");
        assert_eq!(transition.next_phase, SessionTurnAdmissionPhase::Running);
        assert_eq!(
            transition.effects,
            vec![SessionTurnAdmissionEffect::WakeInterrupt]
        );
        assert!(auth.interrupt_pending());
    }

    #[test]
    fn shutdown_gracefully_drains_running_turn() {
        let mut auth = SessionTurnAdmissionAuthority::new();
        auth.apply(SessionTurnAdmissionInput::RequestStartTurn)
            .unwrap();
        auth.apply(SessionTurnAdmissionInput::BeginRun).unwrap();
        auth.apply(SessionTurnAdmissionInput::RequestShutdown)
            .unwrap();
        assert_eq!(auth.phase(), SessionTurnAdmissionPhase::Running);
        assert!(auth.shutdown_pending());

        auth.apply(SessionTurnAdmissionInput::ResolveRun).unwrap();
        let transition = auth
            .apply(SessionTurnAdmissionInput::FinalizeTurn)
            .expect("finalize should enter shutting down");
        assert_eq!(
            transition.next_phase,
            SessionTurnAdmissionPhase::ShuttingDown
        );
    }

    #[test]
    fn shutdown_cancels_admitted_before_run() {
        let mut auth = SessionTurnAdmissionAuthority::new();
        auth.apply(SessionTurnAdmissionInput::RequestStartTurn)
            .unwrap();
        let transition = auth
            .apply(SessionTurnAdmissionInput::RequestShutdown)
            .expect("admitted turn should be shut down before run");
        assert_eq!(
            transition.next_phase,
            SessionTurnAdmissionPhase::ShuttingDown
        );
    }
}
