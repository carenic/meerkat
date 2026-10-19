use std::io;
use std::sync::Arc;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use ciborium::value::Value;

use crate::types::Envelope;

use super::MAX_PAYLOAD_SIZE;

#[derive(Debug, Clone)]
pub struct EnvelopeFrame {
    pub envelope: Envelope,
    pub raw: Arc<Bytes>,
}

impl EnvelopeFrame {
    pub fn from_envelope(envelope: Envelope) -> Self {
        Self {
            envelope,
            raw: Arc::new(Bytes::new()),
        }
    }

    /// Verify the envelope's signature over the frame as received.
    ///
    /// The signed tuple `[id, from, to, kind]` is rebuilt from the received
    /// CBOR map, so every entry of `kind` is covered, including fields this
    /// build does not know: a newer sender may add an optional field to a
    /// signed kind. The typed `envelope` comes from the same bytes and drops
    /// such a field after this check, so it never reaches behaviour. A map
    /// keyed by anything but a text string, or holding the same key twice, at
    /// any depth, is refused before canonicalization; otherwise a frame could
    /// verify one entry while the typed decode acts on the other. The bytes
    /// were already bounded by the codec's payload limit, and the value decode
    /// keeps the nesting limit of the codec's typed decode. A frame built
    /// locally has no received bytes and does not verify.
    pub fn verify_signature(&self) -> bool {
        let Ok(received) = ciborium::from_reader::<Value, _>(self.raw.as_ref().as_ref()) else {
            return false;
        };
        if has_ambiguous_map_key(&received) {
            return false;
        }
        let Value::Map(entries) = received else {
            return false;
        };
        let (mut id, mut from, mut to, mut kind) = (None, None, None, None);
        for (key, value) in entries {
            let slot = match key.as_text() {
                Some("id") => &mut id,
                Some("from") => &mut from,
                Some("to") => &mut to,
                Some("kind") => &mut kind,
                _ => continue,
            };
            *slot = Some(value);
        }
        let (Some(id), Some(from), Some(to), Some(kind)) = (id, from, to, kind) else {
            return false;
        };
        let signable = crate::types::canonical_cbor_bytes(Value::Array(vec![id, from, to, kind]));
        !signable.is_empty() && self.envelope.from.verify(&signable, &self.envelope.sig)
    }
}

/// Whether any CBOR map in `value`, at any depth, has a key that is not a
/// plain text string, or the same key twice.
///
/// The typed decode names a field by more than its text: it skips a key's
/// tags, reads a byte string as the field's name and, inside a message kind,
/// can read an integer as the field's position. Every map this protocol
/// signs is keyed by text (struct fields, enum tags and JSON objects), so any
/// other key is refused rather than compared.
fn has_ambiguous_map_key(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(has_ambiguous_map_key),
        Value::Map(entries) => {
            let mut keys = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let Value::Text(key) = key else {
                    return true;
                };
                if has_ambiguous_map_key(value) {
                    return true;
                }
                keys.push(key.as_str());
            }
            keys.sort_unstable();
            keys.windows(2).any(|pair| pair[0] == pair[1])
        }
        Value::Tag(_, inner) => has_ambiguous_map_key(inner),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransportCodec {
    max_payload_size: u32,
}

impl Default for TransportCodec {
    fn default() -> Self {
        Self {
            max_payload_size: MAX_PAYLOAD_SIZE,
        }
    }
}

impl TransportCodec {
    pub fn new(max_payload_size: u32) -> Self {
        Self {
            max_payload_size: max_payload_size.min(MAX_PAYLOAD_SIZE),
        }
    }
}

