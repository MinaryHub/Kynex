//! What a secure OPC UA channel takes, for the Basic256Sha256 policy
//! (OPC 10000-7): KYNEX's own application certificate and key, the server's
//! certificate, and the algorithms — RSA-OAEP with SHA-1 and RSA PKCS #1
//! v1.5 with SHA-256 for the channel's opening, HMAC-SHA256 and AES-256-CBC
//! for its messages, P_SHA256 for the keys between them (OPC 10000-6). The
//! algorithms are the RustCrypto crates'; nothing here is cryptography of
//! KYNEX's own making.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rand::RngCore;
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::signature::{RandomizedSigner, SignatureEncoding, Verifier};
use rsa::traits::PublicKeyParts;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::asn1::Ia5String;
use x509_cert::der::{Decode, Encode};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{ExtendedKeyUsage, KeyUsage, KeyUsages, SubjectAltName};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{DecodePublicKey, SubjectPublicKeyInfoOwned};
use x509_cert::time::Validity;
use x509_cert::Certificate;

/// The Basic256Sha256 policy's URI.
pub const BASIC256SHA256: &str = "http://opcfoundation.org/UA/SecurityPolicy#Basic256Sha256";
/// KYNEX's application URI: its certificate says it, and its session does.
pub const APPLICATION_URI: &str = "urn:kynex:client";
/// The algorithm a session's signature is made with, as the policy names
/// it.
pub const SIGNATURE_ALGORITHM: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256";
/// The algorithm a user's password is encrypted with.
pub const PASSWORD_ALGORITHM: &str = "http://www.w3.org/2001/04/xmlenc#rsa-oaep";

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// Random bytes, a nonce's.
pub fn nonce() -> [u8; 32] {
    let mut out = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut out);
    out
}

/// P_SHA256 (RFC 5246's P_hash): `len` bytes from `secret` and `seed`.
pub fn p_sha256(secret: &[u8], seed: &[u8], len: usize) -> Vec<u8> {
    let hmac = |data: &[&[u8]]| {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("any key length");
        for d in data {
            m.update(d);
        }
        m.finalize().into_bytes().to_vec()
    };
    let mut out = Vec::with_capacity(len + 32);
    let mut a = hmac(&[seed]);
    while out.len() < len {
        out.extend(hmac(&[&a, seed]));
        a = hmac(&[&a]);
    }
    out.truncate(len);
    out
}

/// One side's keys for a channel's messages.
#[derive(Clone, Debug, PartialEq)]
pub struct Keys {
    pub sign: [u8; 32],
    pub encrypt: [u8; 32],
    pub iv: [u8; 16],
}

impl Keys {
    /// The keys P_SHA256 makes from `secret` and `seed`: the client's from
    /// the server's nonce and its own, the server's the other way round.
    pub fn derive(secret: &[u8], seed: &[u8]) -> Self {
        let bytes = p_sha256(secret, seed, 32 + 32 + 16);
        Self {
            sign: bytes[..32].try_into().unwrap(),
            encrypt: bytes[32..64].try_into().unwrap(),
            iv: bytes[64..].try_into().unwrap(),
        }
    }

    /// HMAC-SHA256 of `data`.
    pub fn mac(&self, data: &[u8]) -> [u8; 32] {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&self.sign).expect("32 bytes");
        m.update(data);
        m.finalize().into_bytes().into()
    }

    /// Whether `mac` is `data`'s, compared in constant time.
    pub fn verify(&self, data: &[u8], mac: &[u8]) -> bool {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&self.sign).expect("32 bytes");
        m.update(data);
        m.verify_slice(mac).is_ok()
    }

    /// AES-256-CBC of `data`, whole blocks of it, in place.
    pub fn encrypt(&self, data: &mut [u8]) -> Result<(), String> {
        if data.len() % 16 != 0 {
            return Err("OPC UA: 암호화할 길이가 블록 단위가 아닙니다".into());
        }
        let mut c = Aes256CbcEnc::new(&self.encrypt.into(), &self.iv.into());
        for block in data.chunks_exact_mut(16) {
            c.encrypt_block_mut(block.into());
        }
        Ok(())
    }

    pub fn decrypt(&self, data: &mut [u8]) -> Result<(), String> {
        if data.len() % 16 != 0 {
            return Err("OPC UA: 암호문 길이가 블록 단위가 아닙니다".into());
        }
        let mut c = Aes256CbcDec::new(&self.encrypt.into(), &self.iv.into());
        for block in data.chunks_exact_mut(16) {
            c.decrypt_block_mut(block.into());
        }
        Ok(())
    }
}

