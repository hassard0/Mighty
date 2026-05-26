//! v0.20 cluster mTLS configuration + cert-CN identity binding.
//!
//! v0.18 + v0.19 shipped **server-side** TLS: the listener presents a
//! cert, the dialer trusts a known root, and that's it. A peer's claimed
//! `NodeId` (from the post-TLS `Hello` frame) was never cryptographically
//! tied to the cert on the wire — a node holding a valid cert could
//! claim to be *any* node in the mesh.
//!
//! Tier 4.2 closes that gap with **mutual TLS** + **CN-bound identity**:
//!
//! - The listener requires every dialer to present a client cert signed
//!   by the cluster CA ([`ClusterTlsConfig::client_ca`]).
//! - After the handshake completes (on both sides), the peer cert's
//!   Common Name is extracted via [`cert_node_id`].
//! - The CN is compared to the `NodeId` claimed in the `Hello` frame.
//!   Mismatch → reject the connection with diag `MT5040`.
//!
//! ### Why CN, not SAN/SPIFFE?
//!
//! For the v0.20 slice we want the simplest cryptographic binding
//! possible: cluster nodes already have a meaningful operator-readable
//! identifier (the `node_id` string in `mighty.toml`), so we put it
//! straight in the cert's CN. SPIFFE IDs (`spiffe://trust-domain/node`)
//! would be technically nicer — they nest under SAN URI, survive issuer
//! changes, and slot directly into Istio / Linkerd / similar — but they
//! also pull in a second identity vocabulary and force every cluster to
//! pick a trust domain. We defer that to whenever an operator actually
//! asks for it; CN ships today.
//!
//! ### Back-compat
//!
//! Setting [`ClusterTlsConfig::require_client_cert`] to `false` builds
//! an acceptor that DOES NOT request a client cert — bit-for-bit the
//! same shape as the v0.18 / v0.19 server-only TLS path. Existing
//! deployments keep working without touching their config; mTLS is
//! opt-in via the `[cluster.tls].require_client_cert = true` knob
//! (added in v0.20).

use crate::cluster::address::NodeId;
use std::sync::Arc;
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
        server::WebPkiClientVerifier,
        ClientConfig, RootCertStore, ServerConfig,
    },
    TlsAcceptor, TlsConnector,
};

/// All the cryptographic material the mesh needs to bring up a fully
/// mTLS-secured listener + dialer pair.
///
/// `server_cert` / `server_key` are what *this node* presents on every
/// inbound and outbound connection (both ends present a cert in mTLS
/// — that's what the `mutual` adjective means).
///
/// `client_ca` is the root the listener will require dialer certs to
/// chain to. In a self-hosted cluster this is one internal CA; for dev
/// it can be the per-node self-signed cert list.
///
/// `require_client_cert` toggles the mode:
/// - `true`  → mTLS (acceptor refuses any handshake without a valid
///   client cert chaining to `client_ca`).
/// - `false` → server-only TLS (back-compat with v0.18 / v0.19).
///
/// Hand-rolled `Clone` because `PrivateKeyDer` is intentionally not
/// `Clone` in rustls 0.23 (defense-in-depth against accidentally
/// shipping key material across an `Arc` boundary). We use the
/// `clone_key()` helper that takes ownership semantics seriously.
pub struct ClusterTlsConfig {
    pub server_cert: CertificateDer<'static>,
    pub server_key: PrivateKeyDer<'static>,
    /// Roots that dialer client certs must chain to (only consulted
    /// when `require_client_cert == true`).
    pub client_ca: Arc<RootCertStore>,
    /// Roots that *our own* dialer trusts on the server side of remote
    /// peers (server-cert verification — same as v0.18).
    pub server_ca: Arc<RootCertStore>,
    /// Cert the dialer presents to remote listeners (mTLS). Defaults to
    /// `server_cert` if not set — most clusters use one cert per node
    /// for both roles.
    pub client_cert: Option<CertificateDer<'static>>,
    /// Private key for [`Self::client_cert`]. If `client_cert` is `None`,
    /// this is ignored and `server_key` is used.
    pub client_key: Option<PrivateKeyDer<'static>>,
    pub require_client_cert: bool,
}

impl Clone for ClusterTlsConfig {
    fn clone(&self) -> Self {
        Self {
            server_cert: self.server_cert.clone(),
            server_key: self.server_key.clone_key(),
            client_ca: self.client_ca.clone(),
            server_ca: self.server_ca.clone(),
            client_cert: self.client_cert.clone(),
            client_key: self.client_key.as_ref().map(|k| k.clone_key()),
            require_client_cert: self.require_client_cert,
        }
    }
}

