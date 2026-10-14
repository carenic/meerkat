//! The OS process confinement a host applies to the processes its agents
//! launch, resolved once from the effective `[confinement]` selection (the
//! realm's documents, then the operator's flags) and installed on the host's
//! [`crate::AgentFactory`] with [`crate::AgentFactory::with_host_confinement`].
//!
//! Resolution lowers each launch kind's selection into the mechanical
//! [`ExecutionConfinement`]; it authorizes nothing. A launch the platform
//! backend cannot confine is refused at that launch with its typed
//! [`ConfinementRefusal`] and never runs unconfined.

use std::path::{Path, PathBuf};

use meerkat_core::confinement::{
    ConfinementRefusal, ConfinementSpec, ExecutionConfinement, FilesystemAccess, IpNetworkAccess,
    PathAccess, PlatformBaseline,
};
use meerkat_core::{
    ConfinementConfig, ConfinementLaunchKind, LaunchConfinement, WorkspaceConfinement,
    WorkspaceNetwork,
};

/// A host confinement selection that cannot be installed. The host refuses
/// to start rather than run a launch kind it was asked to confine as the
/// trusted host.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HostConfinementError {
    /// A `workspace` selection on a host without a workspace root.
    #[error(
        "{kind} confinement mode `workspace` needs a workspace root; start the host with --context-root <dir>"
    )]
    WorkspaceRootRequired { kind: ConfinementLaunchKind },
    /// The workspace root is not absolute.
    #[error(
        "{kind} confinement mode `workspace` needs an absolute workspace root, not '{}'",
        root.display()
    )]
    RelativeWorkspaceRoot {
        kind: ConfinementLaunchKind,
        root: PathBuf,
    },
    /// The lowered requirement is invalid.
    #[error("{kind} confinement requirement is invalid: {refusal}")]
    InvalidRequirement {
        kind: ConfinementLaunchKind,
        refusal: ConfinementRefusal,
    },
}

/// One required launch kind: its requirement and the working directory of
/// the processes that launch from a fixed directory (command hooks and MCP
/// stdio servers; the shell keeps its own).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RequiredLaunch {
    requirement: ExecutionConfinement,
    working_directory: PathBuf,
}

/// The host's resolved OS process confinement, per launch kind. A kind
/// without a requirement launches as the trusted host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostConfinement {
    shell: Option<RequiredLaunch>,
    command_hooks: Option<RequiredLaunch>,
    mcp_stdio: Option<RequiredLaunch>,
}

impl HostConfinement {
    /// Every launch kind runs as the trusted host: the default.
    #[must_use]
    pub fn trusted_host() -> Self {
        Self::default()
    }

    /// Lower the effective `[confinement]` selection (documents composed,
    /// flags applied) over `workspace_root`, the host's workspace: the shell's
    /// project root, which the `workspace` preset grants and command hooks and
    /// MCP stdio servers start in. A host without one refuses that preset.
    pub fn resolve(
        selection: &ConfinementConfig,
        workspace_root: Option<&Path>,
    ) -> Result<Self, HostConfinementError> {
        let mut resolved = Self::default();
        for kind in ConfinementLaunchKind::ALL {
            let launch = match selection.statement(kind) {
                None | Some(LaunchConfinement::TrustedHost) => None,
                Some(LaunchConfinement::Workspace(workspace)) => {
                    Some(lower_workspace(kind, workspace, workspace_root)?)
                }
                Some(LaunchConfinement::Custom(custom)) => Some(RequiredLaunch {
                    requirement: custom.requirement.clone(),
                    working_directory: custom.working_directory.clone(),
                }),
            };
            *resolved.slot_mut(kind) = launch;
        }
        Ok(resolved)
    }

    fn slot(&self, kind: ConfinementLaunchKind) -> Option<&RequiredLaunch> {
        match kind {
            ConfinementLaunchKind::Shell => self.shell.as_ref(),
            ConfinementLaunchKind::CommandHooks => self.command_hooks.as_ref(),
            ConfinementLaunchKind::McpStdio => self.mcp_stdio.as_ref(),
        }
    }

