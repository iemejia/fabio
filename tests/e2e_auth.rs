//! End-to-end integration tests for `fabio auth` commands.

mod common;

use common::{extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_status_returns_authenticated() {
    let assert = fabio().args(["auth", "status"]).assert().success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["status"], "authenticated");
    // Should report credential source
    let source = data["credential_source"].as_str().unwrap_or("");
    assert!(
        [
            "access_token",
            "environment",
            "managed_identity",
            "azure_cli",
            "azure_developer_cli"
        ]
        .contains(&source),
        "Unexpected credential_source: {source}"
    );
    // Message should mention the source
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Token acquired successfully via"),
        "Expected token message, got: {msg}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_status_json_envelope() {
    let assert = fabio()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"data\""));

    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_status_table_format() {
    // auth status with --output table should produce human-readable output
    fabio()
        .args(["auth", "status", "-o", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("authenticated"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_login_validates_credentials() {
    // Login now actually validates credentials (attempts token acquisition)
    let assert = fabio().args(["auth", "login"]).assert().success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "logged_in");
    assert_eq!(data["credential_source"], "azure_cli");
    assert_eq!(data["method"], "azure_cli");
    // Message should say successful
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("Azure CLI authentication validated"),
        "Expected success message, got: {msg}"
    );
}

#[test]
fn sp_login_bare_service_principal_flag_requires_params() {
    // --service-principal alone should fail with helpful error
    let assert = fabio()
        .args(["auth", "login", "--service-principal"])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--tenant is required"),
        "bare --service-principal should require --tenant, got: {stderr}"
    );
}

#[test]
fn device_code_requires_customer_public_client() {
    let assert = fabio()
        .env_remove("FABIO_CLIENT_ID")
        .args(["auth", "login", "--device-code"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("customer-owned public-client ID"),
        "{stderr}"
    );
    assert!(stderr.contains("FABIO_CLIENT_ID"), "{stderr}");
}

#[test]
fn browser_requires_customer_public_client_before_opening_browser() {
    let assert = fabio()
        .env_remove("FABIO_CLIENT_ID")
        .args(["auth", "login", "--browser"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("customer-owned public-client ID"),
        "{stderr}"
    );
}

#[test]
fn interactive_login_rejects_malformed_public_client() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--device-code",
            "--client-id",
            "not-a-uuid",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("must be a valid UUID"), "{stderr}");
}

#[test]
fn client_id_alone_requires_explicit_mode() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--client-id",
            "11111111-2222-3333-4444-555555555555",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("requires an explicit authentication mode"),
        "{stderr}"
    );
}