impl Decoder for TransportCodec {
    type Item = EnvelopeFrame;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            return Ok(None);
        }

        let len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]);
        if len > self.max_payload_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "message too large: {len} bytes (max {})",
                    self.max_payload_size
                ),
            ));
        }

        let frame_len = 4usize + len as usize;
        if src.len() < frame_len {
            return Ok(None);
        }

        src.advance(4);
        let raw = Arc::new(src.split_to(len as usize).freeze());

        let envelope: Envelope = ciborium::from_reader(raw.as_ref().as_ref()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("CBOR decode error: {e}"),
            )
        })?;

        Ok(Some(EnvelopeFrame { envelope, raw }))
    }
}

impl Encoder<EnvelopeFrame> for TransportCodec {
    type Error = io::Error;

    fn encode(&mut self, item: EnvelopeFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if !item.raw.is_empty() {
            let len: u32 = item.raw.len().try_into().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "message too large to length-prefix",
                )
            })?;

            if len > self.max_payload_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "message too large: {len} bytes (max {})",
                        self.max_payload_size
                    ),
                ));
            }

            dst.reserve(4 + item.raw.len());
            dst.put_u32(len);
            dst.put_slice(item.raw.as_ref());
            return Ok(());
        }

        let frame_start = dst.len();
        dst.reserve(4);
        dst.put_u32(0);
        let payload_start = dst.len();

        let encode_result = {
            let mut writer = dst.writer();
            ciborium::into_writer(&item.envelope, &mut writer)
        };

        if let Err(err) = encode_result {
            dst.truncate(frame_start);
            return Err(io::Error::other(format!("CBOR encode error: {err}")));
        }

        let payload_len = dst.len().saturating_sub(payload_start);
        let len: u32 = payload_len.try_into().map_err(|_| {
            dst.truncate(frame_start);
            io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large to length-prefix",
            )
        })?;

        if len > self.max_payload_size {
            dst.truncate(frame_start);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "message too large: {len} bytes (max {})",
                    self.max_payload_size
                ),
            ));
        }

        dst[frame_start..frame_start + 4].copy_from_slice(&len.to_be_bytes());
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) mod test_frames {
    //! Raw signed frames: kinds this build may not know, repeated keys, and
    //! fields altered after signing.
    use ciborium::value::Value;
    use uuid::Uuid;

    use crate::identity::{Keypair, PubKey};

    pub(crate) fn text(value: &str) -> Value {
        Value::Text(value.to_string())
    }

    /// A `message` kind with body "hello" followed by `extra`, in order.
    pub(crate) fn message_kind(extra: Vec<(Value, Value)>) -> Value {
        let mut entries = vec![
            (text("type"), text("message")),
            (text("body"), text("hello")),
        ];
        entries.extend(extra);
        Value::Map(entries)
    }

    /// The envelope map `sender` writes to `to`: `[id, from, to, signed_kind]`
    /// signed canonically, exactly as given, with `sent_kind` on the wire.
    pub(crate) fn signed_map(
        sender: &Keypair,
        to: &PubKey,
        signed_kind: Value,
        sent_kind: Value,
    ) -> (Uuid, Vec<(Value, Value)>) {
        let id = Uuid::new_v4();
        let id_value = Value::serialized(&id).expect("id encodes");
        let from = Value::serialized(&sender.public_key()).expect("sender key encodes");
        let to = Value::serialized(to).expect("recipient key encodes");
        let signable = crate::types::canonical_cbor_bytes(Value::Array(vec![
            id_value.clone(),
            from.clone(),
            to.clone(),
            signed_kind,
        ]));
        let sig = Value::serialized(&sender.sign(&signable)).expect("signature encodes");
        (
            id,
            vec![
                (text("id"), id_value),
                (text("from"), from),
                (text("to"), to),
                (text("kind"), sent_kind),
                (text("sig"), sig),
            ],
        )
    }

    /// One length-prefixed frame holding `entries` as a CBOR map.
    pub(crate) fn frame_bytes(entries: Vec<(Value, Value)>) -> Vec<u8> {
        let mut payload = Vec::new();
        ciborium::into_writer(&Value::Map(entries), &mut payload).expect("frame encodes");
        let mut bytes = u32::try_from(payload.len())
            .expect("frame length fits")
            .to_be_bytes()
            .to_vec();
        bytes.extend_from_slice(&payload);
        bytes
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::test_frames::{frame_bytes, message_kind, signed_map, text};
    use super::*;
    use crate::identity::{Keypair, PubKey, Signature};
    use crate::types::{MessageKind, Status};
    use uuid::Uuid;

    fn decode_bytes(bytes: &[u8]) -> io::Result<Option<EnvelopeFrame>> {
        let mut buf = BytesMut::from(bytes);
        TransportCodec::default().decode(&mut buf)
    }

    fn decode_frame(entries: Vec<(Value, Value)>) -> EnvelopeFrame {
        decode_bytes(&frame_bytes(entries))
            .expect("the frame decodes")
            .expect("one whole frame")
    }

    fn recipient() -> PubKey {
        Keypair::generate().public_key()
    }

    /// The frame `entries` would make if the codec's typed decode were
    /// skipped: only the received bytes, the sender's key and the signature in
    /// `entries` decide whether it verifies.
    fn forced_frame(sender: &Keypair, entries: Vec<(Value, Value)>) -> EnvelopeFrame {
        let sig = entries
            .iter()
            .find(|(key, _)| key.as_text() == Some("sig"))
            .and_then(|(_, sig)| sig.deserialized::<Signature>().ok())
            .expect("the frame carries the sender's signature");
        let bytes = frame_bytes(entries);
        EnvelopeFrame {
            envelope: Envelope {
                id: Uuid::new_v4(),
                from: sender.public_key(),
                to: recipient(),
                kind: MessageKind::Ack {
                    in_reply_to: Uuid::new_v4(),
                },
                sig,
            },
            raw: Arc::new(Bytes::copy_from_slice(&bytes[4..])),
        }
    }

    #[test]
    fn a_signed_field_this_build_does_not_know_verifies_over_the_received_frame() {
        let sender = Keypair::generate();
        let kind = message_kind(vec![(text("future_field"), text("kept"))]);
        let (_, entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        let frame = decode_frame(entries);
        assert!(
            frame.verify_signature(),
            "the signature covers the field as received"
        );
        assert!(
            !frame.envelope.verify(),
            "the typed envelope dropped the field, so it alone cannot verify"
        );
        assert!(
            matches!(&frame.envelope.kind, MessageKind::Message { body, .. } if body == "hello")
        );
    }

    #[test]
    fn a_signed_field_altered_after_signing_does_not_verify() {
        let sender = Keypair::generate();
        let (_, entries) = signed_map(
            &sender,
            &recipient(),
            message_kind(vec![(text("future_field"), text("kept"))]),
            message_kind(vec![(text("future_field"), text("altered"))]),
        );
        assert!(!decode_frame(entries).verify_signature());
    }

    fn every_kind() -> Vec<MessageKind> {
        vec![
            MessageKind::Message {
                body: "hello".to_string(),
                blocks: None,
                content_taint: None,
                handling_mode: None,
                objective_id: None,
                in_reply_to: Some(Uuid::new_v4()),
            },
            MessageKind::IncarnationFencedMessage {
                body: "fenced".to_string(),
                blocks: None,
                content_taint: None,
                handling_mode: None,
                objective_id: None,
                expected_recipient: meerkat_core::comms::PeerRecipientIncarnation {
                    mob_id: "mob".to_string(),
                    agent_identity: "worker".to_string(),
                    host_id: "host".to_string(),
                    binding_generation: 1,
                    member_session_id: "session".to_string(),
                    generation: 2,
                    fence_token: 3,
                },
            },
            MessageKind::Request {
                intent: "probe".to_string(),
                params: serde_json::json!({ "nested": { "list": [1, 2.5, "three", null] } }),
                blocks: None,
                reply_endpoint: None,
                content_taint: None,
                handling_mode: None,
                objective_id: None,
            },
            MessageKind::Lifecycle {
                kind: meerkat_core::comms::PeerLifecycleKind::PeerAdded,
                params: serde_json::json!({ "peer": "added" }),
            },
            MessageKind::Response {
                in_reply_to: Uuid::new_v4(),
                status: Status::Completed,
                result: serde_json::json!({ "ok": true }),
                blocks: None,
                content_taint: None,
                handling_mode: None,
                objective_id: None,
            },
            MessageKind::Ack {
                in_reply_to: Uuid::new_v4(),
            },
        ]
    }

    #[test]
    fn frame_verification_equals_typed_verification_for_every_message_kind() {
        let mut variants = std::collections::BTreeSet::new();
        for kind in every_kind() {
            // A new variant fails to compile here until it has a fixture.
            variants.insert(match &kind {
                MessageKind::Message { .. } => "message",
                MessageKind::IncarnationFencedMessage { .. } => "incarnation_fenced_message",
                MessageKind::Request { .. } => "request",
                MessageKind::Lifecycle { .. } => "lifecycle",
                MessageKind::Response { .. } => "response",
                MessageKind::Ack { .. } => "ack",
            });
            let sender = Keypair::generate();
            let mut envelope = Envelope {
                id: Uuid::new_v4(),
                from: sender.public_key(),
                to: recipient(),
                kind,
                sig: Signature::new([0u8; 64]),
            };
            envelope.sign(&sender);
            let forged = Envelope {
                sig: Signature::new([7u8; 64]),
                ..envelope.clone()
            };
            for (signed, candidate) in [(true, envelope), (false, forged)] {
                let mut buf = BytesMut::new();
                TransportCodec::default()
                    .encode(EnvelopeFrame::from_envelope(candidate.clone()), &mut buf)
                    .expect("the frame encodes");
                let frame = TransportCodec::default()
                    .decode(&mut buf)
                    .expect("the frame decodes")
                    .expect("one whole frame");
                assert_eq!(frame.verify_signature(), candidate.verify());
                assert_eq!(frame.verify_signature(), signed);
            }
        }
        assert_eq!(variants.len(), 6, "every MessageKind variant has a fixture");
    }

    #[test]
    fn a_repeated_key_at_the_top_of_the_envelope_is_refused_at_decode() {
        let sender = Keypair::generate();
        let kind = message_kind(Vec::new());
        let (_, mut entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        entries.push((
            text("kind"),
            Value::Map(vec![
                (text("type"), text("message")),
                (text("body"), text("other")),
            ]),
        ));
        assert!(decode_bytes(&frame_bytes(entries)).is_err());
    }

    #[test]
    fn a_repeated_unknown_key_inside_the_kind_is_refused_even_when_signed() {
        let sender = Keypair::generate();
        // The sender signs both entries; the typed decode ignores the key.
        let kind = message_kind(vec![
            (text("future_field"), text("a")),
            (text("future_field"), text("b")),
        ]);
        let (_, entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        assert!(!decode_frame(entries).verify_signature());
    }

    #[test]
    fn a_repeated_known_field_inside_the_kind_never_verifies() {
        let sender = Keypair::generate();
        let kind = message_kind(vec![(text("body"), text("other"))]);
        let (_, entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        match decode_bytes(&frame_bytes(entries)) {
            Err(_) => {}
            Ok(Some(frame)) => assert!(!frame.verify_signature()),
            Ok(None) => panic!("a whole frame was written"),
        }
    }

    #[test]
    fn a_repeated_key_inside_an_opaque_map_field_is_refused_even_when_signed() {
        let sender = Keypair::generate();
        let params = Value::Map(vec![
            (text("a"), Value::Integer(1.into())),
            (text("a"), Value::Integer(2.into())),
        ]);
        let kind = Value::Map(vec![
            (text("type"), text("request")),
            (text("intent"), text("probe")),
            (text("params"), params),
        ]);
        let (_, entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        let frame = decode_frame(entries);
        assert!(
            matches!(frame.envelope.kind, MessageKind::Request { .. }),
            "the typed decode keeps one of the entries"
        );
        assert!(!frame.verify_signature());
    }

    #[test]
    fn a_second_kind_under_a_byte_string_or_tagged_key_is_refused_even_when_signed() {
        let sender = Keypair::generate();
        let kind = message_kind(Vec::new());
        let (_, entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        assert!(
            forced_frame(&sender, entries.clone()).verify_signature(),
            "control: the frame as signed verifies"
        );
        let other = Value::Map(vec![
            (text("type"), text("message")),
            (text("body"), text("other")),
        ]);
        // The typed decode reads both keys as `kind`.
        for key in [
            Value::Bytes(b"kind".to_vec()),
            Value::Tag(32, Box::new(text("kind"))),
        ] {
            let mut appended = entries.clone();
            appended.push((key, other.clone()));
            match decode_bytes(&frame_bytes(appended.clone())) {
                Err(_) => {}
                Ok(Some(frame)) => assert!(!frame.verify_signature()),
                Ok(None) => panic!("a whole frame was written"),
            }
            // The typed decode refuses a repeated field today; the frame check
            // refuses the key without it.
            assert!(!forced_frame(&sender, appended).verify_signature());
        }
    }

    #[test]
    fn a_kind_keyed_by_anything_but_text_does_not_verify_even_when_signed() {
        let sender = Keypair::generate();
        // The sender signs both entries; the typed decode reads both as the
        // unknown field `future` and ignores it.
        let kind = message_kind(vec![
            (text("future"), text("a")),
            (Value::Bytes(b"future".to_vec()), text("b")),
        ]);
        let (_, entries) = signed_map(&sender, &recipient(), kind.clone(), kind);
        let frame = decode_frame(entries);
        assert!(
            matches!(&frame.envelope.kind, MessageKind::Message { body, .. } if body == "hello"),
            "the typed decode accepts the frame"
        );
        assert!(!frame.verify_signature());
    }

    /// A request signed by `sender` whose params nest `depth` arrays deep.
    fn nested_request(sender: &Keypair, depth: usize) -> Vec<(Value, Value)> {
        let mut nested = Value::Integer(0.into());
        for _ in 0..depth {
            nested = Value::Array(vec![nested]);
        }
        let kind = Value::Map(vec![
            (text("type"), text("request")),
            (text("intent"), text("probe")),
            (text("params"), nested),
        ]);
        signed_map(sender, &recipient(), kind.clone(), kind).1
    }

    #[test]
    fn a_frame_nested_past_the_codec_limit_is_refused_before_any_verification() {
        let sender = Keypair::generate();
        // Control: at a depth both decodes accept, the request verifies through
        // the codec and forced in as received bytes alike.
        let shallow = nested_request(&sender, 200);
        assert!(decode_frame(shallow.clone()).verify_signature());
        assert!(forced_frame(&sender, shallow).verify_signature());

        let deep = nested_request(&sender, 300);
        assert!(
            decode_bytes(&frame_bytes(deep.clone())).is_err(),
            "the codec refuses the frame"
        );
        assert!(
            !forced_frame(&sender, deep).verify_signature(),
            "the value decode keeps the codec's nesting limit, so the sender's own signature does not verify it"
        );
    }

    #[test]
    fn a_frame_built_locally_has_no_received_bytes_and_does_not_verify() {
        let sender = Keypair::generate();
        let mut envelope = Envelope {
            id: Uuid::new_v4(),
            from: sender.public_key(),
            to: recipient(),
            kind: MessageKind::Ack {
                in_reply_to: Uuid::new_v4(),
            },
            sig: Signature::new([0u8; 64]),
        };
        envelope.sign(&sender);
        assert!(envelope.verify());
        assert!(!EnvelopeFrame::from_envelope(envelope).verify_signature());
    }
}
