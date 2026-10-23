//! Host store chapter: the obligations of `meerkat_core::host_store`,
//! checked through the six operations only.
//!
//! This file is a placeholder for the chapter's rows; it fails every run
//! until they are written.

use crate::factory::HostStoreFactory;
use crate::failure::{ConformanceFailure, Steps};

const CHAPTER: &str = "host_store";

/// Host store chapter. Placeholder: fails until its rows are written.
pub async fn host_store(factory: &dyn HostStoreFactory) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter(CHAPTER);
    let _store = factory.open().await?;
    Err(steps.fail("rows_written", "the host store rows are not written yet"))
}
