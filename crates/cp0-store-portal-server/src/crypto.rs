use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const KEY_BYTES: usize = 32;
const XNONCE_BYTES: usize = 24;
const PKCE_AAD: &[u8] = b"cp0-portal-pkce-v1";
const INVITATION_AAD: &[u8] = b"cp0-portal-invitation-v1";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug)]
pub enum CryptoError {
    InvalidKey,
    RandomUnavailable,
    Encryption,
    Decryption,
}

#[derive(Clone)]
pub struct PortalSecrets {
    csrf_key: Zeroizing<[u8; KEY_BYTES]>,
    nonce_key: Zeroizing<[u8; KEY_BYTES]>,
    pkce_key: Zeroizing<[u8; KEY_BYTES]>,
    subject_key: Zeroizing<[u8; KEY_BYTES]>,
    invitation_key: Zeroizing<[u8; KEY_BYTES]>,
}

impl PortalSecrets {
    pub fn from_base64(
        csrf_key: &str,
        nonce_key: &str,
        pkce_key: &str,
        subject_key: &str,
        invitation_key: &str,
    ) -> Result<Self, CryptoError> {
        let csrf_key = decode_key(csrf_key)?;
        let nonce_key = decode_key(nonce_key)?;
        let pkce_key = decode_key(pkce_key)?;
        let subject_key = decode_key(subject_key)?;
        let invitation_key = decode_key(invitation_key)?;
        let keys = [
            csrf_key.as_ref(),
            nonce_key.as_ref(),
            pkce_key.as_ref(),
            subject_key.as_ref(),
            invitation_key.as_ref(),
        ];
        for (index, key) in keys.iter().enumerate() {
            if keys[..index].contains(key) {
                return Err(CryptoError::InvalidKey);
            }
        }
        Ok(Self {
            csrf_key,
            nonce_key,
            pkce_key,
            subject_key,
            invitation_key,
        })
    }

    pub fn random_token(&self) -> Result<String, CryptoError> {
        let mut bytes = Zeroizing::new([0_u8; KEY_BYTES]);
        getrandom::fill(bytes.as_mut()).map_err(|_| CryptoError::RandomUnavailable)?;
        Ok(URL_SAFE_NO_PAD.encode(bytes.as_ref()))
    }

    pub fn csrf_for_session(&self, session_secret: &str) -> String {
        derive_token(self.csrf_key.as_ref(), b"csrf", session_secret.as_bytes())
    }

    pub fn nonce_for_state(&self, state: &str) -> String {
        derive_token(self.nonce_key.as_ref(), b"nonce", state.as_bytes())
    }

    pub fn state_for_action(
        &self,
        session_sha256: &str,
        operation: &str,
        idempotency_key: &str,
    ) -> String {
        let mut value =
            Vec::with_capacity(session_sha256.len() + operation.len() + idempotency_key.len() + 2);
        value.extend_from_slice(session_sha256.as_bytes());
        value.push(0);
        value.extend_from_slice(operation.as_bytes());
        value.push(0);
        value.extend_from_slice(idempotency_key.as_bytes());
        derive_token(self.nonce_key.as_ref(), b"state", &value)
    }

    pub fn subject_hmac(&self, issuer: &str, subject: &str) -> String {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(self.subject_key.as_ref())
            .expect("fixed HMAC key length");
        mac.update(b"subject\0");
        mac.update(issuer.as_bytes());
        mac.update(&[0]);
        mac.update(subject.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    pub fn encrypt_pkce(&self, verifier: &str) -> Result<Vec<u8>, CryptoError> {
        encrypt(&self.pkce_key, PKCE_AAD, verifier)
    }

    pub fn decrypt_pkce(&self, envelope: &[u8]) -> Result<Zeroizing<String>, CryptoError> {
        decrypt(&self.pkce_key, PKCE_AAD, envelope)
    }

    pub fn encrypt_invitation_token(
        &self,
        invitation_id: &str,
        token: &str,
    ) -> Result<Vec<u8>, CryptoError> {
        let aad = invitation_aad(invitation_id);
        encrypt(&self.invitation_key, &aad, token)
    }

    pub fn decrypt_invitation_token(
        &self,
        invitation_id: &str,
        envelope: &[u8],
    ) -> Result<Zeroizing<String>, CryptoError> {
        let aad = invitation_aad(invitation_id);
        decrypt(&self.invitation_key, &aad, envelope)
    }
}

fn encrypt(
    key: &Zeroizing<[u8; KEY_BYTES]>,
    aad: &[u8],
    cleartext: &str,
) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0_u8; XNONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::RandomUnavailable)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let encrypted = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: cleartext.as_bytes(),
                aad,
            },
        )
        .map_err(|_| CryptoError::Encryption)?;
    let mut envelope = Vec::with_capacity(XNONCE_BYTES + encrypted.len());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&encrypted);
    Ok(envelope)
}

