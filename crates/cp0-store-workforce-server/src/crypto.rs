use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const KEY_BYTES: usize = 32;
const XNONCE_BYTES: usize = 24;
const PKCE_AAD: &[u8] = b"cp0-workforce-pkce-v1";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug)]
pub enum CryptoError {
    InvalidKey,
    RandomUnavailable,
    Encryption,
    Decryption,
}

#[derive(Clone)]
pub struct WorkforceSecrets {
    csrf_key: Zeroizing<[u8; KEY_BYTES]>,
    nonce_key: Zeroizing<[u8; KEY_BYTES]>,
    pkce_key: Zeroizing<[u8; KEY_BYTES]>,
    subject_key: Zeroizing<[u8; KEY_BYTES]>,
    control_token_key: Zeroizing<[u8; KEY_BYTES]>,
}

impl WorkforceSecrets {
    pub fn from_base64(
        csrf_key: &str,
        nonce_key: &str,
        pkce_key: &str,
        subject_key: &str,
        control_token_key: &str,
    ) -> Result<Self, CryptoError> {
        let csrf_key = decode_key(csrf_key)?;
        let nonce_key = decode_key(nonce_key)?;
        let pkce_key = decode_key(pkce_key)?;
        let subject_key = decode_key(subject_key)?;
        let control_token_key = decode_key(control_token_key)?;
        let keys = [
            csrf_key.as_ref(),
            nonce_key.as_ref(),
            pkce_key.as_ref(),
            subject_key.as_ref(),
            control_token_key.as_ref(),
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
            control_token_key,
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

    pub fn subject_hmac(&self, issuer: &str, subject: &str) -> String {
        let mut value = Vec::with_capacity(issuer.len() + subject.len() + 1);
        value.extend_from_slice(issuer.as_bytes());
        value.push(0);
        value.extend_from_slice(subject.as_bytes());
        derive_hex(self.subject_key.as_ref(), b"subject", &value)
    }

    pub fn control_token(
        &self,
        session_sha256: &str,
        audience: &str,
        scope: &str,
        idempotency_key: &str,
    ) -> String {
        let mut value = Vec::with_capacity(
            session_sha256.len() + audience.len() + scope.len() + idempotency_key.len() + 3,
        );
        for part in [session_sha256, audience, scope, idempotency_key] {
            if !value.is_empty() {
                value.push(0);
            }
            value.extend_from_slice(part.as_bytes());
        }
        derive_token(self.control_token_key.as_ref(), b"control-token", &value)
    }

    pub fn encrypt_pkce(&self, verifier: &str) -> Result<Vec<u8>, CryptoError> {
        let mut nonce = [0_u8; XNONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| CryptoError::RandomUnavailable)?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(self.pkce_key.as_ref()));
        let encrypted = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: verifier.as_bytes(),
                    aad: PKCE_AAD,
                },
            )
            .map_err(|_| CryptoError::Encryption)?;
        let mut envelope = Vec::with_capacity(XNONCE_BYTES + encrypted.len());
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&encrypted);
        Ok(envelope)
    }

    pub fn decrypt_pkce(&self, envelope: &[u8]) -> Result<Zeroizing<String>, CryptoError> {
        if envelope.len() <= XNONCE_BYTES {
            return Err(CryptoError::Decryption);
        }
        let (nonce, ciphertext) = envelope.split_at(XNONCE_BYTES);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(self.pkce_key.as_ref()));
        let cleartext = cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: PKCE_AAD,
                },
            )
            .map_err(|_| CryptoError::Decryption)?;
        let cleartext = String::from_utf8(cleartext).map_err(|_| CryptoError::Decryption)?;
        Ok(Zeroizing::new(cleartext))
    }
}

pub fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
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
    URL_SAFE_NO_PAD.encode(derive(key, domain, value))
}

fn derive_hex(key: &[u8], domain: &[u8], value: &[u8]) -> String {
    hex(&derive(key, domain, value))
}

fn derive(key: &[u8], domain: &[u8], value: &[u8]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("fixed HMAC key length");
    mac.update(domain);
    mac.update(&[0]);
    mac.update(value);
    mac.finalize().into_bytes().into()
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

    fn secrets() -> WorkforceSecrets {
        WorkforceSecrets::from_base64(
            &URL_SAFE_NO_PAD.encode([1_u8; 32]),
            &URL_SAFE_NO_PAD.encode([2_u8; 32]),
            &URL_SAFE_NO_PAD.encode([3_u8; 32]),
            &URL_SAFE_NO_PAD.encode([4_u8; 32]),
            &URL_SAFE_NO_PAD.encode([5_u8; 32]),
        )
        .unwrap()
    }

    #[test]
    fn derives_domain_separated_browser_secrets() {
        let secrets = secrets();
        let session = URL_SAFE_NO_PAD.encode([9_u8; 32]);
        assert_ne!(
            secrets.csrf_for_session(&session),
            secrets.nonce_for_state(&session)
        );
        assert_ne!(
            secrets.control_token(&"a".repeat(64), "review", "store.review", "request-key"),
            secrets.control_token(
                &"a".repeat(64),
                "operations",
                "store.editorial",
                "request-key"
            )
        );
        assert_eq!(
            secrets.subject_hmac("https://issuer.example", "subject"),
            secrets.subject_hmac("https://issuer.example", "subject")
        );
    }

    #[test]
    fn pkce_envelope_round_trips_and_rejects_tampering() {
        let secrets = secrets();
        let verifier = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let encrypted = secrets.encrypt_pkce(&verifier).unwrap();
        assert_eq!(&*secrets.decrypt_pkce(&encrypted).unwrap(), &verifier);
        let mut tampered = encrypted;
        *tampered.last_mut().unwrap() ^= 1;
        assert!(secrets.decrypt_pkce(&tampered).is_err());
    }
}