impl std::fmt::Debug for ClusterTlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClusterTlsConfig")
            .field("require_client_cert", &self.require_client_cert)
            .field("client_ca_roots", &self.client_ca.roots.len())
            .field("server_ca_roots", &self.server_ca.roots.len())
            .field("has_separate_client_cert", &self.client_cert.is_some())
            .finish_non_exhaustive()
    }
}

/// Errors building / using a [`ClusterTlsConfig`].
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("cluster tls: server config: {0}")]
    Server(String),
    #[error("cluster tls: client config: {0}")]
    Client(String),
    #[error("cluster tls: client verifier: {0}")]
    ClientVerifier(String),
    /// `MT5040` — peer presented a cert whose CN does not match the
    /// `node_id` it claimed in the `Hello` frame.
    #[error("cluster tls (MT5040): cert CN {cn:?} does not match claimed node_id {claimed:?}")]
    IdentityMismatch { cn: String, claimed: String },
    /// `MT5040` — peer cert chain is missing, empty, or the leaf cert
    /// has no parseable Subject CN.
    #[error("cluster tls (MT5040): peer cert chain unusable: {0}")]
    BadPeerCert(String),
}

/// Build the [`TlsAcceptor`] from a [`ClusterTlsConfig`].
///
/// When `require_client_cert == true`, every accepted connection MUST
/// present a client cert that chains to `client_ca`. When `false`, the
/// acceptor accepts anonymous clients (server-only TLS — v0.18 mode).
pub fn build_acceptor(cfg: &ClusterTlsConfig) -> Result<Arc<TlsAcceptor>, TlsError> {
    let server_cfg = if cfg.require_client_cert {
        let verifier = WebPkiClientVerifier::builder(cfg.client_ca.clone())
            .build()
            .map_err(|e| TlsError::ClientVerifier(e.to_string()))?;
        ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![cfg.server_cert.clone()], cfg.server_key.clone_key())
            .map_err(|e| TlsError::Server(e.to_string()))?
    } else {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cfg.server_cert.clone()], cfg.server_key.clone_key())
            .map_err(|e| TlsError::Server(e.to_string()))?
    };
    Ok(Arc::new(TlsAcceptor::from(Arc::new(server_cfg))))
}

/// Build the [`TlsConnector`] from a [`ClusterTlsConfig`].
///
/// When `require_client_cert == true`, the connector presents
/// `client_cert` / `client_key` (defaulting to the server cert if those
/// optional fields are unset) so the remote listener can verify us.
pub fn build_connector(cfg: &ClusterTlsConfig) -> Result<Arc<TlsConnector>, TlsError> {
    let client_cfg = if cfg.require_client_cert {
        let (cert, key) = match (&cfg.client_cert, &cfg.client_key) {
            (Some(c), Some(k)) => (c.clone(), k.clone_key()),
            _ => (cfg.server_cert.clone(), cfg.server_key.clone_key()),
        };
        ClientConfig::builder()
            .with_root_certificates((*cfg.server_ca).clone())
            .with_client_auth_cert(vec![cert], key)
            .map_err(|e| TlsError::Client(e.to_string()))?
    } else {
        ClientConfig::builder()
            .with_root_certificates((*cfg.server_ca).clone())
            .with_no_client_auth()
    };
    Ok(Arc::new(TlsConnector::from(Arc::new(client_cfg))))
}

/// Convenience: build both `(TlsAcceptor, TlsConnector)` in one call so
/// the mesh can stash both into its [`crate::cluster::mesh::TlsConfig`]
/// in one step.
pub fn build_pair(
    cfg: &ClusterTlsConfig,
) -> Result<(Arc<TlsAcceptor>, Arc<TlsConnector>), TlsError> {
    Ok((build_acceptor(cfg)?, build_connector(cfg)?))
}

/// Extract the Subject Common Name from a DER-encoded X.509 cert and
/// return it as a [`NodeId`]. This is the identity we compare against
/// the `Hello` frame's claimed node id during mTLS-bound handshakes.
///
/// We intentionally do NOT use the full SAN list, multi-value RDN
/// sequences, or other X.509 esoterica — the cluster operator controls
/// cert issuance, so a single CN string is the only thing they need to
/// get right. If they want richer identity (multi-tenant, SPIFFE, …)
/// they can wrap this function with a different parser.
pub fn cert_node_id(cert: &CertificateDer<'_>) -> Result<NodeId, TlsError> {
    extract_cn_from_der(cert.as_ref())
        .map(NodeId::new)
        .ok_or_else(|| TlsError::BadPeerCert("could not extract Subject CN from cert".into()))
}

