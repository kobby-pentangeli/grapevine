//! Cryptographic node identity: Ed25519 keypairs, message signing, and
//! origin authentication.
//!
//! # Threat model
//!
//! Every node holds an Ed25519 keypair generated at startup; its [`PeerId`] is
//! the public half. A node signs every message it authors over a
//! domain-separated encoding of the message's *immutable* fields---the origin
//! address, the per-origin sequence, and the payload---and embeds its public
//! key alongside the signature. The mutable [`Message::ttl`] and the
//! metadata-only `MessageId::timestamp` are deliberately excluded, so a
//! signature survives the TTL decrements that forwarding applies.
//!
//! On receipt every message is authenticated by [`authenticate`], which
//! provides:
//!
//! - **Integrity.** A single bit flipped in the origin, sequence, or payload
//!   invalidates the signature, so tampered messages are dropped.
//! - **Proof of possession.** A valid signature proves the sender holds the
//!   private key for the embedded public key.
//! - **Origin authenticity (trust-on-first-use).** The first time a message is
//!   seen for a given origin address, that address is *pinned* to the key the
//!   message carried; any later message claiming the same origin under a
//!   different key is rejected. A peer therefore cannot forge a message
//!   attributed to an origin whose key a node has already pinned---it lacks
//!   that origin's private key.
//!
//! What this does **not** provide is out of scope for v1.1.0 and documented so
//! it is not mistaken for a guarantee:
//!
//! - **No confidentiality.** Messages travel in plaintext; authenticity is not
//!   encryption. Confidentiality requires the deferred TLS/QUIC transport.
//! - **No protection against a first-contact active attacker.** Pinning is
//!   trust-on-first-use, so an attacker who controls the path *before* a key is
//!   pinned can substitute their own key for an origin a node has never seen. A
//!   PKI or transport authentication closes this and is deferred with the
//!   membership overlay.
//! - **No Sybil resistance.** Identities are self-minted keypairs; nothing
//!   binds a key to a real-world principal or limits how many a peer creates.

use std::fmt;
use std::net::SocketAddr;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use ed25519_dalek::{Signature as Ed25519Signature, Signer, SigningKey, VerifyingKey};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Message, MessageId, Payload, Result};

/// Domain-separation tag mixed into every signature preimage so a
/// Grapevine signature can never be confused with one produced for a
/// different protocol or future wire version.
const SIGNING_DOMAIN: &[u8] = b"grapevine.message.v1";

/// A node's cryptographic identity: the Ed25519 public key, in compressed form.
///
/// Identity is the key, not the socket address, so two nodes are the same peer iff
/// they hold the same keypair, regardless of where they connect from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub [u8; 32]);

impl PeerId {
    /// The sentinel carried by an unsigned [`Message`] (see [`Message::new`]).
    /// It is not a valid Ed25519 public key, so [`verify_message`] rejects any
    /// message still bearing it.
    pub const UNSIGNED: Self = Self([0u8; 32]);

    /// Construct a peer identity from raw compressed public-key bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw compressed public-key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0[..8] {
            write!(f, "{byte:02x}")?;
        }
        f.write_str("..")
    }
}

/// An Ed25519 signature over a message's domain-separated signing preimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    /// The sentinel carried by an unsigned [`Message`] (see [`Message::new`]).
    pub const UNSIGNED: Self = Self([0u8; 64]);

    /// The raw 64-byte signature.
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct SignatureVisitor;

        impl<'de> Visitor<'de> for SignatureVisitor {
            type Value = Signature;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a 64-byte Ed25519 signature")
            }

            fn visit_bytes<E: de::Error>(self, value: &[u8]) -> std::result::Result<Signature, E> {
                <[u8; 64]>::try_from(value)
                    .map(Signature)
                    .map_err(|_| E::invalid_length(value.len(), &self))
            }

            fn visit_byte_buf<E: de::Error>(
                self,
                value: Vec<u8>,
            ) -> std::result::Result<Signature, E> {
                self.visit_bytes(&value)
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Signature, A::Error> {
                let mut bytes = [0u8; 64];
                for (index, slot) in bytes.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(index, &self))?;
                }
                Ok(Signature(bytes))
            }
        }

        deserializer.deserialize_bytes(SignatureVisitor)
    }
}

