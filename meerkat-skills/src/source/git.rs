//! Git-backed skill source.
//!
//! Wave-b stub: the real git-clone-and-parse path is a wave-c/wave-d
//! rebuild. The typed surface below (GitRef, GitSkillAuth, GitSkillConfig,
//! GitSkillSource) keeps the public shape consumers import.

use std::path::PathBuf;
use std::time::Duration;

use meerkat_core::skills::{
    SkillDescriptor, SkillDocument, SkillError, SkillFilter, SkillKey, SkillSource,
    SourceHealthThresholds, SourceUuid,
};

#[derive(Debug, Clone)]
pub enum GitRef {
    Branch(String),
    Tag(String),
    Commit(String),
}

#[derive(Debug, Clone)]
pub enum GitSkillAuth {
    HttpsToken(String),
}

#[derive(Debug, Clone)]
pub struct GitSkillConfig {
    pub repo_url: String,
    pub git_ref: GitRef,
    pub cache_dir: PathBuf,
    pub skills_root: Option<String>,
    pub refresh_interval: Duration,
    pub auth: Option<GitSkillAuth>,
    pub depth: Option<usize>,
    pub source_uuid: SourceUuid,
    pub health_thresholds: SourceHealthThresholds,
}

pub struct GitSkillSource {
    #[allow(dead_code)]
    config: GitSkillConfig,
}

impl GitSkillSource {
    pub fn new(config: GitSkillConfig) -> Self {
        Self { config }
    }
}

impl SkillSource for GitSkillSource {
    async fn list(&self, _filter: &SkillFilter) -> Result<Vec<SkillDescriptor>, SkillError> {
        Ok(Vec::new())
    }

    async fn load(&self, key: &SkillKey) -> Result<SkillDocument, SkillError> {
        Err(SkillError::NotFound { key: key.clone() })
    }
}

#[cfg(test)]
pub mod tests_support {
    use std::path::Path;

    pub async fn init_test_repo(_repo: &Path, _work: &Path) -> std::io::Result<()> {
        Ok(())
    }
}