    fn slot_mut(&mut self, kind: ConfinementLaunchKind) -> &mut Option<RequiredLaunch> {
        match kind {
            ConfinementLaunchKind::Shell => &mut self.shell,
            ConfinementLaunchKind::CommandHooks => &mut self.command_hooks,
            ConfinementLaunchKind::McpStdio => &mut self.mcp_stdio,
        }
    }

    /// Whether every launch kind runs as the trusted host.
    #[must_use]
    pub fn is_trusted_host(&self) -> bool {
        self == &Self::default()
    }

    /// The requirement installed for `kind`, `None` for the trusted host.
    #[must_use]
    pub fn requirement(&self, kind: ConfinementLaunchKind) -> Option<&ExecutionConfinement> {
        self.slot(kind).map(|launch| &launch.requirement)
    }

    /// The fixed working directory of `kind`'s confined launches (command
    /// hooks and MCP stdio servers), `None` for the trusted host.
    #[must_use]
    pub fn working_directory(&self, kind: ConfinementLaunchKind) -> Option<&Path> {
        self.slot(kind)
            .map(|launch| launch.working_directory.as_path())
    }

    /// Check now whether this platform's backend can enforce each required
    /// launch kind, compiling each distinct requirement once. This is setup
    /// support only, for the host's startup report: each launch is still
    /// checked and refused on its own, and a refusal here changes nothing.
    #[must_use]
    pub fn preflight(&self) -> Vec<(ConfinementLaunchKind, ConfinementRefusal)> {
        let mut compiled: Vec<(&ExecutionConfinement, Option<ConfinementRefusal>)> = Vec::new();
        let mut refusals = Vec::new();
        for kind in ConfinementLaunchKind::ALL {
            let Some(requirement) = self.requirement(kind) else {
                continue;
            };
            let refusal = match compiled.iter().find(|(seen, _)| *seen == requirement) {
                Some((_, refusal)) => *refusal,
                None => {
                    let refusal = meerkat_sandbox::CompiledConfinement::compile(requirement).err();
                    compiled.push((requirement, refusal));
                    refusal
                }
            };
            if let Some(refusal) = refusal {
                refusals.push((kind, refusal));
            }
        }
        refusals
    }

    /// Report this host's confinement at startup in its log: each launch
    /// kind's mode, and a warning for each required kind this platform's
    /// backend refuses, whose every launch is then refused.
    pub fn log_startup(&self) {
        if self.is_trusted_host() {
            return;
        }
        tracing::info!(confinement = %self, "OS process confinement installed");
        for (kind, refusal) in self.preflight() {
            tracing::warn!(
                %kind,
                %refusal,
                "confinement cannot be enforced on this host; every launch of this kind will be refused"
            );
        }
    }
}

impl std::fmt::Display for HostConfinement {
    /// One line naming each launch kind's mode, for a host's startup report.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, kind) in ConfinementLaunchKind::ALL.into_iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            let mode = if self.slot(kind).is_some() {
                "required"
            } else {
                "trusted host"
            };
            write!(f, "{kind}: {mode}")?;
        }
        Ok(())
    }
}