/// A certificate KYNEX was given: the server's.
#[derive(Clone, Debug)]
pub struct Peer {
    pub der: Vec<u8>,
    pub key: RsaPublicKey,
}

impl Peer {
    /// The certificate in `der`, and its RSA public key.
    pub fn of(der: &[u8]) -> Result<Self, String> {
        let cert = Certificate::from_der(der)
            .map_err(|e| format!("OPC UA: 서버 인증서를 읽지 못했습니다: {e}"))?;
        let spki = cert
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(|e| e.to_string())?;
        let key = RsaPublicKey::from_public_key_der(&spki)
            .map_err(|e| format!("OPC UA: 서버 인증서의 키가 RSA가 아닙니다: {e}"))?;
        Ok(Self {
            der: der.to_vec(),
            key,
        })
    }

    /// Its SHA-1 thumbprint, as OPC UA names a certificate.
    pub fn thumbprint(&self) -> [u8; 20] {
        thumbprint(&self.der)
    }

    pub fn key_bytes(&self) -> usize {
        self.key.size()
    }

    /// RSA-OAEP (SHA-1) of `data`, block by block.
    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let block = self.key_bytes() - 42;
        let mut out = Vec::with_capacity(data.len().div_ceil(block) * self.key_bytes());
        let mut rng = rand::thread_rng();
        for chunk in data.chunks(block) {
            out.extend(
                self.key
                    .encrypt(&mut rng, Oaep::new::<Sha1>(), chunk)
                    .map_err(|e| format!("OPC UA: 암호화 실패: {e}"))?,
            );
        }
        Ok(out)
    }

    /// Whether `signature` is `data`'s, made with the certificate's key
    /// (RSA PKCS #1 v1.5, SHA-256).
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        let Ok(sig) = Signature::try_from(signature) else {
            return false;
        };
        VerifyingKey::<Sha256>::new(self.key.clone())
            .verify(data, &sig)
            .is_ok()
    }
}

/// `der`'s SHA-1 thumbprint.
pub fn thumbprint(der: &[u8]) -> [u8; 20] {
    Sha1::digest(der).into()
}

/// A thumbprint in hex, as a person compares it.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// KYNEX's own application instance: its key, and the certificate that
/// says it is KYNEX, made once and kept.
pub struct Identity {
    key: RsaPrivateKey,
    pub der: Vec<u8>,
}

impl Identity {
    /// The identity kept in `dir`, or a new one made and kept there: an
    /// RSA-2048 key, and a certificate signed with it, good for twenty
    /// years, its application URI KYNEX's.
    pub fn load_or_create(dir: &Path) -> Result<Self, String> {
        let (key_file, cert_file) = (dir.join("kynex_key.pem"), dir.join("kynex_cert.der"));
        if let (Ok(pem), Ok(der)) = (
            std::fs::read_to_string(&key_file),
            std::fs::read(&cert_file),
        ) {
            let key = RsaPrivateKey::from_pkcs8_pem(&pem)
                .map_err(|e| format!("{}: {e}", key_file.display()))?;
            return Ok(Self { key, der });
        }
        let identity = Self::create()?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let pem = identity
            .key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| e.to_string())?;
        std::fs::write(&key_file, pem.as_bytes())
            .map_err(|e| format!("{}: {e}", key_file.display()))?;
        std::fs::write(&cert_file, &identity.der)
            .map_err(|e| format!("{}: {e}", cert_file.display()))?;
        Ok(identity)
    }

    fn create() -> Result<Self, String> {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).map_err(|e| e.to_string())?;
        let signer = SigningKey::<Sha256>::new(key.clone());
        let spki =
            SubjectPublicKeyInfoOwned::from_key(key.to_public_key()).map_err(|e| e.to_string())?;
        let subject = Name::from_str("CN=KYNEX,O=KYNEX").map_err(|e| e.to_string())?;
        let mut serial = [0u8; 16];
        rng.fill_bytes(&mut serial);
        serial[0] &= 0x7F;
        let serial = SerialNumber::new(&serial).map_err(|e| e.to_string())?;
        let validity = Validity::from_now(Duration::from_secs(20 * 365 * 24 * 3600))
            .map_err(|e| e.to_string())?;
        let mut builder = CertificateBuilder::new(
            Profile::Manual { issuer: None },
            serial,
            validity,
            subject,
            spki,
            &signer,
        )
        .map_err(|e| e.to_string())?;
        let host = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "localhost".into());
        let mut names = vec![GeneralName::UniformResourceIdentifier(
            Ia5String::new(APPLICATION_URI).map_err(|e| e.to_string())?,
        )];
        if let Ok(dns) = Ia5String::new(&host) {
            names.push(GeneralName::DnsName(dns));
        }
        builder
            .add_extension(&SubjectAltName(names))
            .map_err(|e| e.to_string())?;
        builder
            .add_extension(&KeyUsage(
                KeyUsages::DigitalSignature
                    | KeyUsages::NonRepudiation
                    | KeyUsages::KeyEncipherment
                    | KeyUsages::DataEncipherment,
            ))
            .map_err(|e| e.to_string())?;
        builder
            .add_extension(&ExtendedKeyUsage(vec![
                const_oid::db::rfc5280::ID_KP_CLIENT_AUTH,
                const_oid::db::rfc5280::ID_KP_SERVER_AUTH,
            ]))
            .map_err(|e| e.to_string())?;
        let cert = builder
            .build_with_rng::<rsa::pkcs1v15::Signature>(&mut rng)
            .map_err(|e| e.to_string())?;
        let der = cert.to_der().map_err(|e| e.to_string())?;
        Ok(Self { key, der })
    }

    pub fn key_bytes(&self) -> usize {
        self.key.size()
    }

    #[cfg(test)]
    pub fn thumbprint(&self) -> [u8; 20] {
        thumbprint(&self.der)
    }

    /// RSA PKCS #1 v1.5 (SHA-256) of `data`.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        SigningKey::<Sha256>::new(self.key.clone())
            .sign_with_rng(&mut rand::thread_rng(), data)
            .to_vec()
    }

    /// RSA-OAEP (SHA-1) undone on `data`, block by block.
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let block = self.key_bytes();
        if data.len() % block != 0 {
            return Err("OPC UA: 암호문 길이가 키 블록 단위가 아닙니다".into());
        }
        let mut out = Vec::with_capacity(data.len());
        for chunk in data.chunks(block) {
            out.extend(
                self.key
                    .decrypt(Oaep::new::<Sha1>(), chunk)
                    .map_err(|_| "OPC UA: 복호화 실패".to_string())?,
            );
        }
        Ok(out)
    }
}

