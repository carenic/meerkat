//! Anthropic backend kinds (typed, provider-owned).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnthropicBackendKind {
    AnthropicApi,
}

impl AnthropicBackendKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "anthropic_api" => Some(Self::AnthropicApi),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicApi => "anthropic_api",
        }
    }
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::AnthropicApi => "https://api.anthropic.com",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        assert_eq!(
            AnthropicBackendKind::parse("anthropic_api"),
            Some(AnthropicBackendKind::AnthropicApi),
        );
        assert_eq!(AnthropicBackendKind::parse("other"), None);
    }
}
