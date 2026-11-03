//! MCP interface (Model Context Protocol) for AI-assisted translation of Susi/Sisulizer projects.
//!
//! Start via `susi-cli mcp [project.slp]`; the server speaks MCP over stdio.

pub mod server;
pub mod session;

use std::path::Path;

use rmcp::ServiceExt;

pub use server::SusiServer;
pub use session::Session;

/// Runs the MCP server on stdin/stdout until the client disconnects.
/// `project` is opened up front if given; `tm` overrides the translation memory file.
pub async fn serve_stdio(project: Option<&Path>, tm: Option<&Path>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut session = Session::new();
    session.tm_path = tm.map(Path::to_path_buf);
    if let Some(p) = project {
        session.open(p)?;
    }
    let service = SusiServer::new(session).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