/// A node's signing identity: its Ed25519 keypair.
///
/// The private key never leaves this type; callers author messages through
/// [`Identity::author`], which signs them, and read the node's public identity
/// through [`Identity::peer_id`].
pub struct Identity {
    signing_key: SigningKey,
    peer_id: PeerId,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never render the private key.
        f.debug_struct("Identity")
            .field("peer_id", &self.peer_id)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Generate a fresh keypair from operating-system randomness.
    ///
    /// The identity is stable for the lifetime of the process; persisting it
    /// across restarts is deferred for now.
    pub fn generate() -> Self {
        let seed: [u8; 32] = rand::random();
        let signing_key = SigningKey::from_bytes(&seed);
        let peer_id = PeerId(signing_key.verifying_key().to_bytes());
        Self {
            signing_key,
            peer_id,
        }
    }

    /// This node's public identity.
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Author and sign a message originated by this node, with the default TTL.
    ///
    /// # Errors
    /// Returns [`Error::Serialization`] if the signing preimage cannot be
    /// encoded (in practice this does not occur for well-formed payloads).
    pub fn author(&self, origin: SocketAddr, sequence: u64, payload: Payload) -> Result<Message> {
        self.author_with_ttl(origin, sequence, payload, Message::DEFAULT_TTL)
    }

    /// Author and sign a message originated by this node, with an explicit TTL.
    ///
    /// # Errors
    /// Returns [`Error::Serialization`] if the signing preimage cannot be
    /// encoded.
    pub fn author_with_ttl(
        &self,
        origin: SocketAddr,
        sequence: u64,
        payload: Payload,
        ttl: u8,
    ) -> Result<Message> {
        let preimage = preimage_bytes(origin, sequence, &payload)?;
        let signature = Signature(self.signing_key.sign(&preimage).to_bytes());
        Ok(Message {
            id: MessageId::new(origin, sequence),
            ttl,
            payload,
            origin_key: self.peer_id,
            signature,
        })
    }
}

/// Authenticate a received message: verify its signature, then enforce the
/// trust-on-first-use binding between its origin address and its key.
///
/// The first authentic message seen for an origin pins that origin to its key;
/// a later message claiming the same origin under a different key is rejected.
///
/// # Errors
/// Returns [`Error::InvalidSignature`] if verification fails, or
/// [`Error::OriginKeyMismatch`] if the origin is already pinned to a different
/// key.
pub fn authenticate(message: &Message, pins: &DashMap<SocketAddr, PeerId>) -> Result<()> {
    verify_message(message)?;

    let origin = message.id.origin;
    match pins.entry(origin) {
        Entry::Occupied(pinned) if *pinned.get() != message.origin_key => {
            Err(Error::OriginKeyMismatch(origin))
        }
        Entry::Occupied(_) => Ok(()),
        Entry::Vacant(slot) => {
            slot.insert(message.origin_key);
            Ok(())
        }
    }
}

/// Verify a message's signature against the public key it carries.
///
/// This proves integrity and proof-of-possession but says nothing about whether
/// the embedded key is the *right* key for the claimed origin; use
/// [`authenticate`] for the full origin check.
///
/// # Errors
/// Returns [`Error::InvalidSignature`] if the message is unsigned, carries an
/// invalid public key, or the signature does not verify.
pub fn verify_message(message: &Message) -> Result<()> {
    let origin = message.id.origin;

    if message.origin_key == PeerId::UNSIGNED {
        return Err(Error::InvalidSignature(origin));
    }

    let verifying_key = VerifyingKey::from_bytes(&message.origin_key.0)
        .map_err(|_| Error::InvalidSignature(origin))?;
    let preimage = preimage_bytes(origin, message.id.sequence, &message.payload)?;
    let signature = Ed25519Signature::from_bytes(&message.signature.0);

    verifying_key
        .verify_strict(&preimage, &signature)
        .map_err(|_| Error::InvalidSignature(origin))
}