/// Validate that `claimed` matches the CN inside `cert`. Used by both
/// sides of the handshake (server validates the dialer's client cert,
/// dialer validates the listener's server cert) once mTLS is on.
pub fn verify_peer_identity(cert: &CertificateDer<'_>, claimed: &NodeId) -> Result<(), TlsError> {
    let actual = cert_node_id(cert)?;
    if &actual != claimed {
        return Err(TlsError::IdentityMismatch {
            cn: actual.as_str().to_string(),
            claimed: claimed.as_str().to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------
// Minimal DER walker — we only need the Subject CN, and pulling in a
// full ASN.1 / X.509 dep just for that is overkill. The body below
// walks the SEQUENCE structure deep enough to find the Subject's CN
// RDN and returns the printable string.
//
// X.509 cert shape (RFC 5280, simplified):
//
//     Certificate ::= SEQUENCE {
//         tbsCertificate       TBSCertificate,
//         signatureAlgorithm   AlgorithmIdentifier,
//         signatureValue       BIT STRING
//     }
//     TBSCertificate ::= SEQUENCE {
//         [0] version            Version DEFAULT v1,
//         serialNumber           CertificateSerialNumber,
//         signature              AlgorithmIdentifier,
//         issuer                 Name,
//         validity               Validity,
//         subject                Name,           <-- we want this
//         subjectPublicKeyInfo   SubjectPublicKeyInfo,
//         ...
//     }
//
// Name ::= CHOICE { rdnSequence RDNSequence }
// RDNSequence ::= SEQUENCE OF RelativeDistinguishedName
// RelativeDistinguishedName ::= SET OF AttributeTypeAndValue
// AttributeTypeAndValue ::= SEQUENCE { type OBJECT IDENTIFIER, value ANY }
//
// CN OID is 2.5.4.3 → DER bytes: 06 03 55 04 03
// ---------------------------------------------------------------

/// OID 2.5.4.3 (id-at-commonName), DER-encoded as an `OBJECT IDENTIFIER`
/// tag + length + body.
const CN_OID_DER: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03];

fn extract_cn_from_der(cert_der: &[u8]) -> Option<String> {
    // Outer SEQUENCE — Certificate. The first inner field is
    // TBSCertificate (another SEQUENCE), whose body (after a possibly-
    // present `[0] EXPLICIT Version`) is:
    //   serialNumber, signature, issuer, validity, subject, …
    //
    // We don't try to skip through every field by tag — rcgen and other
    // X.509 emitters use a mix of `INTEGER` / `OBJECT IDENTIFIER` / etc.
    // and an extension list that the simple "skip 5 fields" walker has
    // to special-case. Instead we walk all top-level TLVs inside
    // TBSCertificate, recognising the two `Name` SEQUENCEs (issuer +
    // subject) by structure: a Name is a SEQUENCE OF SET, whose inner
    // SET contains a SEQUENCE { OID, value }. The second such Name is
    // the subject — that's the CN we want.
    let (cert_body, _) = read_seq(cert_der)?;
    let (tbs_body, _) = read_seq(cert_body)?;
    let mut cn_found: Vec<String> = Vec::new();
    let mut cursor = tbs_body;
    while !cursor.is_empty() {
        let (body, next) = read_tlv(cursor)?;
        // A Name body is a SEQUENCE OF SET. Detect by trying to walk it.
        if let Some(cn) = walk_name_for_cn(body) {
            cn_found.push(cn);
            if cn_found.len() == 2 {
                // Subject = second Name in the cert (issuer first).
                return cn_found.pop();
            }
        }
        cursor = next;
    }
    // Self-signed certs may emit issuer == subject once — fall back to
    // the single Name we saw.
    cn_found.pop()
}

fn walk_name_for_cn(rdn_sequence: &[u8]) -> Option<String> {
    let mut cursor = rdn_sequence;
    while !cursor.is_empty() {
        // Each RDN is a SET.
        let (set_body, next) = read_set(cursor)?;
        if let Some(cn) = walk_rdn_for_cn(set_body) {
            return Some(cn);
        }
        cursor = next;
    }
    None
}

fn walk_rdn_for_cn(set_body: &[u8]) -> Option<String> {
    let mut cursor = set_body;
    while !cursor.is_empty() {
        // Each AttributeTypeAndValue is a SEQUENCE { OID, value }.
        let (atv_body, next) = read_seq(cursor)?;
        if atv_body.starts_with(CN_OID_DER) {
            let value_tlv = &atv_body[CN_OID_DER.len()..];
            // The value is a directory string — UTF8String (0x0C),
            // PrintableString (0x13), IA5String (0x16), …. We accept
            // any printable tag and decode the body as UTF-8 lossily.
            let (value_body, _) = read_tlv(value_tlv)?;
            return Some(String::from_utf8_lossy(value_body).into_owned());
        }
        cursor = next;
    }
    None
}

/// Read a `SEQUENCE` TLV (tag 0x30). Returns `(body, rest_after_TLV)`.
fn read_seq(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    if buf.first() != Some(&0x30) {
        return None;
    }
    read_after_tag(buf)
}

/// Read a `SET` TLV (tag 0x31). Returns `(body, rest_after_TLV)`.
fn read_set(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    if buf.first() != Some(&0x31) {
        return None;
    }
    read_after_tag(buf)
}

/// Read any TLV. Returns `(body, rest_after_TLV)`.
fn read_tlv(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    if buf.is_empty() {
        return None;
    }
    read_after_tag(buf)
}

fn read_after_tag(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    if buf.len() < 2 {
        return None;
    }
    // Skip the tag byte; parse the length.
    let (len, len_bytes) = parse_der_length(&buf[1..])?;
    let header = 1 + len_bytes;
    if buf.len() < header + len {
        return None;
    }
    let body = &buf[header..header + len];
    let rest = &buf[header + len..];
    Some((body, rest))
}

fn parse_der_length(buf: &[u8]) -> Option<(usize, usize)> {
    if buf.is_empty() {
        return None;
    }
    let first = buf[0];
    if first & 0x80 == 0 {
        return Some((first as usize, 1));
    }
    let n = (first & 0x7F) as usize;
    if n == 0 || n > 4 || buf.len() < 1 + n {
        return None;
    }
    let mut len: usize = 0;
    for &b in &buf[1..=n] {
        len = (len << 8) | b as usize;
    }
    Some((len, 1 + n))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rcgen 0.13: generate a self-signed cert whose **Subject CN** is
    /// exactly `cn`. The simple `generate_simple_self_signed` helper
    /// only stamps the SAN list and leaves the default `"rcgen self
    /// signed cert"` CN; for the CN-binding test path we need an
    /// explicit CN.
    fn mint_with_cn(cn: &str, sans: &[&str]) -> CertificateDer<'static> {
        let san_vec: Vec<String> = sans.iter().map(|s| s.to_string()).collect();
        let mut params = rcgen::CertificateParams::new(san_vec).expect("params");
        let mut name = rcgen::DistinguishedName::new();
        name.push(rcgen::DnType::CommonName, cn);
        params.distinguished_name = name;
        let key = rcgen::KeyPair::generate().expect("keygen");
        let cert = params.self_signed(&key).expect("self sign");
        CertificateDer::from(cert.der().to_vec())
    }

    #[test]
    fn cert_node_id_reads_simple_cn() {
        let der = mint_with_cn("node-alpha", &["node-alpha.test"]);
        let nid = cert_node_id(&der).expect("extract cn");
        assert_eq!(nid.as_str(), "node-alpha");
    }

    #[test]
    fn cert_node_id_reads_dashy_cn() {
        let der = mint_with_cn("east-cluster-7", &["east-cluster-7.test"]);
        let nid = cert_node_id(&der).expect("extract cn");
        assert_eq!(nid.as_str(), "east-cluster-7");
    }

    #[test]
    fn verify_peer_identity_accepts_matching_cn() {
        let der = mint_with_cn("node-b", &["node-b.test"]);
        verify_peer_identity(&der, &NodeId::new("node-b")).expect("matches");
    }

    #[test]
    fn verify_peer_identity_rejects_wrong_cn() {
        let der = mint_with_cn("node-b", &["node-b.test"]);
        let err = verify_peer_identity(&der, &NodeId::new("node-a")).unwrap_err();
        match err {
            TlsError::IdentityMismatch { cn, claimed } => {
                assert_eq!(cn, "node-b");
                assert_eq!(claimed, "node-a");
            }
            other => panic!("expected IdentityMismatch, got {other:?}"),
        }
    }
}