#[test]
fn auth_login_help_explains_azure_cli_default() {
    let assert = fabio().args(["auth", "login", "--help"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("Validate Azure CLI authentication"),
        "{stdout}"
    );
    assert!(stdout.contains("--device-code"), "{stdout}");
    assert!(
        stdout.contains("customer-managed public client"),
        "{stdout}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_logout_succeeds() {
    let assert = fabio().args(["auth", "logout"]).assert().success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "logged_out");
}

#[test]
fn auth_logout_removes_fabio_cache_and_legacy_marker() {
    let home = tempfile::tempdir().unwrap();
    let fabio_dir = home.path().join(".fabio");
    std::fs::create_dir_all(&fabio_dir).unwrap();
    let cache = fabio_dir.join("token_cache.json");
    let marker = fabio_dir.join(".logged_out");
    std::fs::write(&cache, "{}").unwrap();
    std::fs::write(&marker, "").unwrap();

    let assert = fabio()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .args(["auth", "logout"])
        .assert()
        .success();
    let data = parse_json(&assert);
    let data = extract_data(&data);
    assert_eq!(data["status"], "logged_out");
    assert!(!cache.exists());
    assert!(!marker.exists());
    assert!(data["message"].as_str().unwrap().contains("not signed out"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_status_with_query_extracts_field() {
    let assert = fabio()
        .args(["auth", "status", "--query", "status"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // --query status should extract just the status string
    assert_eq!(data, "authenticated");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn auth_status_reports_credential_source() {
    let assert = fabio()
        .args(["auth", "status", "--query", "credential_source"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should be one of the valid credential sources
    let source = data.as_str().unwrap_or("");
    assert!(
        [
            "access_token",
            "environment",
            "managed_identity",
            "azure_cli",
            "azure_developer_cli"
        ]
        .contains(&source),
        "Unexpected credential_source: {source}"
    );
}

// ── Service principal login validation tests (offline) ──────────────────────

#[test]
fn sp_login_requires_tenant() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--client-id",
            "abc",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--tenant is required"),
        "expected tenant required error, got: {stderr}"
    );
}

#[test]
fn sp_login_requires_client_id() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--client-id is required"),
        "expected client-id required error, got: {stderr}"
    );
}

#[test]
fn sp_login_requires_credential_type() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--client-secret, --certificate, or --federated-token"),
        "expected credential type required error, got: {stderr}"
    );
}

#[test]
fn sp_login_rejects_multiple_credential_types() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--client-secret",
            "secret",
            "--certificate",
            "/tmp/cert.pem",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Only one credential type allowed"),
        "expected mutual exclusion error, got: {stderr}"
    );
}

#[test]
#[cfg(windows)]
fn sp_login_certificate_file_not_found() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            "/nonexistent/path/cert.pem",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Failed to read certificate file"),
        "expected file not found error, got: {stderr}"
    );
}

#[test]
#[cfg(not(windows))]
fn sp_login_certificate_unsupported_on_non_windows() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            "/nonexistent/path/cert.pem",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("only supported on Windows"),
        "expected platform error, got: {stderr}"
    );
}

#[test]
fn sp_login_federated_token_file_not_found() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--federated-token-file",
            "/nonexistent/path/token.txt",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Failed to read federated token file"),
        "expected file not found error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_invalid_secret_returns_auth_error() {
    // Use fake tenant/client to verify the flow reaches Azure and fails gracefully
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--client-secret",
            "fake-secret-value",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "expected AUTH_REQUIRED error code, got: {stderr}"
    );
    assert!(
        stderr.contains("authentication failed")
            || stderr.contains("AADSTS")
            || stderr.contains("not found"),
        "expected Azure error details, got: {stderr}"
    );
}

// ── Empty string validation tests (offline) ─────────────────────────────────

#[test]
fn sp_login_empty_tenant_treated_as_missing() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "",
            "--client-id",
            "abc",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--tenant is required"),
        "empty --tenant should be treated as missing, got: {stderr}"
    );
}

#[test]
fn sp_login_empty_client_id_treated_as_missing() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--client-id is required"),
        "empty --client-id should be treated as missing, got: {stderr}"
    );
}

#[test]
fn sp_login_empty_secret_treated_as_no_credential() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--client-secret",
            "",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("requires one of"),
        "empty --client-secret should be treated as no credential, got: {stderr}"
    );
}

#[test]
fn sp_login_empty_certificate_path_treated_as_no_credential() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            "",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("requires one of"),
        "empty --certificate should be treated as no credential, got: {stderr}"
    );
}

#[test]
fn sp_login_empty_federated_token_treated_as_no_credential() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--federated-token",
            "",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("requires one of"),
        "empty --federated-token should be treated as no credential, got: {stderr}"
    );
}

// ── File content edge case tests (offline) ──────────────────────────────────

#[test]
#[cfg(windows)]
fn sp_login_empty_certificate_file() {
    // Create a temp empty file
    let temp = std::env::temp_dir().join("fabio_test_empty_cert.pem");
    std::fs::write(&temp, "").unwrap();

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            temp.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("is empty"),
        "empty certificate file should produce clear error, got: {stderr}"
    );

    std::fs::remove_file(&temp).ok();
}

