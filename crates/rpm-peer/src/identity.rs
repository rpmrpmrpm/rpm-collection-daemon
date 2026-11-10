//! The device key. Its public half is what readers pin, and its hash is the device id.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::elliptic_curve::Generate;
use p256::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rpm_wire::base::crockford32;
use sha2::{Digest, Sha256};

/// How many bytes of the key's hash make the device id.
const ID_BYTES: usize = 16;

/// An ECDSA P-256 key pair, the same kind `PeerIdentity` makes in `RPM.Core`.
pub struct Identity {
    signing: SigningKey,
    pkcs8: Vec<u8>,
    public_key: Vec<u8>,
    device_id: String,
}

impl Identity {
    /// A new random key.
    pub fn generate() -> Identity {
        Identity::from_signing(SigningKey::generate())
    }

    /// A key read from PKCS#8 DER, the format `RPM.Core` exports.
    pub fn from_pkcs8(der: &[u8]) -> Result<Identity, p256::pkcs8::Error> {
        Ok(Identity::from_signing(SigningKey::from_pkcs8_der(der)?))
    }

    fn from_signing(signing: SigningKey) -> Identity {
        let pkcs8 = signing
            .to_pkcs8_der()
            .expect("a P-256 key always encodes")
            .as_bytes()
            .to_vec();
        let public_key = signing
            .verifying_key()
            .to_public_key_der()
            .expect("a P-256 public key always encodes")
            .into_vec();
        let device_id = id_for(&public_key);
        Identity {
            signing,
            pkcs8,
            public_key,
            device_id,
        }
    }

    /// Reads the key at `path`, or makes one and writes it there when there is no file.
    ///
    /// The file is written to a temporary name and then renamed, with mode 0600, so a crash while
    /// writing never leaves a half-written key. A key that cannot be read is an error, not a reason to
    /// make a new one: a new key is a new device id, and every reader would stop trusting this NAS.
    pub fn load_or_create(path: &Path) -> io::Result<Identity> {
        match fs::read(path) {
            Ok(der) => Identity::from_pkcs8(&der).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not a key: {e}", path.display()),
                )
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let identity = Identity::generate();
                write_private(path, identity.pkcs8())?;
                Ok(identity)
            }
            Err(e) => Err(e),
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The public key as SubjectPublicKeyInfo DER.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// The private key as PKCS#8 DER.
    pub fn pkcs8(&self) -> &[u8] {
        &self.pkcs8
    }

    /// ECDSA with SHA-256 over `data`, as 64 bytes: r then s. This is the format .NET's `SignData`
    /// writes and reads.
    pub fn sign(&self, data: &[u8]) -> [u8; 64] {
        let signature: Signature = self.signing.sign(data);
        signature.to_bytes().into()
    }
}

/// Whether `signature` (64 bytes, r then s) is a signature over `data` by the key in `spki`.
pub fn verify(spki: &[u8], data: &[u8], signature: &[u8]) -> bool {
    let Ok(key) = VerifyingKey::from_public_key_der(spki) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(signature) else {
        return false;
    };
    key.verify(data, &signature).is_ok()
}

/// The device id for a public key: Crockford base32 of the first 16 bytes of its SHA-256.
pub fn id_for(spki: &[u8]) -> String {
    crockford32(&Sha256::digest(spki)[..ID_BYTES])
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}
