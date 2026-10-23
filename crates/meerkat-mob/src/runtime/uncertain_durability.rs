//! Why an actor of the mob runtime fail-stops on an uncertain durable
//! mutation: that mutation's own error, recorded once.

use std::sync::Arc;

use crate::MobError;

/// The first uncertain durable mutation's own error, once one has made the
/// actor fail-stop. The mob actor's loop exits read it into the mob's
/// [`MobError::DurabilityFailStop`] record, which a status read, a resume
/// and the lifecycle observation report. The mob host actor, which keeps
/// serving, names it in every refusal after it (its cause is the host
/// actor's own error type).
///
/// It is set only through [`Self::record`] and [`Self::record_unreplied`]:
/// the first cause stands, and a later one is only logged.
#[derive(Debug)]
pub(in crate::runtime) struct UncertainDurabilityFailStop<E = MobError> {
    cause: Option<Arc<E>>,
}

impl<E> Default for UncertainDurabilityFailStop<E> {
    fn default() -> Self {
        Self { cause: None }
    }
}

impl UncertainDurabilityFailStop<MobError> {
    /// Fail-stop on `cause`, an uncertain durable mutation's own error, and
    /// return the error for the reply of the command that hit it: `cause`
    /// itself, shared in [`MobError::SharedLifecycleFailure`], so the reply
    /// keeps the cause's Display, wire code and classification.
    #[must_use = "the reply of the command that hit the fail-stop"]
    pub(in crate::runtime) fn record(&mut self, cause: MobError) -> MobError {
        MobError::SharedLifecycleFailure(self.keep(cause))
    }
}

impl<E: std::fmt::Display> UncertainDurabilityFailStop<E> {
    /// Fail-stop on `cause` at a site with no reply to give (or whose reply
    /// is not this error): `cause` is the error it logs.
    pub(in crate::runtime) fn record_unreplied(&mut self, cause: E) {
        self.keep(cause);
    }

    fn keep(&mut self, cause: E) -> Arc<E> {
        let cause = Arc::new(cause);
        match &self.cause {
            None => self.cause = Some(Arc::clone(&cause)),
            Some(first) => tracing::error!(
                first = %first,
                later = %cause,
                "a later durable mutation also stayed uncertain; the first cause stands"
            ),
        }
        cause
    }

    /// Whether the actor must fail-stop.
    pub(in crate::runtime) fn is_set(&self) -> bool {
        self.cause.is_some()
    }

    /// The recorded cause, once the actor must fail-stop.
    pub(in crate::runtime) fn cause(&self) -> Option<&Arc<E>> {
        self.cause.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two fail-stops in one loop turn: the first cause stands, the second
    /// is only logged, and each reply carries its own cause, shared.
    #[test]
    fn the_first_uncertain_mutation_is_the_recorded_cause() {
        let mut fail_stop = UncertainDurabilityFailStop::default();
        assert!(!fail_stop.is_set());
        assert!(fail_stop.cause().is_none());

        let first = fail_stop.record(MobError::WiringError("first".to_string()));
        let recorded = Arc::clone(fail_stop.cause().expect("the first cause is recorded"));
        assert!(fail_stop.is_set());
        assert!(
            matches!(recorded.as_ref(), MobError::WiringError(message) if message == "first"),
            "the cause keeps its own variant: {recorded:?}"
        );
        match &first {
            MobError::SharedLifecycleFailure(replied) => assert!(
                Arc::ptr_eq(replied, &recorded),
                "the reply carries the recorded cause: {first:?}"
            ),
            other => panic!("the reply is the recorded cause, shared: {other:?}"),
        }
        assert_eq!(first.to_string(), recorded.to_string());
        assert_eq!(first.failure_class(), recorded.failure_class());

        let second = fail_stop.record(MobError::Internal("second".to_string()));
        assert!(
            matches!(&second, MobError::SharedLifecycleFailure(replied)
                if matches!(replied.as_ref(), MobError::Internal(message) if message == "second")),
            "a later command's reply carries its own cause: {second:?}"
        );
        fail_stop.record_unreplied(MobError::Internal("third".to_string()));
        assert!(
            Arc::ptr_eq(fail_stop.cause().expect("still recorded"), &recorded),
            "the first cause stands"
        );
    }

    /// The mob host actor's record holds its own error type, and its first
    /// uncertain write stands too.
    #[test]
    fn the_host_record_keeps_its_first_uncertain_write() {
        use crate::runtime::host_actor::MobHostActorError;
        let mut fail_stop = UncertainDurabilityFailStop::<MobHostActorError>::default();
        assert!(!fail_stop.is_set());
        fail_stop.record_unreplied(MobHostActorError::DurableUncertainty {
            detail: "first".to_string(),
        });
        fail_stop.record_unreplied(MobHostActorError::Internal {
            detail: "second".to_string(),
        });
        assert!(fail_stop.is_set());
        assert!(
            matches!(
                fail_stop.cause().map(AsRef::as_ref),
                Some(MobHostActorError::DurableUncertainty { detail }) if detail == "first"
            ),
            "the first uncertain write stands: {:?}",
            fail_stop.cause()
        );
    }
}