#[test]
#[cfg(not(windows))]
fn sp_login_certificate_rejects_on_non_windows() {
    // Create a temp empty file
    let temp = std::env::temp_dir().join("fabio_test_empty_cert.pem");
    std::fs::write(&temp, "").unwrap();

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            temp.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("only supported on Windows"),
        "expected platform error, got: {stderr}"
    );

    std::fs::remove_file(&temp).ok();
}

#[test]
fn sp_login_empty_federated_token_file() {
    // Create a temp empty file
    let temp = std::env::temp_dir().join("fabio_test_empty_token.txt");
    std::fs::write(&temp, "   \n  ").unwrap(); // whitespace only

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--federated-token-file",
            temp.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("is empty"),
        "whitespace-only federated token file should be treated as empty, got: {stderr}"
    );

    std::fs::remove_file(&temp).ok();
}

#[test]
fn sp_login_invalid_certificate_content() {
    // Create a temp file with garbage content (not valid PEM/PFX)
    let temp = std::env::temp_dir().join("fabio_test_bad_cert.pem");
    std::fs::write(&temp, "this is not a certificate").unwrap();

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            temp.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED") || stderr.contains("certificate"),
        "invalid cert content should produce auth/cert error, got: {stderr}"
    );

    std::fs::remove_file(&temp).ok();
}

// ── Structured output validation (offline) ──────────────────────────────────

#[test]
fn sp_login_error_has_structured_json_envelope() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    // Should be valid JSON with error envelope
    let json: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");
    assert!(json.get("error").is_some(), "should have error field");
    assert_eq!(json["error"]["code"].as_str().unwrap(), "INVALID_INPUT");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("requires one of"),
        "message should explain the issue"
    );
}

#[test]
fn sp_login_error_includes_hint() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--client-id",
            "abc",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let json: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");
    assert!(
        json["error"]["hint"].as_str().is_some(),
        "error should include a hint with example command"
    );
    assert!(
        json["error"]["hint"].as_str().unwrap().contains("Example:"),
        "hint should contain an example"
    );
}

// ── Live tests with real Azure (invalid credentials) ────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_invalid_tenant_returns_descriptive_error() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "not-a-real-tenant-id",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--client-secret",
            "fake-secret",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "expected AUTH_REQUIRED code, got: {stderr}"
    );
    // Azure should report tenant not found
    assert!(
        stderr.contains("AADSTS") || stderr.contains("not found") || stderr.contains("Tenant"),
        "expected tenant-related error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_invalid_federated_token_returns_auth_error() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--federated-token",
            "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.invalid.token",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "expected AUTH_REQUIRED code, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_error_output_is_valid_json() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--client-secret",
            "bad",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let json: serde_json::Value =
        serde_json::from_str(&stderr).expect("live error should be valid JSON on stderr");
    assert_eq!(json["error"]["code"].as_str().unwrap(), "AUTH_REQUIRED");
    assert!(json["error"]["message"].as_str().unwrap().len() > 20);
    assert!(json["error"]["hint"].as_str().is_some());
}

// ── Additional edge case tests ──────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_custom_scope_reaches_azure() {
    // Custom scope still fails on invalid tenant, proving scope is passed through
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--client-secret",
            "fake",
            "--scope",
            "https://storage.azure.com/.default",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "custom scope should still reach Azure, got: {stderr}"
    );
}

#[test]
fn sp_login_both_federated_token_and_file_picks_inline() {
    // If both --federated-token and --federated-token-file are given,
    // it should count as one credential type (federated), not two
    // (this tests the has_federated logic: either OR both = 1 credential)
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--federated-token",
            "my-token",
            "--federated-token-file",
            "/some/path",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    // Should NOT say "only one credential type" since both are federated
    // Should fail at Azure auth (invalid tenant) or credential creation, NOT input validation
    assert!(
        !stderr.contains("Only one credential type"),
        "both federated flags should be treated as one credential type, got: {stderr}"
    );
}