/// The workspace preset over `root`: read and write the root and the
/// `write` subtrees, read the `read` subtrees, the platform baseline, and IP
/// sockets only when the network is unrestricted. A subtree that another
/// grant of the same or a stronger class already covers (the root, a
/// writable subtree, or a listed subtree containing it) is not granted
/// again, so the preset adds no nested grant of its own.
fn lower_workspace(
    kind: ConfinementLaunchKind,
    workspace: &WorkspaceConfinement,
    root: Option<&Path>,
) -> Result<RequiredLaunch, HostConfinementError> {
    let root = root.ok_or(HostConfinementError::WorkspaceRootRequired { kind })?;
    if !root.is_absolute() {
        return Err(HostConfinementError::RelativeWorkspaceRoot {
            kind,
            root: root.to_path_buf(),
        });
    }
    let writable = outermost_subtrees(
        std::iter::once(root.to_path_buf()).chain(workspace.write.iter().cloned()),
    );
    let mut readable = writable.clone();
    readable.extend(outermost_subtrees(
        workspace
            .read
            .iter()
            .filter(|path| !writable.iter().any(|granted| path.starts_with(granted)))
            .cloned(),
    ));
    let subtrees = |paths: Vec<PathBuf>| paths.into_iter().map(PathAccess::Subtree).collect();
    let requirement = ConfinementSpec {
        baseline: PlatformBaseline::CommandRuntimeV1,
        read: FilesystemAccess::Paths(subtrees(readable)),
        write: FilesystemAccess::Paths(subtrees(writable)),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
        network: match workspace.network {
            WorkspaceNetwork::Denied => IpNetworkAccess::Denied,
            WorkspaceNetwork::Unrestricted => IpNetworkAccess::Unrestricted,
        },
        unix_connect: Vec::new(),
        require_descendant_termination: false,
    }
    .try_into()
    .map_err(|refusal| HostConfinementError::InvalidRequirement { kind, refusal })?;
    Ok(RequiredLaunch {
        requirement,
        working_directory: root.to_path_buf(),
    })
}

