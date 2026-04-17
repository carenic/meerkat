//! Anthropic auth methods (typed, provider-owned).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnthropicAuthMethod {
    ApiKey,
    StaticBearer,
    ClaudeAiOauth,
    OauthToApiKey,
    ExternalAuthorizer,
}

impl AnthropicAuthMethod {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "api_key" => Some(Self::ApiKey),
            "static_bearer" => Some(Self::StaticBearer),
            "claude_ai_oauth" => Some(Self::ClaudeAiOauth),
            "oauth_to_api_key" => Some(Self::OauthToApiKey),
            "external_authorizer" => Some(Self::ExternalAuthorizer),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::StaticBearer => "static_bearer",
            Self::ClaudeAiOauth => "claude_ai_oauth",
            Self::OauthToApiKey => "oauth_to_api_key",
            Self::ExternalAuthorizer => "external_authorizer",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip_all_variants() {
        for v in [
            AnthropicAuthMethod::ApiKey,
            AnthropicAuthMethod::StaticBearer,
            AnthropicAuthMethod::ClaudeAiOauth,
            AnthropicAuthMethod::OauthToApiKey,
            AnthropicAuthMethod::ExternalAuthorizer,
        ] {
            assert_eq!(AnthropicAuthMethod::parse(v.as_str()), Some(v));
        }
    }
}
