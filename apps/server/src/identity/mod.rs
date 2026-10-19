//! Who is calling: session cookies, API keys, password hashing, second factors, sealed secrets
//! and the account records behind them. Owns `auth.*`, `account.*`, `twoFactor.*` and
//! `apiKeys.*`.

mod account;
mod api_keys;
mod auth;
mod caller;
mod limiter;
pub mod secrets;
pub mod totp;
pub(crate) mod two_factor;

pub(crate) use auth::start_session;
pub use caller::{Caller, KeyCaller, has_api_key, resolve_caller};
pub use limiter::{ChallengeAttempts, RateLimiter};
pub use secrets::SECRETS_KEY_FILE;

use std::sync::OnceLock;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::UserId;
use v1per_protocol::limits::{
    EMAIL_MAX_CHARS, NAME_MAX_CHARS, PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS,
};
use v1per_protocol::operations::auth::Account;

use crate::db::new_id;
use crate::error::{ApiError, FieldErrors};

pub const SESSION_COOKIE: &str = "v1per_session";
/// Carries the login challenge between a correct password and the second factor.
pub const CHALLENGE_COOKIE: &str = "v1per_challenge";
const GENERATED_PASSWORD_CHARS: usize = 24;

/// A new random token (256 bits, base64url) and its digest, the only form stored. Used for
/// sessions, challenges, key secrets and invitation codes.
pub fn new_session_token() -> (String, String) {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS provides randomness");
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let digest = token_digest(&token);
    (token, digest)
}

/// SHA-256 hex of a high-entropy token: session IDs, challenges, key secrets, codes.
pub fn token_digest(token: &str) -> String {
    let hash = Sha256::digest(token.as_bytes());
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn hash_password(password: &str) -> String {
    Argon2::default()
        .hash_password(password.as_bytes())
        .expect("argon2 hashes any password with default parameters")
        .to_string()
}

/// Runs on the blocking pool: argon2 is deliberately slow. Accounts imported from Kuma keep
/// their bcrypt hash until their first sign-in replaces it ([`is_imported_hash`], 0007).
pub async fn verify_password(hash: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || {
        if is_imported_hash(&hash) {
            return bcrypt::verify(password.as_bytes(), &hash).unwrap_or(false);
        }
        Argon2::default()
            .verify_password(password.as_bytes(), hash.as_str())
            .is_ok()
    })
    .await
    .unwrap_or(false)
}

/// A bcrypt hash as Kuma stores it, replaced with argon2 at the next successful sign-in.
pub fn is_imported_hash(hash: &str) -> bool {
    hash.starts_with("$2")
}

/// Verified when the email is unknown, so both failures take the same time.
fn decoy_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("decoy password that never matches"))
        .clone()
}

/// 24 characters from a 62-character alphabet, about 143 bits.
pub fn generate_password() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut password = String::with_capacity(GENERATED_PASSWORD_CHARS);
    while password.len() < GENERATED_PASSWORD_CHARS {
        let mut byte = [0u8; 1];
        getrandom::fill(&mut byte).expect("the OS provides randomness");
        // 248 is the largest multiple of 62 below 256, so every character is equally likely.
        if byte[0] < 248 {
            password.push(ALPHABET[(byte[0] % 62) as usize] as char);
        }
    }
    password
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Inserts an account with an argon2 `password_hash` (from [`hash_password`]).
pub async fn create_user(
    conn: &mut SqliteConnection,
    email: &str,
    name: &str,
    password_hash: &str,
    instance_admin: bool,
    now: i64,
) -> Result<UserId, sqlx::Error> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO users (id, email, name, password_hash, instance_admin, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(normalize_email(email))
    .bind(name)
    .bind(password_hash)
    .bind(instance_admin)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(UserId::new(id))
}

#[derive(sqlx::FromRow)]
struct AccountRow {
    id: String,
    email: String,
    name: String,
    instance_admin: bool,
}

impl From<AccountRow> for Account {
    fn from(row: AccountRow) -> Self {
        Account {
            id: UserId::new(row.id),
            email: row.email,
            name: row.name,
            instance_admin: row.instance_admin,
        }
    }
}

pub async fn account(
    conn: &mut SqliteConnection,
    user: &UserId,
) -> Result<Option<Account>, sqlx::Error> {
    let row: Option<AccountRow> =
        sqlx::query_as("SELECT id, email, name, instance_admin FROM users WHERE id = ?")
            .bind(user.as_str())
            .fetch_optional(conn)
            .await?;
    Ok(row.map(Account::from))
}

