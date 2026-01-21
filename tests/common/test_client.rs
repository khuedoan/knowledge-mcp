//! Test client handler for integration tests.
//!
//! Provides a configurable MCP client that can:
//! - Declare elicitation capability
//! - Respond to elicitation requests with configurable actions

use rmcp::{ClientHandler, ErrorData, RoleClient, model::*, service::RequestContext};
use serde_json::json;

/// A test MCP client handler with configurable elicitation behavior.
#[derive(Clone)]
pub struct TestClientHandler {
    /// How to respond to elicitation requests.
    pub elicitation_response: ElicitationAction,
    /// Whether to declare elicitation capability.
    pub supports_elicitation: bool,
}

impl TestClientHandler {
    /// Create a new test client that supports elicitation and accepts requests.
    pub fn new() -> Self {
        Self {
            elicitation_response: ElicitationAction::Accept,
            supports_elicitation: true,
        }
    }

    /// Create a test client that declines elicitation requests.
    pub fn declining() -> Self {
        Self {
            elicitation_response: ElicitationAction::Decline,
            supports_elicitation: true,
        }
    }

    /// Create a test client without elicitation support.
    pub fn without_elicitation() -> Self {
        Self {
            elicitation_response: ElicitationAction::Decline,
            supports_elicitation: false,
        }
    }
}

impl Default for TestClientHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientHandler for TestClientHandler {
    fn get_info(&self) -> ClientInfo {
        let capabilities = if self.supports_elicitation {
            ClientCapabilities::builder().enable_elicitation().build()
        } else {
            ClientCapabilities::default()
        };

        ClientInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities,
            client_info: Implementation {
                name: "test-client".to_string(),
                version: "1.0.0".to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
        }
    }

    async fn create_elicitation(
        &self,
        _params: CreateElicitationRequestParam,
        _context: RequestContext<RoleClient>,
    ) -> Result<CreateElicitationResult, ErrorData> {
        Ok(CreateElicitationResult {
            action: self.elicitation_response.clone(),
            content: match self.elicitation_response {
                ElicitationAction::Accept => Some(json!({"confirm": true})),
                _ => None,
            },
        })
    }
}
