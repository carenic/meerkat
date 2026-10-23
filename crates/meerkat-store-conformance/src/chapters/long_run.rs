//! Long run and durable worker chapters. Placeholder: both fail until their
//! harness and rows are written.

use crate::failure::{ConformanceFailure, Steps};

/// Long run chapter. Placeholder: fails until its rows are written.
pub async fn long_run() -> Result<(), ConformanceFailure> {
    Err(Steps::chapter("long_run").fail("rows_written", "the long run rows are not written yet"))
}

/// Durable worker chapter. Placeholder: fails until its rows are written.
pub async fn durable_worker() -> Result<(), ConformanceFailure> {
    Err(Steps::chapter("durable_worker").fail(
        "rows_written",
        "the durable worker rows are not written yet",
    ))
}
