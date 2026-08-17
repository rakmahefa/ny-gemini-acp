//! MCP tool infrastructure.
//!
//! The runtime targets MCP `2026-07-28`: requests are self-describing and
//! stateless at the protocol layer. MCP remains behind the existing
//! `ToolRegistry` surface so builtin and remote tools share one execution
//! contract.

const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
const CLIENT_NAME: &str = "gemini-acp";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGE_COUNT: usize = 10_000;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const CACHE_DEFAULT_TTL: std::time::Duration = std::time::Duration::ZERO;
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

mod catalog;
mod config;
mod protocol;
mod render;
mod transport;

pub use catalog::McpCatalog;
pub use config::{McpError, McpServerConfig, McpToolDescriptor, McpTransportKind};
