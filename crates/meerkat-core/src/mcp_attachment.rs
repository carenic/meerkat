//! Present native authority for governed MCP server attachment.
//!
//! On a runtime with an installed native work host, an MCP server is attached
//! to a session (live), written to configuration (persisted) or connected for a
//! configured session only through a prepared, authenticated attachment control
//! owned by that host. These traits are the surface-facing view of such a
//! control: the surfaces ask it again at each effect boundary, after any wait,
//! and never infer permission from a saved `mcp.toml` row, a server name or an
//! earlier permit. Ungoverned runtimes do not use them.

use crate::mcp_config::{McpScope, McpServerConfig};

/// Why a governed attachment effect was refused. Local and typed: no variant
/// touches the session, its runs or another realm.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpAttachmentRefusal {
    /// The current mandate, grant or account decision does not allow it.
    #[error("the attachment is not permitted")]
    Denied,
    /// The authority needed to decide is not available now.
    #[error("attachment authority is unavailable")]
    Unavailable,
    /// The prepared attachment no longer matches the current session,
    /// destination or executor attachment.
    #[error("the prepared attachment is no longer current")]
    NotCurrent,
    /// The current decision requires an operation review this control does
    /// not carry.
    #[error("the attachment requires a review")]
    ReviewRequired,
    /// The control does not cover this exact server descriptor or effect.
    #[error("the attachment control does not cover this server or effect")]
    Mismatch,
}

/// One prepared, authenticated attachment control for an exact server
/// descriptor. Each call rechecks the current mandate, grant/account decision,
/// review requirement and native attachment; a success is consumed by that one
/// effect and is not permission for another.
pub trait McpAttachmentAuthority: Send + Sync {
    /// The exact server descriptor this control was prepared for.
    fn server(&self) -> &McpServerConfig;

    /// Recheck at the live apply boundary, before the server is discovered,
    /// spawned or its tools become visible to the session.
    fn authorize_apply(&self) -> Result<(), McpAttachmentRefusal>;

    /// Recheck before the descriptor is written to configuration in `scope`.
    /// The current native configuration owner decides for the exact realm,
    /// descriptor and effect; a live-session permit alone is not enough.
    fn authorize_persist(&self, scope: McpScope) -> Result<(), McpAttachmentRefusal>;
}

/// Installed by a governed host composition: authorizes each configured
/// server for a fresh session before discovery, spawn or first use. The
/// configured row is configuration, never present authority.
pub trait ConfiguredMcpAttachmentAuthority: Send + Sync {
    fn authorize_configured(&self, server: &McpServerConfig) -> Result<(), McpAttachmentRefusal>;
}

/// The truthful outcome of one governed live MCP add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpAttachmentOutcome {
    /// The descriptor was written to configuration.
    pub persisted: bool,
    /// The add was staged for the session's next boundary.
    pub staged: bool,
}

/// One reviewed MCP server to attach to the current member live: the exact
/// descriptor, the native prepared control for it (never inferred from a
/// session id or an account review), and, separately, the configuration
/// owner to persist it under. The persisted write is its own effect, decided
/// by the control under the config lock; the live add is staged with the
/// control, which travels to the connection's physical entry.
pub struct LiveMcpAttachment {
    pub control: std::sync::Arc<dyn McpAttachmentAuthority>,
    pub server: McpServerConfig,
    pub persist: Option<crate::mcp_config::McpConfigMutationAuthority>,
}

impl std::fmt::Debug for LiveMcpAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveMcpAttachment")
            .field("server", &self.server.name)
            .field(
                "persist",
                &self.persist.as_ref().map(|persist| &persist.scope),
            )
            .finish_non_exhaustive()
    }
}

/// Why a live MCP attachment to the current member did not stage.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiveMcpAttachError {
    /// The control, the current member or the configuration owner refused.
    #[error("MCP attachment refused: {0}")]
    Refused(McpAttachmentRefusal),
    /// The member's turn boundary is held (this proves a held boundary, not
    /// necessarily an active turn): nothing was attached and nothing waited.
    /// `Busy` grants no permission, and whether to try again is for the
    /// caller's owner to decide; it is neither an instruction to retry nor a
    /// promise of automatic retry. Current-turn attachment is not
    /// implemented: an in-turn caller always gets `Busy`.
    #[error("the member's turn boundary is held; nothing was attached")]
    Busy,
    /// This member has no live MCP attachment surface (it was not built by a
    /// governed session-owned construction).
    #[error("this member has no live MCP attachment surface")]
    Unsupported,
    /// Writing the server to configuration failed.
    #[error("{0}")]
    Persist(String),
    /// Staging or applying the server failed (a persisted write is reported
    /// separately and is not undone by a later live failure).
    #[error("{0}")]
    Stage(String),
}

