//! `v1per init`: the instance admin, the secrets key and the state `serve` requires.

use v1per_server::db::Db;
use v1per_server::identity::{self, SECRETS_KEY_FILE};
use v1per_server::init::{self, InitArgs};

fn args(data: &std::path::Path) -> InitArgs {
    InitArgs {
        data_dir: data.to_path_buf(),
        admin_email: "Admin@Example.test".into(),
        admin_password: None,
        secrets_key: None,
        secrets_key_file: None,
    }
}

/// Init prints a generated password once; no known default works.
#[tokio::test]
async fn init_creates_an_instance_admin_without_a_known_default_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let data = dir.path().join("data");
    assert_eq!(
        init::check_initialized(&data).unwrap_err().code,
        "not_initialized"
    );
    let done = init::init(args(&data)).await.unwrap();
    let password = done.generated_password.unwrap();
    assert_eq!(password.chars().count(), 24);
    init::check_initialized(&data).unwrap();

    let again = init::init(args(&data)).await;
    assert!(matches!(again, Err(failure) if failure.code == "already_initialized"));

    let db = Db::open(&data).await.unwrap();
    let (hash, instance_admin): (String, bool) = sqlx::query_as(
        "SELECT password_hash, instance_admin FROM users WHERE email = 'admin@example.test'",
    )
    .fetch_one(&db.reader)
    .await
    .unwrap();
    assert!(instance_admin);
    assert!(identity::verify_password(hash.clone(), password).await);
    assert!(!identity::verify_password(hash, "admin".into()).await);
}

/// The key file is written readable by its owner only, and not at all when the key comes
/// from the environment.
#[tokio::test]
async fn init_writes_the_secrets_key_only_when_none_is_configured() {
    let dir = tempfile::TempDir::new().unwrap();
    let data = dir.path().join("data");
    init::init(args(&data)).await.unwrap();
    let key = std::fs::metadata(data.join(SECRETS_KEY_FILE)).unwrap();
    assert_eq!(key.len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(key.permissions().mode() & 0o777, 0o600);
    }

    let configured = dir.path().join("configured");
    init::init(InitArgs {
        secrets_key: Some("BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=".into()),
        ..args(&configured)
    })
    .await
    .unwrap();
    assert!(!configured.join(SECRETS_KEY_FILE).exists());
}
