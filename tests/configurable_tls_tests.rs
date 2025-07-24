use ferrium::config::{AuthMethod, SecurityConfig};
use ferrium::tls::{ClientTlsConfig, TlsConfig};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Test helper to create TLS certificates for testing
struct TlsCertificateTestSetup {
    _temp_dir: TempDir,
    ca_cert_path: PathBuf,
    server_cert_path: PathBuf,
    server_key_path: PathBuf,
}

impl TlsCertificateTestSetup {
    pub fn new() -> std::io::Result<Self> {
        // Check if OpenSSL is available - fail fast with clear error
        if std::process::Command::new("openssl")
            .arg("version")
            .output()
            .is_err()
        {
            panic!(
                "❌ OpenSSL is required for TLS tests but was not found. Please install OpenSSL."
            );
        }

        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path();

        // Generate CA certificate
        let ca_cert_path = temp_path.join("ca-cert.pem");
        let ca_key_path = temp_path.join("ca-key.pem");

        // Generate server certificate
        let server_cert_path = temp_path.join("server-cert.pem");
        let server_key_path = temp_path.join("server-key.pem");

        // Use OpenSSL commands to generate self-signed certificates for testing
        // Generate CA private key
        std::process::Command::new("openssl")
            .args(["genrsa", "-out", ca_key_path.to_str().unwrap(), "2048"])
            .output()
            .expect("Failed to generate CA private key");

        // Generate CA certificate with X.509 v3 extensions
        std::process::Command::new("openssl")
            .args([
                "req",
                "-new",
                "-x509",
                "-key",
                ca_key_path.to_str().unwrap(),
                "-sha256",
                "-subj",
                "/C=US/ST=CA/O=Ferrium Test/CN=Ferrium Test CA",
                "-days",
                "365",
                "-out",
                ca_cert_path.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to generate CA certificate");

        // Generate server private key
        std::process::Command::new("openssl")
            .args(["genrsa", "-out", server_key_path.to_str().unwrap(), "2048"])
            .output()
            .expect("Failed to generate server private key");

        // Generate server certificate signing request
        let server_csr_path = temp_path.join("server.csr");
        std::process::Command::new("openssl")
            .args([
                "req",
                "-new",
                "-key",
                server_key_path.to_str().unwrap(),
                "-subj",
                "/C=US/ST=CA/O=Ferrium Test/CN=localhost",
                "-out",
                server_csr_path.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to generate server certificate signing request");

        // Create a config file for X.509 v3 extensions
        let server_ext_path = temp_path.join("server.ext");
        std::fs::write(
            &server_ext_path,
            "authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, nonRepudiation, keyEncipherment, dataEncipherment
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
DNS.2 = *.localhost
IP.1 = 127.0.0.1
IP.2 = ::1
",
        )?;

        // Sign server certificate with CA using X.509 v3 extensions
        std::process::Command::new("openssl")
            .args([
                "x509",
                "-req",
                "-in",
                server_csr_path.to_str().unwrap(),
                "-CA",
                ca_cert_path.to_str().unwrap(),
                "-CAkey",
                ca_key_path.to_str().unwrap(),
                "-CAcreateserial",
                "-out",
                server_cert_path.to_str().unwrap(),
                "-days",
                "365",
                "-sha256",
                "-extfile",
                server_ext_path.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to sign server certificate");

        Ok(TlsCertificateTestSetup {
            _temp_dir: temp_dir,
            ca_cert_path,
            server_cert_path,
            server_key_path,
        })
    }

    pub fn create_permissive_security_config(&self) -> SecurityConfig {
        SecurityConfig {
            enable_tls: true,
            enable_mtls: false,
            cert_file: Some(self.server_cert_path.clone()),
            key_file: Some(self.server_key_path.clone()),
            ca_file: Some(self.ca_cert_path.clone()),
            accept_invalid_certs: true, // Permissive mode for self-signed certificates
            auth_method: AuthMethod::None,
        }
    }

    pub fn create_strict_security_config(&self) -> SecurityConfig {
        SecurityConfig {
            enable_tls: true,
            enable_mtls: false,
            cert_file: Some(self.server_cert_path.clone()),
            key_file: Some(self.server_key_path.clone()),
            ca_file: Some(self.ca_cert_path.clone()),
            accept_invalid_certs: false, // Strict mode for production
            auth_method: AuthMethod::None,
        }
    }

    pub fn create_mtls_permissive_security_config(&self) -> SecurityConfig {
        SecurityConfig {
            enable_tls: true,
            enable_mtls: true,
            cert_file: Some(self.server_cert_path.clone()),
            key_file: Some(self.server_key_path.clone()),
            ca_file: Some(self.ca_cert_path.clone()),
            accept_invalid_certs: true, // Permissive mode for self-signed certificates
            auth_method: AuthMethod::Certificate,
        }
    }
}

#[tokio::test]
async fn test_tls_client_config_permissive_mode() {
    println!("🧪 Testing TLS client config in permissive mode...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");
    let security_config = cert_setup.create_permissive_security_config();

    // Create client TLS config from security config
    let client_tls_config = ClientTlsConfig::from_security_config(&security_config)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    // Verify permissive settings
    assert!(
        client_tls_config.accept_invalid_certs,
        "Should accept invalid certificates in permissive mode"
    );
    assert!(
        !client_tls_config.verify_hostname,
        "Should not verify hostname in permissive mode"
    );

    println!("✅ Permissive mode client config created successfully");
}

#[tokio::test]
async fn test_tls_client_config_strict_mode() {
    println!("🧪 Testing TLS client config in strict mode...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");
    let security_config = cert_setup.create_strict_security_config();

    // Create client TLS config from security config
    let client_tls_config = ClientTlsConfig::from_security_config(&security_config)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    // Verify strict settings
    assert!(
        !client_tls_config.accept_invalid_certs,
        "Should NOT accept invalid certificates in strict mode"
    );
    assert!(
        client_tls_config.verify_hostname,
        "Should verify hostname in strict mode"
    );

    println!("✅ Strict mode client config created successfully");
}

#[tokio::test]
async fn test_tls_server_config_creation() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing TLS server config creation with different modes...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");

    // Test permissive mode
    let permissive_config = cert_setup.create_permissive_security_config();
    let server_tls_config = TlsConfig::from_security_config(&permissive_config)
        .expect("Failed to create server TLS config")
        .expect("Expected Some(TlsConfig)");

    // Should be able to create rustls server config
    let _rustls_config = server_tls_config
        .create_rustls_server_config()
        .expect("Failed to create rustls server config");

    // Test strict mode
    let strict_config = cert_setup.create_strict_security_config();
    let server_tls_config = TlsConfig::from_security_config(&strict_config)
        .expect("Failed to create server TLS config")
        .expect("Expected Some(TlsConfig)");

    // Should be able to create rustls server config
    let _rustls_config = server_tls_config
        .create_rustls_server_config()
        .expect("Failed to create rustls server config");

    println!("✅ Server configs created successfully for both modes");
}

#[tokio::test]
async fn test_mtls_permissive_mode() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing mTLS in permissive mode...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");
    let security_config = cert_setup.create_mtls_permissive_security_config();

    // Create server TLS config
    let server_tls_config = TlsConfig::from_security_config(&security_config)
        .expect("Failed to create server TLS config")
        .expect("Expected Some(TlsConfig)");

    // Verify mTLS is enabled
    assert!(
        server_tls_config.require_client_cert,
        "Should require client certificates in mTLS mode"
    );
    assert!(
        server_tls_config.client_ca_certs.is_some(),
        "Should have CA certs for client verification"
    );

    // Create client TLS config
    let client_tls_config = ClientTlsConfig::from_security_config(&security_config)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    // Verify permissive settings for mTLS
    assert!(
        client_tls_config.accept_invalid_certs,
        "Should accept invalid certificates in permissive mTLS mode"
    );
    assert!(
        !client_tls_config.verify_hostname,
        "Should not verify hostname in permissive mTLS mode"
    );
    assert!(
        client_tls_config.client_cert.is_some(),
        "Should have client certificate for mTLS"
    );
    assert!(
        client_tls_config.client_key.is_some(),
        "Should have client key for mTLS"
    );

    println!("✅ mTLS permissive mode config created successfully");
}

#[tokio::test]
async fn test_network_client_with_permissive_tls() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing network client creation with permissive TLS...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");
    let security_config = cert_setup.create_permissive_security_config();

    // Create client TLS config
    let client_tls_config = ClientTlsConfig::from_security_config(&security_config)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    let client_tls_config = Arc::new(client_tls_config);

    // Test that we can create HTTP clients with permissive TLS settings
    // This simulates what happens in the network layer for auto-join
    let client_builder = reqwest::Client::builder().timeout(Duration::from_secs(5));

    let client_builder = if client_tls_config.accept_invalid_certs {
        client_builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
    } else {
        // This would use proper certificate validation
        match client_tls_config.create_rustls_client_config() {
            Ok(rustls_config) => {
                let config = Arc::try_unwrap(rustls_config).unwrap_or_else(|arc| (*arc).clone());
                client_builder.use_preconfigured_tls(config)
            }
            Err(_) => {
                panic!("Failed to create proper TLS config in strict mode");
            }
        }
    };

    let _client = client_builder.build().expect("Failed to build HTTP client");

    println!("✅ Network client with permissive TLS created successfully");
}

#[tokio::test]
async fn test_configuration_validation() {
    println!("🧪 Testing configuration validation for different TLS modes...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");

    // Test permissive mode validation
    let permissive_config = cert_setup.create_permissive_security_config();
    assert!(
        ferrium::tls::validate_tls_config(&permissive_config).is_ok(),
        "Permissive TLS config should validate successfully"
    );

    // Test strict mode validation
    let strict_config = cert_setup.create_strict_security_config();
    assert!(
        ferrium::tls::validate_tls_config(&strict_config).is_ok(),
        "Strict TLS config should validate successfully"
    );

    // Test mTLS permissive mode validation
    let mtls_config = cert_setup.create_mtls_permissive_security_config();
    assert!(
        ferrium::tls::validate_tls_config(&mtls_config).is_ok(),
        "mTLS permissive config should validate successfully"
    );

    println!("✅ All TLS configurations validated successfully");
}

#[tokio::test]
async fn test_toml_serialization_with_accept_invalid_certs() {
    println!("🧪 Testing TOML serialization/deserialization with accept_invalid_certs field...");

    let cert_setup = TlsCertificateTestSetup::new().expect("Failed to setup certificates");

    // Test permissive mode
    let permissive_config = cert_setup.create_permissive_security_config();
    let toml_string = toml::to_string(&permissive_config).expect("Failed to serialize to TOML");

    println!("Permissive config TOML:\n{toml_string}");
    assert!(
        toml_string.contains("accept_invalid_certs = true"),
        "TOML should contain accept_invalid_certs = true"
    );

    let deserialized: SecurityConfig =
        toml::from_str(&toml_string).expect("Failed to deserialize from TOML");
    assert!(
        deserialized.accept_invalid_certs,
        "Deserialized config should have accept_invalid_certs = true"
    );

    // Test strict mode
    let strict_config = cert_setup.create_strict_security_config();
    let toml_string = toml::to_string(&strict_config).expect("Failed to serialize to TOML");

    println!("Strict config TOML:\n{toml_string}");
    assert!(
        toml_string.contains("accept_invalid_certs = false"),
        "TOML should contain accept_invalid_certs = false"
    );

    let deserialized: SecurityConfig =
        toml::from_str(&toml_string).expect("Failed to deserialize from TOML");
    assert!(
        !deserialized.accept_invalid_certs,
        "Deserialized config should have accept_invalid_certs = false"
    );

    println!("✅ TOML serialization/deserialization works correctly");
}
