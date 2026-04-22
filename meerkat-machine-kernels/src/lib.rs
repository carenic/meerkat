pub mod generated;
#[cfg(feature = "test-oracle")]
mod runtime;

#[cfg(feature = "test-oracle")]
pub mod test_oracle {
    pub use crate::runtime::{
        GeneratedMachineKernel, KernelEffect, KernelInput, KernelSignal, KernelState, KernelValue,
        TransitionOutcome, TransitionRefusal,
    };
}
