//! Skill repository resolution.
//!
//! Wave-b stub: the real "config → CompositeSkillSource" pipeline needs the
//! out-of-allowlist consumer rewrite in wave-c. What remains here are the
//! public entry points in the shape the facade currently imports — every
//! configured repository yields a typed `SkillKey`-backed source.

use std::path::Path;

use meerkat_core::skills::{SkillError, SkillScope};
use meerkat_core::skills_config::{SkillRepoTransport, SkillsConfig};

use crate::source::composite::NamedSource;
use crate::source::{CompositeSkillSource, FilesystemSkillSource, SourceNode};

pub async fn resolve_repositories(
    config: &SkillsConfig,
    project_root: Option<&Path>,
) -> Result<Option<CompositeSkillSource>, SkillError> {
    let user_root = std::env::var_os("HOME").map(std::path::PathBuf::from);
    resolve_repositories_with_roots(config, project_root, user_root.as_deref(), project_root).await
}

pub async fn resolve_repositories_with_roots(
    config: &SkillsConfig,
    context_root: Option<&Path>,
    _user_root: Option<&Path>,
    cache_root: Option<&Path>,
) -> Result<Option<CompositeSkillSource>, SkillError> {
    if !config.enabled {
        return Ok(None);
    }

    let mut sources: Vec<NamedSource> = Vec::new();
    for repo in &config.repositories {
        match &repo.transport {
            SkillRepoTransport::Filesystem { path } => {
                let resolution_root = context_root
                    .or(cache_root)
                    .unwrap_or_else(|| Path::new("."));
                let full_path = if Path::new(path).is_relative() {
                    resolution_root.join(path)
                } else {
                    path.into()
                };
                sources.push(NamedSource {
                    name: repo.name.clone(),
                    source: SourceNode::Filesystem(FilesystemSkillSource::new_with_identity(
                        full_path,
                        SkillScope::Project,
                        repo.source_uuid.clone(),
                        config.health_thresholds,
                    )),
                });
            }
            SkillRepoTransport::Http { .. }
            | SkillRepoTransport::Stdio { .. }
            | SkillRepoTransport::Git { .. } => {
                // Wave-b stub: non-filesystem transports are rewired in wave-c.
                tracing::warn!(
                    repo = %repo.name,
                    "non-filesystem skill repos are wave-c-gated"
                );
            }
        }
    }

    if sources.is_empty()
        && let Some(root) = context_root
    {
        let default_project_skills = root.join(".rkat/skills");
        if default_project_skills.is_dir() {
            sources.push(NamedSource {
                name: "project".to_string(),
                source: SourceNode::Filesystem(FilesystemSkillSource::new(
                    default_project_skills,
                    SkillScope::Project,
                )),
            });
        }
    }

    Ok(Some(CompositeSkillSource::from_named(sources)))
}