fn decrypt(
    key: &Zeroizing<[u8; KEY_BYTES]>,
    aad: &[u8],
    envelope: &[u8],
) -> Result<Zeroizing<String>, CryptoError> {
    if envelope.len() <= XNONCE_BYTES {
        return Err(CryptoError::Decryption);
    }
    let (nonce, ciphertext) = envelope.split_at(XNONCE_BYTES);
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let cleartext = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Decryption)?;
    let cleartext = String::from_utf8(cleartext).map_err(|_| CryptoError::Decryption)?;
    Ok(Zeroizing::new(cleartext))
}

fn invitation_aad(invitation_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(INVITATION_AAD.len() + invitation_id.len() + 1);
    aad.extend_from_slice(INVITATION_AAD);
    aad.push(0);
    aad.extend_from_slice(invitation_id.as_bytes());
    aad
}

pub fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn decode_key(encoded: &str) -> Result<Zeroizing<[u8; KEY_BYTES]>, CryptoError> {
    let mut decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| CryptoError::InvalidKey)?,
    );
    let key: [u8; KEY_BYTES] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidKey)?;
    decoded.fill(0);
    Ok(Zeroizing::new(key))
}

fn derive_token(key: &[u8], domain: &[u8], value: &[u8]) -> String {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("fixed HMAC key length");
    mac.update(domain);
    mac.update(&[0]);
    mac.update(value);
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> PortalSecrets {
        let csrf = URL_SAFE_NO_PAD.encode([7_u8; KEY_BYTES]);
        let nonce = URL_SAFE_NO_PAD.encode([8_u8; KEY_BYTES]);
        let pkce = URL_SAFE_NO_PAD.encode([9_u8; KEY_BYTES]);
        let subject = URL_SAFE_NO_PAD.encode([10_u8; KEY_BYTES]);
        let invitation = URL_SAFE_NO_PAD.encode([11_u8; KEY_BYTES]);
        PortalSecrets::from_base64(&csrf, &nonce, &pkce, &subject, &invitation).unwrap()
    }

    #[test]
    fn derives_domain_separated_tokens_and_subjects() {
        let secrets = secrets();
        assert_ne!(
            secrets.csrf_for_session("same"),
            secrets.nonce_for_state("same")
        );
        assert_eq!(
            secrets.state_for_action("session", "step-up", "request-key"),
            secrets.state_for_action("session", "step-up", "request-key")
        );
        assert_ne!(
            secrets.state_for_action("session", "step-up", "request-key"),
            secrets.state_for_action("session", "link", "request-key")
        );
        assert_ne!(
            secrets.subject_hmac("https://issuer-a.example", "subject"),
            secrets.subject_hmac("https://issuer-b.example", "subject")
        );
    }

    #[test]
    fn pkce_envelope_round_trips_and_rejects_tampering() {
        let secrets = secrets();
        let verifier = "v".repeat(43);
        let mut envelope = secrets.encrypt_pkce(&verifier).unwrap();
        assert_eq!(&*secrets.decrypt_pkce(&envelope).unwrap(), &verifier);
        let last = envelope.last_mut().unwrap();
        *last ^= 1;
        assert!(matches!(
            secrets.decrypt_pkce(&envelope),
            Err(CryptoError::Decryption)
        ));
    }

    #[test]
    fn rejects_reused_purpose_keys() {
        let key = URL_SAFE_NO_PAD.encode([7_u8; KEY_BYTES]);
        assert!(matches!(
            PortalSecrets::from_base64(&key, &key, &key, &key, &key),
            Err(CryptoError::InvalidKey)
        ));
    }

    #[test]
    fn invitation_and_pkce_envelopes_are_domain_separated() {
        let secrets = secrets();
        let token = "t".repeat(43);
        let invitation_id = "invite_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let invitation = secrets
            .encrypt_invitation_token(invitation_id, &token)
            .unwrap();
        assert_eq!(
            &*secrets
                .decrypt_invitation_token(invitation_id, &invitation)
                .unwrap(),
            &token
        );
        assert!(secrets.decrypt_pkce(&invitation).is_err());
        assert!(
            secrets
                .decrypt_invitation_token("invite_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", &invitation)
                .is_err()
        );
    }
}
