//! Embedder-facing MCP helpers.
//!
//! The preferred path for attaching MCP servers to factory-built agents is
//! declarative: put [`meerkat_core::mcp_config::McpServerConfig`] entries in
//! `SessionBuildOptions::mcp_servers` / `AgentBuildConfig::mcp_servers` and
//! the factory materializes a session-owned router bound to the build mode's
//! canonical external-tool surface authority (composing with builtins and
//! surviving mob member revival via the durable profile).
//!
//! For hosts that drive a raw [`McpRouter`] themselves (outside a factory
//! build), [`standalone_router`] is the supported constructor:
//! `McpRouter::new()`'s default unbound surface handle deliberately
//! fail-closes `stage_add`, so standalone use needs an ephemeral runtime
//! surface authority — previously an incantation that only existed in test
//! code.

use std::sync::Arc;

use meerkat_mcp::{McpRouter, McpStdioLaunchProfile};
use meerkat_runtime::handles::RuntimeExternalToolSurfaceHandle;

/// Construct a standalone [`McpRouter`] backed by an ephemeral external-tool
/// surface authority.
///
/// Use this when driving MCP servers OUTSIDE a factory-built session (e.g. a
/// host-side router shared across spawns). The returned router accepts
/// `stage_add`/`apply_staged` immediately. Note the surface authority is
/// process-local and ephemeral: sessions built through the factory should
/// prefer the declarative `mcp_servers` build option, which binds the router
/// to the session's own machine authority instead.
///
/// Local stdio servers launch as the trusted host. A host with a launch
/// boundary uses [`standalone_router_with_stdio_profile`].
#[must_use]
pub fn standalone_router() -> McpRouter {
    standalone_router_with_stdio_profile(McpStdioLaunchProfile::trusted_host())
}

/// [`standalone_router`] retaining the host's local MCP launch boundary
/// (canonically [`crate::AgentFactory::mcp_stdio_launch_profile`]) for every
/// stdio server it launches, as a factory-built router does.
#[must_use]
pub fn standalone_router_with_stdio_profile(profile: McpStdioLaunchProfile) -> McpRouter {
    McpRouter::new_with_surface_handle_and_stdio_profile(
        Arc::new(RuntimeExternalToolSurfaceHandle::ephemeral()),
        profile,
    )
}