/// The account and password hash for an email, or the decoy hash when the email is unknown.
async fn credentials(
    conn: &mut SqliteConnection,
    email: &str,
) -> Result<(Option<Account>, String), sqlx::Error> {
    let row: Option<(String, String, String, bool, String)> = sqlx::query_as(
        "SELECT id, email, name, instance_admin, password_hash FROM users WHERE email = ?",
    )
    .bind(normalize_email(email))
    .fetch_optional(conn)
    .await?;
    Ok(match row {
        Some((id, email, name, instance_admin, hash)) => (
            Some(Account::from(AccountRow {
                id,
                email,
                name,
                instance_admin,
            })),
            hash,
        ),
        None => (None, decoy_hash()),
    })
}

/// A person's second factor: confirmed or not, and whether a node requires one.
pub(crate) struct TwoFactorState {
    pub enrolled: bool,
    pub required: bool,
}

/// 2FA is required when a node the person holds a grant on, or an ancestor of it, requires it.
/// Instance admins count as holding the root. A requirement on a node below someone's grants
/// does not reach them: it follows the tree downward, as grants do.
pub(crate) async fn two_factor_state(
    conn: &mut SqliteConnection,
    user: &UserId,
) -> Result<TwoFactorState, sqlx::Error> {
    let (enrolled, required): (bool, bool) = sqlx::query_as(
        "WITH RECURSIVE up(id) AS (
             SELECT node_id FROM grants WHERE user_id = ?1
             UNION
             SELECT n.id FROM nodes n JOIN users u ON u.id = ?1
              WHERE n.parent_id IS NULL AND u.instance_admin = 1
             UNION
             SELECT n.parent_id FROM nodes n JOIN up ON n.id = up.id WHERE n.parent_id IS NOT NULL
         )
         SELECT EXISTS (SELECT 1 FROM two_factor WHERE user_id = ?1 AND secret IS NOT NULL),
                EXISTS (SELECT 1 FROM nodes n JOIN up ON n.id = up.id WHERE n.two_factor_required = 1)",
    )
    .bind(user.as_str())
    .fetch_one(conn)
    .await?;
    Ok(TwoFactorState { enrolled, required })
}

/// The password hash of an existing account.
async fn password_hash(conn: &mut SqliteConnection, user: &UserId) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.as_str())
        .fetch_one(conn)
        .await
}

/// [`hash_password`] on the blocking pool, so a command does not stall other requests.
pub async fn hash_password_blocking(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(ApiError::internal)
}

/// Ends every session of `user` except `keep`, and any login challenge between password and
/// second factor.
async fn end_sessions(
    conn: &mut SqliteConnection,
    user: &UserId,
    keep: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id IS NOT ?")
        .bind(user.as_str())
        .bind(keep)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM login_challenges WHERE user_id = ?")
        .bind(user.as_str())
        .execute(conn)
        .await?;
    Ok(())
}

/// `invalid_arguments` on `field` unless the password has 12 to 256 characters.
pub fn validate_password(field: &str, password: &str) -> Result<(), ApiError> {
    match password.chars().count() {
        n if n < PASSWORD_MIN_CHARS => Err(FieldErrors::one(
            field,
            FieldErrorCode::TooShort,
            Some(PASSWORD_MIN_CHARS as u32),
        )),
        n if n > PASSWORD_MAX_CHARS => Err(FieldErrors::one(
            field,
            FieldErrorCode::TooLong,
            Some(PASSWORD_MAX_CHARS as u32),
        )),
        _ => Ok(()),
    }
}

/// A trimmed name of 1 to 100 characters, or `invalid_arguments` on `field`.
pub fn validate_name(field: &str, name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    match name.chars().count() {
        0 => Err(FieldErrors::one(field, FieldErrorCode::Required, None)),
        n if n > NAME_MAX_CHARS => Err(FieldErrors::one(
            field,
            FieldErrorCode::TooLong,
            Some(NAME_MAX_CHARS as u32),
        )),
        _ => Ok(name.to_owned()),
    }
}

/// A normalized email with something on both sides of `@`, or `invalid_arguments` on `field`.
pub fn validate_email(field: &str, email: &str) -> Result<String, ApiError> {
    let email = normalize_email(email);
    let well_formed = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
        && !email.chars().any(char::is_whitespace);
    if email.is_empty() {
        Err(FieldErrors::one(field, FieldErrorCode::Required, None))
    } else if email.chars().count() > EMAIL_MAX_CHARS {
        Err(FieldErrors::one(
            field,
            FieldErrorCode::TooLong,
            Some(EMAIL_MAX_CHARS as u32),
        ))
    } else if !well_formed {
        Err(FieldErrors::one(field, FieldErrorCode::InvalidFormat, None))
    } else {
        Ok(email)
    }
}

/// The signed-in person. Account operations are theirs alone; the dispatcher keeps API keys
/// out before a handler runs.
fn signed_in(caller: &Caller) -> Result<UserId, ApiError> {
    caller
        .user()
        .cloned()
        .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))
}