// ── Certificate credential tests (PEM/PFX) ─────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_pem_certificate_reaches_azure() {
    // Generate a valid self-signed PEM cert (key+cert combined), then try to auth.
    // The cert is valid structurally but won't match any app registration.
    let cert_dir = std::env::temp_dir().join("fabio_test_pem_live");
    std::fs::create_dir_all(&cert_dir).unwrap();
    let key_path = cert_dir.join("key.pem");
    let cert_path = cert_dir.join("cert.pem");
    let combined_path = cert_dir.join("combined.pem");

    // Generate key and cert separately
    std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "1",
            "-nodes",
            "-subj",
            "/CN=fabio-e2e-test",
        ])
        .output()
        .expect("openssl must be available for cert tests");

    // Combine key + cert into single PEM file
    let key_content = std::fs::read_to_string(&key_path).unwrap();
    let cert_content = std::fs::read_to_string(&cert_path).unwrap();
    std::fs::write(&combined_path, format!("{key_content}{cert_content}")).unwrap();

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--certificate",
            combined_path.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "PEM cert should fail with AUTH_REQUIRED, got: {stderr}"
    );
    // Should mention certificate authentication or AADSTS tenant error
    assert!(
        stderr.contains("authentication failed")
            || stderr.contains("AADSTS")
            || stderr.contains("Tenant")
            || stderr.contains("certificate"),
        "expected cert/Azure error, got: {stderr}"
    );

    std::fs::remove_dir_all(&cert_dir).ok();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_pfx_certificate_with_password_reaches_azure() {
    // Generate a PFX cert with password, attempt auth
    let cert_dir = std::env::temp_dir().join("fabio_test_pfx");
    std::fs::create_dir_all(&cert_dir).unwrap();
    let key_path = cert_dir.join("key.pem");
    let cert_path = cert_dir.join("cert.pem");
    let pfx_path = cert_dir.join("test.pfx");

    // Generate key + cert
    std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "1",
            "-nodes",
            "-subj",
            "/CN=fabio-pfx-test",
        ])
        .output()
        .expect("openssl required");

    // Convert to PFX with password
    std::process::Command::new("openssl")
        .args([
            "pkcs12",
            "-export",
            "-out",
            pfx_path.to_str().unwrap(),
            "-inkey",
            key_path.to_str().unwrap(),
            "-in",
            cert_path.to_str().unwrap(),
            "-passout",
            "pass:e2eTestPw!",
        ])
        .output()
        .expect("openssl pkcs12 required");

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--certificate",
            pfx_path.to_str().unwrap(),
            "--certificate-password",
            "e2eTestPw!",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "PFX cert should reach Azure and fail on invalid tenant, got: {stderr}"
    );

    std::fs::remove_dir_all(&cert_dir).ok();
}

#[test]
fn sp_login_pfx_wrong_password_fails_locally() {
    // PFX with wrong password should fail at certificate parsing (not reach Azure)
    let cert_dir = std::env::temp_dir().join("fabio_test_pfx_badpw");
    std::fs::create_dir_all(&cert_dir).unwrap();
    let key_path = cert_dir.join("key.pem");
    let cert_path = cert_dir.join("cert.pem");
    let pfx_path = cert_dir.join("test.pfx");

    std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "1",
            "-nodes",
            "-subj",
            "/CN=fabio-badpw",
        ])
        .output()
        .expect("openssl required");

    std::process::Command::new("openssl")
        .args([
            "pkcs12",
            "-export",
            "-out",
            pfx_path.to_str().unwrap(),
            "-inkey",
            key_path.to_str().unwrap(),
            "-in",
            cert_path.to_str().unwrap(),
            "-passout",
            "pass:correctPassword",
        ])
        .output()
        .expect("openssl pkcs12 required");

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            pfx_path.to_str().unwrap(),
            "--certificate-password",
            "wrongPassword",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    // Should fail at certificate parsing, not at Azure
    assert!(
        stderr.contains("AUTH_REQUIRED") || stderr.contains("certificate"),
        "wrong PFX password should fail with cert error, got: {stderr}"
    );

    std::fs::remove_dir_all(&cert_dir).ok();
}