/// `paths` once each, without a path that another listed path contains, in
/// the order each kept path was first listed.
fn outermost_subtrees(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in paths {
        if kept.iter().any(|outer| path.starts_with(outer)) {
            continue;
        }
        kept.retain(|inner| !inner.starts_with(&path));
        kept.push(path);
    }
    kept
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use meerkat_core::CustomConfinement;

    fn workspace_spec(read: &[&str], write: &[&str], network: IpNetworkAccess) -> ConfinementSpec {
        let subtrees = |paths: &[&str]| {
            paths
                .iter()
                .map(|path| PathAccess::Subtree(PathBuf::from(path)))
                .collect()
        };
        ConfinementSpec {
            baseline: PlatformBaseline::CommandRuntimeV1,
            read: FilesystemAccess::Paths(subtrees(read)),
            write: FilesystemAccess::Paths(subtrees(write)),
            deny_read: vec![],
            deny_write: vec![],
            network,
            unix_connect: vec![],
            require_descendant_termination: false,
        }
    }

    #[test]
    fn no_selection_is_the_trusted_host() {
        let resolved =
            HostConfinement::resolve(&ConfinementConfig::default(), Some(Path::new("/work")))
                .unwrap();
        assert!(resolved.is_trusted_host());
        assert_eq!(resolved, HostConfinement::trusted_host());
        assert!(resolved.preflight().is_empty());
        let explicit = ConfinementConfig::for_all(LaunchConfinement::TrustedHost);
        assert!(
            HostConfinement::resolve(&explicit, None)
                .unwrap()
                .is_trusted_host()
        );
    }

    #[test]
    fn workspace_lowers_to_the_root_and_extra_grants_for_every_kind() {
        let selection =
            ConfinementConfig::for_all(LaunchConfinement::Workspace(WorkspaceConfinement {
                network: WorkspaceNetwork::Unrestricted,
                read: vec!["/opt/tool".into(), "/work/docs".into(), "/opt/tool".into()],
                write: vec!["/tmp/build".into(), "/work/target".into()],
            }));
        let resolved = HostConfinement::resolve(&selection, Some(Path::new("/work"))).unwrap();
        let expected: ExecutionConfinement = workspace_spec(
            &["/work", "/tmp/build", "/opt/tool"],
            &["/work", "/tmp/build"],
            IpNetworkAccess::Unrestricted,
        )
        .try_into()
        .unwrap();
        for kind in ConfinementLaunchKind::ALL {
            assert_eq!(resolved.requirement(kind), Some(&expected), "{kind}");
            assert_eq!(
                resolved.working_directory(kind),
                Some(Path::new("/work")),
                "{kind}"
            );
        }
    }

    #[test]
    fn workspace_grants_each_subtree_once_without_nesting() {
        let selection =
            ConfinementConfig::for_all(LaunchConfinement::Workspace(WorkspaceConfinement {
                network: WorkspaceNetwork::Denied,
                read: vec![
                    "/tmp/build/cache".into(),
                    "/opt/tool/bin".into(),
                    "/opt/tool".into(),
                    "/srv/data".into(),
                ],
                write: vec![
                    "/tmp/build/out".into(),
                    "/tmp/build".into(),
                    "/work/target".into(),
                ],
            }));
        let resolved = HostConfinement::resolve(&selection, Some(Path::new("/work"))).unwrap();
        let expected: ExecutionConfinement = workspace_spec(
            &["/work", "/tmp/build", "/opt/tool", "/srv/data"],
            &["/work", "/tmp/build"],
            IpNetworkAccess::Denied,
        )
        .try_into()
        .unwrap();
        assert_eq!(
            resolved.requirement(ConfinementLaunchKind::Shell),
            Some(&expected)
        );
    }

    #[test]
    fn workspace_defaults_to_the_root_alone_with_no_network() {
        let selection = ConfinementConfig::for_all(LaunchConfinement::Workspace(
            WorkspaceConfinement::default(),
        ));
        let resolved = HostConfinement::resolve(&selection, Some(Path::new("/work"))).unwrap();
        let expected: ExecutionConfinement =
            workspace_spec(&["/work"], &["/work"], IpNetworkAccess::Denied)
                .try_into()
                .unwrap();
        assert_eq!(
            resolved.requirement(ConfinementLaunchKind::Shell),
            Some(&expected)
        );
    }

    #[test]
    fn workspace_without_an_absolute_root_is_refused_not_unconfined() {
        let selection = ConfinementConfig::for_all(LaunchConfinement::Workspace(
            WorkspaceConfinement::default(),
        ));
        assert_eq!(
            HostConfinement::resolve(&selection, None),
            Err(HostConfinementError::WorkspaceRootRequired {
                kind: ConfinementLaunchKind::Shell
            })
        );
        assert!(matches!(
            HostConfinement::resolve(&selection, Some(Path::new("relative"))),
            Err(HostConfinementError::RelativeWorkspaceRoot { .. })
        ));
    }

    #[test]
    fn custom_and_per_kind_selections_pass_through_exactly() {
        let requirement: ExecutionConfinement =
            workspace_spec(&["/srv/mcp"], &[], IpNetworkAccess::Denied)
                .try_into()
                .unwrap();
        let selection = ConfinementConfig {
            mcp_stdio: Some(LaunchConfinement::Custom(CustomConfinement {
                working_directory: "/srv/mcp".into(),
                requirement: requirement.clone(),
            })),
            command_hooks: Some(LaunchConfinement::TrustedHost),
            ..ConfinementConfig::default()
        };
        // A custom selection needs no workspace root.
        let resolved = HostConfinement::resolve(&selection, None).unwrap();
        assert_eq!(
            resolved.requirement(ConfinementLaunchKind::McpStdio),
            Some(&requirement)
        );
        assert_eq!(
            resolved.working_directory(ConfinementLaunchKind::McpStdio),
            Some(Path::new("/srv/mcp"))
        );
        assert_eq!(resolved.requirement(ConfinementLaunchKind::Shell), None);
        assert_eq!(
            resolved.requirement(ConfinementLaunchKind::CommandHooks),
            None
        );
        assert_eq!(
            resolved.to_string(),
            "shell: trusted host, command_hooks: trusted host, mcp_stdio: required"
        );
    }

    #[test]
    fn preflight_reports_a_requirement_no_backend_enforces() {
        // Descendant termination is unsupported on every platform backend.
        let mut spec = workspace_spec(&["/work"], &["/work"], IpNetworkAccess::Denied);
        spec.require_descendant_termination = true;
        let selection = ConfinementConfig::for_all(LaunchConfinement::Custom(CustomConfinement {
            working_directory: "/work".into(),
            requirement: spec.try_into().unwrap(),
        }));
        let resolved = HostConfinement::resolve(&selection, None).unwrap();
        assert_eq!(
            resolved.preflight(),
            ConfinementLaunchKind::ALL
                .into_iter()
                .map(|kind| (kind, ConfinementRefusal::UnsupportedRequirement))
                .collect::<Vec<_>>()
        );
    }
}