/// The live MCP attachment surface of one built member: its construction's
/// own router, which the member's actor mutates between turns.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait LiveMcpAttachmentSurface: Send + Sync {
    /// Stage `attachment` on this member's router and apply it now (the
    /// actor serializes this with turns). Reports the persisted and staged
    /// effects separately and returns the exact attempt to observe; it makes
    /// no connection-success claim.
    async fn attach(
        &self,
        attachment: LiveMcpAttachment,
    ) -> Result<LiveMcpAttachmentReceipt, LiveMcpAttachError>;
}

/// What one live attachment did, and the exact attempt it started.
#[derive(Debug, Clone)]
pub struct LiveMcpAttachmentReceipt {
    pub outcome: McpAttachmentOutcome,
    pub attempt: McpAttachmentAttempt,
}

/// Where the exact attempt of one live attachment stands now, for one tool
/// of its server, from the attaching router's own accepted
/// outcome of that attempt. A local observation, never a permit: it extends
/// no authority, reserves nothing and proves no cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpAttachmentReadiness {
    /// Staged or connecting; the router has accepted no outcome yet. A wait
    /// deadline returns `Pending`: not settled yet, not a failure.
    Pending,
    /// The attempt's connection is installed and is the server's current
    /// connection, and its published routes serve the tool.
    Usable { connection_generation: u64 },
    /// The attempt's connection is installed and current, but no published
    /// route serves the tool on it.
    ToolNotRouted { connection_generation: u64 },
    /// The router refused the attempt's apply boundary.
    Rejected { detail: String },
    /// The attempt's connection failed, or its install was refused.
    Failed { detail: String },
    /// A newer intent for the server replaced the attempt, the router
    /// refused its result as stale, or its connection is no longer the
    /// server's current one.
    Superseded,
    /// The attaching router is gone (taken for shutdown, or dropped). This
    /// is not proof that its shutdown, or any process close, finished.
    Retired,
    /// The member actor the attempt was attached through is no longer the
    /// current one.
    NotCurrent,
}

/// Observes one exact attachment attempt.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait McpAttachmentAttemptObserver: Send + Sync {
    /// The attempt's readiness for `tool` now, from one read of its
    /// resolution and, once installed, of the router's current connection
    /// and routes. It neither drives progress nor waits for the attempt, but
    /// it has no deadline: its reads can wait behind a writer of the router.
    /// Use `wait` for a deadline-bounded answer.
    async fn observe(&self, tool: &str) -> McpAttachmentReadiness;

    /// Wait at most `timeout` for the attempt to leave `Pending`, driving
    /// the attaching router's own progress, and return its readiness for
    /// `tool`. One deadline bounds every step, lock acquisition included.
    /// The timeout bounds the observation only: it cancels nothing, and
    /// `Pending` after it is a local not-ready answer.
    async fn wait(&self, tool: &str, timeout: std::time::Duration) -> McpAttachmentReadiness;
}

/// The exact attempt one live attachment started on its member's router.
#[derive(Clone)]
pub struct McpAttachmentAttempt {
    server: String,
    observer: std::sync::Arc<dyn McpAttachmentAttemptObserver>,
}

impl McpAttachmentAttempt {
    pub fn new(
        server: impl Into<String>,
        observer: std::sync::Arc<dyn McpAttachmentAttemptObserver>,
    ) -> Self {
        Self {
            server: server.into(),
            observer,
        }
    }

    /// The attached server's name.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// The same attempt observed through `wrap` (an owner that adds its own
    /// currentness check around every observation).
    #[must_use]
    pub fn observed_through(
        self,
        wrap: impl FnOnce(
            std::sync::Arc<dyn McpAttachmentAttemptObserver>,
        ) -> std::sync::Arc<dyn McpAttachmentAttemptObserver>,
    ) -> Self {
        Self {
            server: self.server,
            observer: wrap(self.observer),
        }
    }

    /// See [`McpAttachmentAttemptObserver::observe`].
    pub async fn observe(&self, tool: &str) -> McpAttachmentReadiness {
        self.observer.observe(tool).await
    }

    /// See [`McpAttachmentAttemptObserver::wait`].
    pub async fn wait(&self, tool: &str, timeout: std::time::Duration) -> McpAttachmentReadiness {
        self.observer.wait(tool, timeout).await
    }
}

impl std::fmt::Debug for McpAttachmentAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAttachmentAttempt")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}