#[test]
fn sp_login_pem_without_private_key_fails() {
    // A PEM with only the certificate (no private key) should fail
    let cert_dir = std::env::temp_dir().join("fabio_test_pem_nokey");
    std::fs::create_dir_all(&cert_dir).unwrap();
    let key_path = cert_dir.join("key.pem");
    let cert_path = cert_dir.join("cert_only.pem");

    std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "1",
            "-nodes",
            "-subj",
            "/CN=fabio-nokey",
        ])
        .output()
        .expect("openssl required");

    // Use cert_only.pem (no key included)
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--certificate",
            cert_path.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED")
            || stderr.contains("certificate")
            || stderr.contains("key"),
        "cert without private key should fail, got: {stderr}"
    );

    std::fs::remove_dir_all(&cert_dir).ok();
}

// ── Federated token (OIDC) from file tests ──────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sp_login_federated_token_from_file_reaches_azure() {
    // Write a fake JWT to a file, then test that --federated-token-file reads it and reaches Azure
    let token_file = std::env::temp_dir().join("fabio_test_oidc_token.txt");
    std::fs::write(
        &token_file,
        "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.invalid_sig",
    )
    .unwrap();

    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "00000000-0000-0000-0000-000000000001",
            "--client-id",
            "00000000-0000-0000-0000-000000000002",
            "--federated-token-file",
            token_file.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "federated token from file should reach Azure, got: {stderr}"
    );
    // Confirm it's an Azure auth error (not a file read error)
    assert!(
        !stderr.contains("Failed to read"),
        "should not be a file read error since file exists, got: {stderr}"
    );

    std::fs::remove_file(&token_file).ok();
}

#[test]
fn sp_login_federated_token_file_with_newlines_trimmed() {
    // Token files often have trailing newlines — verify they're trimmed
    let token_file = std::env::temp_dir().join("fabio_test_token_newlines.txt");
    std::fs::write(&token_file, "  my-token-value  \n\n").unwrap();

    // This will fail at Azure (invalid tenant), but proves the token was read and trimmed
    // If it said "is empty" that would mean trimming removed everything
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--federated-token-file",
            token_file.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    // Should NOT say "is empty" since after trimming we have "my-token-value"
    assert!(
        !stderr.contains("is empty"),
        "token with whitespace should be trimmed, not treated as empty, got: {stderr}"
    );
    // Should fail at credential creation or Azure auth, not input validation
    assert!(
        stderr.contains("AUTH_REQUIRED"),
        "expected auth error after token read, got: {stderr}"
    );

    std::fs::remove_file(&token_file).ok();
}

// ── FABIO_ACCESS_TOKEN environment variable tests (offline) ─────────────────

#[test]
fn access_token_env_auth_status_reports_credential_source() {
    // FABIO_ACCESS_TOKEN is checked before all other credential sources.
    // auth status only calls require_auth() (no API call), so a fake token works.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-bearer-token")
        .args(["auth", "status", "-o", "json"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["status"], "authenticated");
    assert_eq!(
        data["credential_source"], "access_token",
        "should report access_token credential source"
    );
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("FABIO_ACCESS_TOKEN"),
        "message should mention FABIO_ACCESS_TOKEN, got: {msg}"
    );
}

#[test]
fn access_token_env_empty_reports_error() {
    // An empty FABIO_ACCESS_TOKEN should fail with a clear error, not silently
    // fall through to the next credential source.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "")
        .args(["auth", "status", "-o", "json"])
        .assert()
        .success(); // auth status always exits 0 (reports status in JSON)

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["status"], "not_authenticated");
    let msg = data["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("FABIO_ACCESS_TOKEN is set but empty"),
        "should report empty token error, got: {msg}"
    );
}

#[test]
fn access_token_env_takes_precedence_over_azure_sp() {
    // When FABIO_ACCESS_TOKEN is set alongside service principal env vars,
    // the static token should win (it's checked first in the credential chain).
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "my-static-token")
        .env("AZURE_TENANT_ID", "fake-tenant")
        .env("AZURE_CLIENT_ID", "fake-client")
        .env("AZURE_CLIENT_SECRET", "fake-secret")
        .args(["auth", "status", "-o", "json"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["credential_source"], "access_token");
}