/// Where KYNEX keeps its identity: beside its settings for this user.
pub fn pki_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("kynex").join("pki")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P_SHA256 as RFC 5246 defines it, against Python's own hmac module:
    /// `p_sha256(b"secret", b"seed", 80)`.
    #[test]
    fn p_sha256_is_the_tls_prf() {
        let out = p_sha256(b"secret", b"seed", 80);
        assert_eq!(
            hex(&out),
            "8E4D932530D765A0AAE974C304735ECC1202A819F80ADBD5AD09C1A34FC06918\
             E3D07795214D94C6A1976CAEA5A0B644C5B04D1AD3E09C6111C37AFC00DF0B6D\
             134279F973813359AD8BE59625D3BA60"
        );
    }

    /// A message encrypted and signed with one side's keys comes back with
    /// the same keys; a byte changed fails its check.
    #[test]
    fn keys_encrypt_and_sign_both_ways() {
        let keys = Keys::derive(&nonce(), &nonce());
        let mut data = *b"0123456789abcdef0123456789abcdef";
        let mac = keys.mac(&data);
        keys.encrypt(&mut data).unwrap();
        assert_ne!(&data, b"0123456789abcdef0123456789abcdef");
        keys.decrypt(&mut data).unwrap();
        assert_eq!(&data, b"0123456789abcdef0123456789abcdef");
        assert!(keys.verify(&data, &mac));
        data[0] ^= 1;
        assert!(!keys.verify(&data, &mac));
    }

    /// An identity made, kept and read back the same; its certificate
    /// read as a peer's, its key encrypting to it and checking its
    /// signatures.
    #[test]
    fn an_identity_is_made_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("kynex-pki-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let made = Identity::load_or_create(&dir).unwrap();
        let again = Identity::load_or_create(&dir).unwrap();
        assert_eq!(made.der, again.der);
        let peer = Peer::of(&made.der).unwrap();
        assert_eq!(peer.key_bytes(), 256);
        assert_eq!(peer.thumbprint(), made.thumbprint());
        let secret = vec![7u8; 300];
        let sealed = peer.encrypt(&secret).unwrap();
        assert_eq!(sealed.len(), 2 * 256);
        assert_eq!(again.decrypt(&sealed).unwrap(), secret);
        let sig = again.sign(b"hello");
        assert!(peer.verify(b"hello", &sig));
        assert!(!peer.verify(b"hullo", &sig));
        // Its application URI, as a server checks it.
        let cert = Certificate::from_der(&made.der).unwrap();
        let uri = cert.tbs_certificate.extensions.unwrap().iter().any(|e| {
            e.extn_value
                .as_bytes()
                .windows(16)
                .any(|w| w == APPLICATION_URI.as_bytes())
        });
        assert!(uri);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
