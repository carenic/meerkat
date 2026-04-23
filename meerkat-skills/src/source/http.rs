//! HTTP skill source.
//!
//! Wave-b stub: the real HTTP transport is a wave-c/wave-d rebuild that
//! constructs `SkillKey` directly from the remote listing. The typed
//! surface below is the contract consumers will compile against.

use std::time::Duration;

use meerkat_core::skills::{
    SkillDescriptor, SkillDocument, SkillError, SkillFilter, SkillKey, SkillSource,
};

#[derive(Debug, Clone)]
pub enum HttpSkillAuth {
    Bearer(String),
    Header { name: String, value: String },
}

pub struct HttpSkillSource {
    #[allow(dead_code)]
    source_uuid: String,
    #[allow(dead_code)]
    url: String,
    #[allow(dead_code)]
    auth: Option<HttpSkillAuth>,
    #[allow(dead_code)]
    refresh_interval: Duration,
    #[allow(dead_code)]
    request_timeout: Duration,
}

impl HttpSkillSource {
    pub fn new_with_source_uuid(
        source_uuid: String,
        url: String,
        auth: Option<HttpSkillAuth>,
        refresh_interval: Duration,
        request_timeout: Duration,
    ) -> Self {
        Self {
            source_uuid,
            url,
            auth,
            refresh_interval,
            request_timeout,
        }
    }
}

impl SkillSource for HttpSkillSource {
    async fn list(&self, _filter: &SkillFilter) -> Result<Vec<SkillDescriptor>, SkillError> {
        Ok(Vec::new())
    }

    async fn load(&self, key: &SkillKey) -> Result<SkillDocument, SkillError> {
        Err(SkillError::NotFound { key: key.clone() })
    }
}