#[test]
fn access_token_env_json_envelope_structure() {
    // Verify the JSON envelope is well-formed with expected fields.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "test-token-for-envelope")
        .args(["auth", "status", "-o", "json"])
        .assert()
        .success();

    let json = parse_json(&assert);
    assert!(json.get("data").is_some(), "should have data field");
    let data = extract_data(&json);
    assert!(data.get("status").is_some(), "data should have status");
    assert!(
        data.get("credential_source").is_some(),
        "data should have credential_source"
    );
    assert!(data.get("message").is_some(), "data should have message");
}

#[test]
fn access_token_env_table_format() {
    // Table output should mention the credential source.
    fabio()
        .env("FABIO_ACCESS_TOKEN", "test-token-table")
        .args(["auth", "status", "-o", "table"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("access_token")
                .or(predicate::str::contains("FABIO_ACCESS_TOKEN")),
        );
}

#[test]
fn access_token_env_query_extracts_credential_source() {
    // --query credential_source should extract just the string value.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "test-token-query")
        .args(["auth", "status", "--query", "credential_source"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data, "access_token");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn access_token_env_invalid_token_rejected_by_api() {
    // A fake token should authenticate locally (auth status succeeds) but fail
    // when actually calling the API (workspace list returns 401).
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "definitely-not-a-valid-jwt")
        .args(["workspace", "list", "--limit", "1", "-o", "json"])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("AUTH_REQUIRED") || stderr.contains("401"),
        "invalid token should be rejected by the API, got: {stderr}"
    );
}

#[test]
#[cfg(windows)]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dpapi_login_creates_encrypted_cache() {
    // After a successful auth status (which caches a token), the cache file
    // should contain encrypted (non-JSON) content on Windows
    let assert = fabio().args(["auth", "status"]).assert().success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "authenticated");

    // Check the cache file is NOT plaintext JSON
    let home = home::home_dir().expect("home dir");
    let cache_path = home.join(".fabio").join("token_cache.json");
    if cache_path.exists() {
        let raw = std::fs::read(&cache_path).unwrap();
        let as_str = String::from_utf8(raw);
        // If it's valid UTF-8 that parses as JSON, it's NOT encrypted
        let is_plaintext = as_str
            .as_ref()
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .is_some();
        assert!(
            !is_plaintext,
            "token cache should be DPAPI-encrypted on Windows, not plaintext JSON"
        );
    }
}

#[test]
#[cfg(windows)]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dpapi_encrypted_cache_is_readable_after_write() {
    // Auth status writes to cache, subsequent auth status reads it back
    fabio().args(["auth", "status"]).assert().success();

    // Second call should also succeed (reads the DPAPI-encrypted cache)
    let assert = fabio().args(["auth", "status"]).assert().success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "authenticated");
}

#[test]
#[cfg(windows)]
fn dpapi_logout_removes_encrypted_cache() {
    // After logout, the cache file should be deleted
    let home = home::home_dir().expect("home dir");
    let cache_path = home.join(".fabio").join("token_cache.json");

    // Create a dummy cache file
    std::fs::create_dir_all(cache_path.parent().unwrap()).ok();
    std::fs::write(&cache_path, b"dummy").ok();

    fabio().args(["auth", "logout"]).assert().success();

    assert!(
        !cache_path.exists(),
        "cache file should be deleted after logout"
    );
}

// ── WAM broker authentication tests ─────────────────────────────────────────

#[test]
fn wam_flag_accepted_by_parser() {
    // The --wam flag should be accepted (not rejected by clap)
    // On non-Windows it returns an error but does NOT crash
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--wam",
            "--client-id",
            "11111111-2222-3333-4444-555555555555",
        ])
        .assert();
    // On Linux/macOS: failure with INVALID_INPUT
    // On Windows: would attempt WAM (may succeed or fail depending on sign-in state)
    #[cfg(not(windows))]
    assert.failure();
    #[cfg(windows)]
    let _ = assert; // may succeed or fail
}

