//! External skill source protocol (stdio).
//!
//! Wave-b stub: the real implementation is a wave-c/wave-d rebuild that
//! constructs `SkillKey` directly from the remote `source_uuid` +
//! `skill_name` tuple returned by the external protocol. Everything below
//! the trait surface is placeholder scaffolding until the wave-c rebuild
//! lands.

use std::collections::BTreeMap;
use std::path::PathBuf;

use meerkat_core::skills::{
    SkillDescriptor, SkillDocument, SkillError, SkillFilter, SkillKey, SkillSource,
};

/// Client that speaks the external skill-source protocol over stdio.
pub struct StdioExternalClient {
    #[allow(dead_code)]
    command: String,
    #[allow(dead_code)]
    args: Vec<String>,
    #[allow(dead_code)]
    env: BTreeMap<String, String>,
    #[allow(dead_code)]
    cwd: Option<PathBuf>,
    #[allow(dead_code)]
    timeout: std::time::Duration,
}

impl StdioExternalClient {
    pub fn new(
        command: impl Into<String>,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: Option<PathBuf>,
    ) -> Self {
        Self {
            command: command.into(),
            args,
            env,
            cwd,
            timeout: std::time::Duration::from_secs(15),
        }
    }

    pub fn new_with_timeout(
        command: impl Into<String>,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: Option<PathBuf>,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            command: command.into(),
            args,
            env,
            cwd,
            timeout,
        }
    }
}

/// Skill source backed by an external protocol client.
pub struct ExternalSkillSource<C> {
    #[allow(dead_code)]
    client: C,
}

impl<C> ExternalSkillSource<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }
}

impl<C: Send + Sync> SkillSource for ExternalSkillSource<C> {
    async fn list(&self, _filter: &SkillFilter) -> Result<Vec<SkillDescriptor>, SkillError> {
        Ok(Vec::new())
    }

    async fn load(&self, key: &SkillKey) -> Result<SkillDocument, SkillError> {
        Err(SkillError::NotFound { key: key.clone() })
    }
}
