//! Sealing of secrets at rest (decision 0006): XChaCha20-Poly1305 under one 32-byte key that
//! never lives in `control.sqlite`. The key comes from `V1PER_SECRETS_KEY` (base64), else
//! `V1PER_SECRETS_KEY_FILE`, else `<data-dir>/secrets.key`. Every sealed value names its
//! context, such as `monitor:<id>:password`, as associated data, so a ciphertext copied to
//! another record does not open.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};

pub const SECRETS_KEY_FILE: &str = "secrets.key";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;

/// The instance's secret key. Deliberately without `Debug`, and errors never include it.
pub struct SecretKey {
    cipher: XChaCha20Poly1305,
    bytes: [u8; KEY_BYTES],
    in_data_dir: bool,
}

impl SecretKey {
    /// The configured key: `value`, else `file`, else `secrets.key` in `dir`, which must exist.
    /// A missing data-directory key is an error, never a new key, because a new key would
    /// leave every sealed secret unreadable.
    pub fn load(dir: &Path, value: Option<&str>, file: Option<&Path>) -> io::Result<Self> {
        match (value, file) {
            (Some(_), Some(_)) => Err(invalid(
                "set V1PER_SECRETS_KEY or V1PER_SECRETS_KEY_FILE, not both",
            )),
            (Some(value), None) => Self::from_bytes(&decode(value, "V1PER_SECRETS_KEY")?),
            (None, Some(file)) => Self::from_file(file),
            (None, None) => Ok(Self {
                in_data_dir: true,
                ..Self::from_file(&dir.join(SECRETS_KEY_FILE))?
            }),
        }
    }

    /// [`SecretKey::load`], creating `secrets.key` in `dir` (mode 0600) when no key is
    /// configured and none exists yet. Only `init` creates a key.
    pub fn load_or_create(
        dir: &Path,
        value: Option<&str>,
        file: Option<&Path>,
    ) -> io::Result<Self> {
        if value.is_none() && file.is_none() {
            let path = dir.join(SECRETS_KEY_FILE);
            if !path.exists() {
                return Ok(Self {
                    in_data_dir: true,
                    ..Self::from_bytes(&create(&path)?)?
                });
            }
        }
        Self::load(dir, value, file)
    }

    fn from_file(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path).map_err(|e| {
            io::Error::new(e.kind(), format!("secrets key {}: {e}", path.display()))
        })?;
        Self::from_key_file(&bytes, &path.display().to_string())
    }

    /// The contents of a key file: 32 raw bytes or their base64. `origin` names it in errors.
    pub fn from_key_file(bytes: &[u8], origin: &str) -> io::Result<Self> {
        if bytes.len() == KEY_BYTES {
            return Self::from_bytes(bytes);
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| invalid(&format!("{origin} is not a secrets key")))?;
        Self::from_bytes(&decode(text, origin)?)
    }

    fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let bytes: [u8; KEY_BYTES] = bytes
            .try_into()
            .map_err(|_| invalid(&format!("a secrets key holds {KEY_BYTES} bytes")))?;
        Ok(Self {
            cipher: XChaCha20Poly1305::new(&bytes.into()),
            bytes,
            in_data_dir: false,
        })
    }

    /// Names the key without revealing it, so a backup can say which key it needs. A hash of
    /// 32 random bytes gives nothing about them away.
    pub fn fingerprint(&self) -> String {
        let digest = Sha256::new()
            .chain_update(b"v1per secrets key fingerprint\0")
            .chain_update(self.bytes)
            .finalize();
        digest[..16].iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Whether the key is `secrets.key` in the data directory, rather than from
    /// `V1PER_SECRETS_KEY` or a key file elsewhere.
    pub fn in_data_dir(&self) -> bool {
        self.in_data_dir
    }

    /// The raw key, for a backup the operator asked to hold it (`--include-key`).
    pub(crate) fn bytes(&self) -> &[u8; KEY_BYTES] {
        &self.bytes
    }

    /// A random nonce followed by the ciphertext and tag.
    pub fn seal(&self, context: &str, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).expect("the OS provides randomness");
        let payload = Payload {
            msg: plain,
            aad: context.as_bytes(),
        };
        let sealed = self
            .cipher
            .encrypt(&XNonce::from(nonce), payload)
            .expect("encryption of a short secret cannot fail");
        [nonce.as_slice(), &sealed].concat()
    }

    /// `None` when the ciphertext belongs to another context or another key.
    pub fn open(&self, context: &str, sealed: &[u8]) -> Option<Vec<u8>> {
        let (nonce, ciphertext) = sealed.split_at_checked(NONCE_BYTES)?;
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().ok()?;
        let payload = Payload {
            msg: ciphertext,
            aad: context.as_bytes(),
        };
        self.cipher.decrypt(&XNonce::from(nonce), payload).ok()
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// Base64 of exactly 32 bytes. The error names where the key came from, never its value.
fn decode(text: &str, origin: &str) -> io::Result<Vec<u8>> {
    STANDARD
        .decode(text.trim())
        .ok()
        .filter(|bytes| bytes.len() == KEY_BYTES)
        .ok_or_else(|| invalid(&format!("{origin} must be base64 of {KEY_BYTES} bytes")))
}

/// Writes a new random key. `create_new` makes a concurrent first start read the winner's key.
fn create(path: &Path) -> io::Result<Vec<u8>> {
    let mut key = [0u8; KEY_BYTES];
    getrandom::fill(&mut key).expect("the OS provides randomness");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key)?;
            file.sync_all()?;
            Ok(key.to_vec())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => fs::read(path),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertexts_open_only_for_their_context_and_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = SecretKey::load_or_create(dir.path(), None, None).unwrap();
        let sealed = key.seal("monitor:a:password", b"secret");
        assert_eq!(key.open("monitor:a:password", &sealed).unwrap(), b"secret");
        assert!(key.open("monitor:b:password", &sealed).is_none());

        let again = SecretKey::load(dir.path(), None, None).unwrap();
        assert_eq!(
            again.open("monitor:a:password", &sealed).unwrap(),
            b"secret"
        );
        let other = tempfile::TempDir::new().unwrap();
        let foreign = SecretKey::load_or_create(other.path(), None, None).unwrap();
        assert!(foreign.open("monitor:a:password", &sealed).is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(SECRETS_KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    /// The environment value, a base64 key file and the raw data-directory file are one key.
    #[test]
    fn every_key_source_reads_the_same_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let raw = [7u8; KEY_BYTES];
        fs::write(dir.path().join(SECRETS_KEY_FILE), raw).unwrap();
        let encoded = STANDARD.encode(raw);
        let text_file = dir.path().join("key.txt");
        fs::write(&text_file, format!("{encoded}\n")).unwrap();

        let sealed = SecretKey::load(dir.path(), None, None)
            .unwrap()
            .seal("c", b"x");
        let from_env = SecretKey::load(dir.path(), Some(&encoded), None).unwrap();
        let from_file = SecretKey::load(dir.path(), None, Some(&text_file)).unwrap();
        assert_eq!(from_env.open("c", &sealed).unwrap(), b"x");
        assert_eq!(from_file.open("c", &sealed).unwrap(), b"x");

        let empty = tempfile::TempDir::new().unwrap();
        assert!(SecretKey::load(empty.path(), None, None).is_err());
        assert!(SecretKey::load(empty.path(), Some("c2hvcnQ="), None).is_err());
        assert!(SecretKey::load(empty.path(), Some(&encoded), Some(&text_file)).is_err());
    }
}