#[test]
#[cfg(not(windows))]
fn wam_rejected_on_non_windows() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--wam",
            "--client-id",
            "11111111-2222-3333-4444-555555555555",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("only supported on Windows"),
        "expected Windows-only error, got: {stderr}"
    );
    assert!(
        stderr.contains("INVALID_INPUT"),
        "expected INVALID_INPUT code, got: {stderr}"
    );
}

#[test]
#[cfg(not(windows))]
fn wam_error_suggests_alternatives() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--wam",
            "--client-id",
            "11111111-2222-3333-4444-555555555555",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Azure CLI") || stderr.contains("--device-code"),
        "error hint should suggest alternatives, got: {stderr}"
    );
}

#[test]
#[cfg(not(windows))]
fn wam_conflicts_with_service_principal() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--wam",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "mixed auth modes should be rejected by clap, got: {stderr}"
    );
}

// ── Windows-only WAM E2E tests ──────────────────────────────────────────────

#[test]
#[cfg(windows)]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn wam_login_produces_structured_output() {
    let client_id = std::env::var("FABIO_TEST_PUBLIC_CLIENT_ID")
        .expect("FABIO_TEST_PUBLIC_CLIENT_ID is required for WAM live tests");
    let assert = fabio()
        .args(["auth", "login", "--wam", "--client-id", &client_id])
        .assert();

    // WAM may succeed (if user is signed in) or fail (if not)
    let output = &assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let json: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be JSON");
        let data = &json["data"];
        assert_eq!(data["status"], "logged_in");
        assert_eq!(data["credential_source"], "wam_broker");
        assert!(data["expires_in_seconds"].is_number());
    } else {
        // Failure should still be structured JSON on stderr
        let json: serde_json::Value =
            serde_json::from_str(&stderr).expect("stderr should be JSON on failure");
        assert_eq!(json["error"]["code"], "AUTH_REQUIRED");
    }
}

#[test]
#[cfg(windows)]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn wam_login_with_tenant_override() {
    let client_id = std::env::var("FABIO_TEST_PUBLIC_CLIENT_ID")
        .expect("FABIO_TEST_PUBLIC_CLIENT_ID is required for WAM live tests");
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--wam",
            "--client-id",
            &client_id,
            "--tenant",
            "organizations",
        ])
        .assert();

    // Just verify it doesn't crash — result depends on Windows sign-in state
    let output = &assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Output should be parseable JSON (either success or error)
    if output.status.success() {
        serde_json::from_str::<serde_json::Value>(&stdout).expect("stdout should be JSON");
    } else {
        serde_json::from_str::<serde_json::Value>(&stderr).expect("stderr should be JSON");
    }
}

// ── Browser-based PKCE authentication tests ─────────────────────────────────

#[test]
fn browser_flag_accepted_by_parser() {
    // --browser should be accepted (the command starts but will timeout waiting for redirect)
    // We can't complete the flow in tests, but we can verify the flag is recognized
    fabio()
        .args(["auth", "login", "--browser", "--help"])
        .assert()
        .success();
}

#[test]
fn browser_flag_visible_in_help() {
    let assert = fabio().args(["auth", "login", "--help"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("--browser"),
        "expected --browser in help output"
    );
    assert!(
        stdout.contains("PKCE") || stdout.contains("browser"),
        "expected PKCE/browser description in help"
    );
    assert!(stdout.contains("customer-managed public client"));
}

#[test]
fn browser_conflicts_with_service_principal() {
    let assert = fabio()
        .args([
            "auth",
            "login",
            "--service-principal",
            "--browser",
            "--tenant",
            "abc",
            "--client-id",
            "def",
            "--client-secret",
            "xyz",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "mixed auth modes should be rejected by clap, got: {stderr}"
    );
}
