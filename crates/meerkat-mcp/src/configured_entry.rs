//! A configured server's entry control on a governed runtime.

use std::sync::Arc;

use meerkat_core::mcp_attachment::{
    ConfiguredMcpAttachmentAuthority, McpAttachmentAuthority, McpAttachmentRefusal,
};
use meerkat_core::mcp_config::{McpScope, McpServerConfig};

/// Carries the governed host's configured-server authority to the exact
/// configured descriptor's physical entry, so it is asked after any
/// preparation wait and before the spawn or first request, not only when the
/// server is enqueued. It never authorizes a configuration write.
pub struct ConfiguredMcpEntry {
    authority: Arc<dyn ConfiguredMcpAttachmentAuthority>,
    server: McpServerConfig,
}

impl ConfiguredMcpEntry {
    pub fn new(
        authority: Arc<dyn ConfiguredMcpAttachmentAuthority>,
        server: McpServerConfig,
    ) -> Arc<Self> {
        Arc::new(Self { authority, server })
    }
}

impl McpAttachmentAuthority for ConfiguredMcpEntry {
    fn server(&self) -> &McpServerConfig {
        &self.server
    }

    fn authorize_apply(&self) -> Result<(), McpAttachmentRefusal> {
        self.authority.authorize_configured(&self.server)
    }

    fn authorize_persist(&self, _scope: McpScope) -> Result<(), McpAttachmentRefusal> {
        Err(McpAttachmentRefusal::Mismatch)
    }
}
