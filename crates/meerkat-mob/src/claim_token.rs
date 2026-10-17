//! The claim token of a mob's callback route.
//!
//! A connection that creates a mob over RPC receives the mob's claim token
//! once, in the create result. Another connection can take over the mob's
//! callback route (its members' callbacks and its pending detached callback
//! jobs) only by presenting it. The mob keeps only a verifier, the SHA-256 of
//! the token, in its own store: the verifier survives restarts and never
//! yields the token.

use sha2::{Digest, Sha256};

/// The prefix of a rendered claim token.
const CLAIM_TOKEN_PREFIX: &str = "mct_";

/// The number of OS entropy bytes in a claim token.
const CLAIM_TOKEN_BYTES: usize = 32;

/// The length of the token's base64url body (no padding) for
/// [`CLAIM_TOKEN_BYTES`] bytes.
const CLAIM_TOKEN_BODY_LEN: usize = (CLAIM_TOKEN_BYTES * 4).div_ceil(3);

/// A mob's callback claim token: `mct_` and the unpadded base64url of 32
/// bytes of OS entropy. Bearer authority for one mob's callback route; its
/// `Debug` is redacted, and it is never logged or persisted in clear.
#[derive(Clone)]
pub struct MobClaimToken(String);

/// A text that is not a [`MobClaimToken`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MobClaimTokenError {
    /// The text does not have the token's shape: `mct_` and 43 base64url
    /// characters.
    #[error("not a mob claim token: expected `mct_` and 43 base64url characters")]
    Malformed,
    /// The OS entropy source failed while minting.
    #[error("could not mint a mob claim token: {0}")]
    Entropy(String),
}

impl MobClaimToken {
    /// Mint a new token from OS entropy.
    pub fn mint() -> Result<Self, MobClaimTokenError> {
        let mut bytes = [0_u8; CLAIM_TOKEN_BYTES];
        meerkat_core::secret_entropy::fill_secret_entropy(&mut bytes)
            .map_err(|error| MobClaimTokenError::Entropy(error.to_string()))?;
        let mut rendered = String::with_capacity(CLAIM_TOKEN_PREFIX.len() + CLAIM_TOKEN_BODY_LEN);
        rendered.push_str(CLAIM_TOKEN_PREFIX);
        encode_base64url_unpadded(&bytes, &mut rendered);
        Ok(Self(rendered))
    }

    /// Accept a token presented on the wire. Only its shape is checked here;
    /// whether it is a mob's token is the verifier's question.
    pub fn parse(text: &str) -> Result<Self, MobClaimTokenError> {
        let body = text
            .strip_prefix(CLAIM_TOKEN_PREFIX)
            .filter(|body| body.len() == CLAIM_TOKEN_BODY_LEN)
            .ok_or(MobClaimTokenError::Malformed)?;
        if !body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(MobClaimTokenError::Malformed);
        }
        Ok(Self(text.to_owned()))
    }

    /// The token's text, for the one create result that returns it to its
    /// creator. Never log it.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// The verifier a mob keeps for this token.
    pub fn verifier(&self) -> MobClaimVerifier {
        MobClaimVerifier(Sha256::digest(self.0.as_bytes()).into())
    }
}

impl std::fmt::Debug for MobClaimToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MobClaimToken([REDACTED])")
    }
}

/// What a mob keeps of its claim token: the SHA-256 of the token's text. It
/// is compared with a presented token in constant time.
#[derive(Clone, PartialEq, Eq)]
pub struct MobClaimVerifier([u8; 32]);

impl MobClaimVerifier {
    /// Rebuild a verifier from the bytes a store kept.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The verifier's bytes, for a store to keep.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether `token` is the token this verifier was made from, compared in
    /// constant time.
    pub fn verifies(&self, token: &MobClaimToken) -> bool {
        let presented: [u8; 32] = Sha256::digest(token.0.as_bytes()).into();
        presented
            .iter()
            .zip(self.0.iter())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }
}

impl std::fmt::Debug for MobClaimVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MobClaimVerifier(..)")
    }
}

/// The answer to a presented claim token for one mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackClaimCheck {
    /// The token is the mob's.
    Verified,
    /// The mob has a claim token, and this is not it.
    Mismatch,
    /// The mob has no claim token: it was not created over RPC (an agent's
    /// `mob_create`, an implicit mob, a host's Rust create) or before 0.9.3,
    /// and the host minted none for it. No token claims its route.
    NotClaimable,
}

/// Append the unpadded base64url encoding of `bytes` to `output`.
fn encode_base64url_unpadded(bytes: &[u8], output: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let group = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        for shift in [18, 12, 6, 0] {
            output.push(char::from(ALPHABET[((group >> shift) & 0x3f) as usize]));
        }
    }
    match *chunks.remainder() {
        [first] => {
            let group = u32::from(first) << 16;
            for shift in [18, 12] {
                output.push(char::from(ALPHABET[((group >> shift) & 0x3f) as usize]));
            }
        }
        [first, second] => {
            let group = (u32::from(first) << 16) | (u32::from(second) << 8);
            for shift in [18, 12, 6] {
                output.push(char::from(ALPHABET[((group >> shift) & 0x3f) as usize]));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_token_has_its_shape_and_parses_back() {
        let token = MobClaimToken::mint().expect("entropy");
        let text = token.expose_secret();
        assert!(text.starts_with("mct_"));
        assert_eq!(text.len(), 4 + 43);
        assert_eq!(
            MobClaimToken::parse(text).map(|parsed| parsed.expose_secret().to_owned()),
            Ok(text.to_owned())
        );
        assert_ne!(
            MobClaimToken::mint().expect("entropy").expose_secret(),
            text,
            "independent mints differ"
        );
        assert_eq!(format!("{token:?}"), "MobClaimToken([REDACTED])");
        assert!(!format!("{:?}", token.verifier()).contains(&text[4..12]));
    }

    #[test]
    fn the_encoding_is_unpadded_base64url() {
        let mut rendered = String::new();
        encode_base64url_unpadded(&[0xfb, 0xff, 0xbf, 0x00, 0x01], &mut rendered);
        assert_eq!(rendered, "-_-_AAE");
        let mut zeroes = String::new();
        encode_base64url_unpadded(&[0; 32], &mut zeroes);
        assert_eq!(zeroes, "A".repeat(43));
    }

    #[test]
    fn parsing_refuses_other_shapes() {
        let body = "A".repeat(43);
        for other in [
            String::new(),
            body.clone(),
            format!("mct{body}"),
            format!("mct_{}", &body[1..]),
            format!("mct_{body}A"),
            format!("mct_{}+", &body[1..]),
            format!("mct_{}=", &body[1..]),
            format!("mct_{} ", &body[1..]),
        ] {
            assert!(
                matches!(
                    MobClaimToken::parse(&other),
                    Err(MobClaimTokenError::Malformed)
                ),
                "{other}"
            );
        }
    }

    #[test]
    fn a_verifier_verifies_only_its_own_token() {
        let token = MobClaimToken::mint().expect("entropy");
        let other = MobClaimToken::mint().expect("entropy");
        let verifier = token.verifier();
        assert!(verifier.verifies(&token));
        assert!(!verifier.verifies(&other));
        let kept = MobClaimVerifier::from_bytes(*verifier.as_bytes());
        assert!(kept.verifies(&token));
        assert_eq!(kept, verifier);
    }
}