/// The bytes a signature commits to: the domain tag, the origin, the sequence,
/// and the payload.
fn preimage_bytes(origin: SocketAddr, sequence: u64, payload: &Payload) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct Preimage<'a> {
        domain: &'static [u8],
        origin: SocketAddr,
        sequence: u64,
        payload: &'a Payload,
    }

    let preimage = Preimage {
        domain: SIGNING_DOMAIN,
        origin,
        sequence,
        payload,
    };
    Ok(bincode::serde::encode_to_vec(
        &preimage,
        bincode::config::standard(),
    )?)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn app(origin: SocketAddr, sequence: u64, body: &str) -> Message {
        Identity::generate()
            .author(
                origin,
                sequence,
                Payload::Application(Bytes::from(body.to_owned())),
            )
            .expect("authoring a well-formed message succeeds")
    }

    #[test]
    fn peer_id_is_the_verifying_key() {
        let identity = Identity::generate();
        let message = identity
            .author(addr(8000), 0, Payload::PeerListRequest)
            .unwrap();
        assert_eq!(message.origin_key, identity.peer_id());
    }

    #[test]
    fn peer_id_serde() {
        let id = Identity::generate().peer_id();
        let encoded = bincode::serde::encode_to_vec(id, bincode::config::standard()).unwrap();
        let (decoded, _): (PeerId, _) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn sign_then_verify() {
        let identity = Identity::generate();
        let message = identity
            .author(
                addr(8000),
                7,
                Payload::Application(Bytes::from_static(b"hi")),
            )
            .unwrap();
        assert!(verify_message(&message).is_ok());
    }

    #[test]
    fn signature_survives_ttl_decrement() {
        let identity = Identity::generate();
        let mut message = identity
            .author(
                addr(8000),
                1,
                Payload::Application(Bytes::from_static(b"x")),
            )
            .unwrap();
        message.decrement_ttl();
        message.decrement_ttl();
        assert!(
            verify_message(&message).is_ok(),
            "ttl is excluded from the signature so forwarding cannot break it"
        );
    }

    #[test]
    fn tampered_payload_fails_verification() {
        let mut message = app(addr(8000), 1, "original");
        message.payload = Payload::Application(Bytes::from_static(b"tampered"));
        assert!(matches!(
            verify_message(&message),
            Err(Error::InvalidSignature(_))
        ));
    }

    #[test]
    fn tampered_origin_fails_verification() {
        let mut message = app(addr(8000), 1, "body");
        message.id.origin = addr(9999);
        assert!(matches!(
            verify_message(&message),
            Err(Error::InvalidSignature(_))
        ));
    }

    #[test]
    fn tampered_sequence_fails_verification() {
        let mut message = app(addr(8000), 1, "body");
        message.id.sequence = 2;
        assert!(matches!(
            verify_message(&message),
            Err(Error::InvalidSignature(_))
        ));
    }

    #[test]
    fn unsigned_message_is_rejected() {
        let message = Message::new(addr(8000), 0, Payload::PeerListRequest);
        assert!(matches!(
            verify_message(&message),
            Err(Error::InvalidSignature(_))
        ));
    }

    #[test]
    fn swapped_key_fails_verification() {
        let mut message = app(addr(8000), 1, "body");
        message.origin_key = Identity::generate().peer_id();
        assert!(matches!(
            verify_message(&message),
            Err(Error::InvalidSignature(_))
        ));
    }

    #[test]
    fn authenticate_pins_first_key_and_rejects_later_changes() {
        let pins: DashMap<SocketAddr, PeerId> = DashMap::new();
        let origin = addr(8000);

        let honest = Identity::generate();
        let first = honest
            .author(origin, 0, Payload::Application(Bytes::from_static(b"one")))
            .unwrap();
        assert!(authenticate(&first, &pins).is_ok());
        assert_eq!(pins.get(&origin).map(|k| *k), Some(honest.peer_id()));

        // A second authentic message from the same key is accepted.
        let second = honest
            .author(origin, 1, Payload::Application(Bytes::from_static(b"two")))
            .unwrap();
        assert!(authenticate(&second, &pins).is_ok());

        // A different keypair claiming the pinned origin is rejected, even
        // though its own signature is internally valid.
        let forger = Identity::generate();
        let forged = forger
            .author(
                origin,
                2,
                Payload::Application(Bytes::from_static(b"forged")),
            )
            .unwrap();
        assert!(verify_message(&forged).is_ok());
        assert!(matches!(
            authenticate(&forged, &pins),
            Err(Error::OriginKeyMismatch(o)) if o == origin
        ));
    }
}
