use crate::config::SecurityConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

/// TLS-related errors
#[derive(Error, Debug)]
pub enum TlsError {
    #[error("Failed to read certificate file: {0}")]
    CertificateRead(#[from] std::io::Error),

    #[error("Failed to parse certificate: {0}")]
    CertificateParse(String),

    #[error("Failed to parse private key: {0}")]
    PrivateKeyParse(String),

    #[error("No valid private key found")]
    NoPrivateKey,

    #[error("TLS configuration error: {0}")]
    Configuration(String),

    #[error("Certificate validation failed: {0}")]
    Validation(String),
}

/// Certificate verifier that accepts all certificates (for testing)
#[derive(Debug)]
struct InsecureCertVerifier;

impl rustls::client::danger::ServerCertVerifier for InsecureCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Accept all certificates without verification
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        // Accept all signatures without verification
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        // Accept all signatures without verification
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        // Support all signature schemes
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}

/// TLS configuration builder for servers
pub struct TlsConfig {
    pub certificates: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
    pub client_ca_certs: Option<Vec<CertificateDer<'static>>>,
    pub require_client_cert: bool,
}

impl Clone for TlsConfig {
    fn clone(&self) -> Self {
        Self {
            certificates: self.certificates.clone(),
            private_key: self.private_key.clone_key(),
            client_ca_certs: self.client_ca_certs.clone(),
            require_client_cert: self.require_client_cert,
        }
    }
}

impl TlsConfig {
    /// Load TLS configuration from security config
    pub fn from_security_config(security: &SecurityConfig) -> Result<Option<Self>, TlsError> {
        if !security.enable_tls {
            return Ok(None);
        }

        let cert_file = security.cert_file.as_ref().ok_or_else(|| {
            TlsError::Configuration("cert_file is required when TLS is enabled".to_string())
        })?;

        let key_file = security.key_file.as_ref().ok_or_else(|| {
            TlsError::Configuration("key_file is required when TLS is enabled".to_string())
        })?;

        let certificates = load_certificates(cert_file)?;
        let private_key = load_private_key(key_file)?;

        let client_ca_certs = if security.enable_mtls {
            let ca_file = security.ca_file.as_ref().ok_or_else(|| {
                TlsError::Configuration("ca_file is required when mTLS is enabled".to_string())
            })?;
            Some(load_certificates(ca_file)?)
        } else {
            None
        };

        Ok(Some(TlsConfig {
            certificates,
            private_key,
            client_ca_certs,
            require_client_cert: security.enable_mtls,
        }))
    }

    /// Create rustls ServerConfig for HTTP server
    pub fn create_rustls_server_config(&self) -> Result<Arc<rustls::ServerConfig>, TlsError> {
        let config = rustls::ServerConfig::builder();

        // Configure client certificate verification if mTLS is enabled
        let config = if self.require_client_cert {
            if let Some(ref ca_certs) = self.client_ca_certs {
                let mut roots = rustls::RootCertStore::empty();
                for cert in ca_certs {
                    roots.add(cert.clone()).map_err(|e| {
                        TlsError::Configuration(format!("Failed to add CA certificate: {e}"))
                    })?;
                }

                config.with_client_cert_verifier(
                    rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                        .build()
                        .map_err(|e| {
                            TlsError::Configuration(format!("Failed to build client verifier: {e}"))
                        })?,
                )
            } else {
                return Err(TlsError::Configuration(
                    "mTLS enabled but no CA certificates provided".to_string(),
                ));
            }
        } else {
            config.with_no_client_auth()
        };

        let config = config
            .with_single_cert(self.certificates.clone(), self.private_key.clone_key())
            .map_err(|e| {
                TlsError::Configuration(format!("Failed to configure server certificate: {e}"))
            })?;

        Ok(Arc::new(config))
    }

    /// Create tonic ServerTlsConfig for gRPC server
    pub fn create_tonic_server_config(
        &self,
    ) -> Result<tonic::transport::ServerTlsConfig, TlsError> {
        let mut tls_config = tonic::transport::ServerTlsConfig::new();

        // Set server certificate and key
        if !self.certificates.is_empty() {
            // Convert DER to PEM for tonic (tonic expects PEM format)
            let cert_pem = self
                .certificates
                .iter()
                .map(|cert| der_to_pem(cert, "CERTIFICATE"))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            let key_pem = private_key_der_to_pem(&self.private_key)?;

            tls_config =
                tls_config.identity(tonic::transport::Identity::from_pem(cert_pem, key_pem));
        }

        // Configure client certificate verification for mTLS
        if self.require_client_cert {
            if let Some(ref ca_certs) = self.client_ca_certs {
                let ca_pem = ca_certs
                    .iter()
                    .map(|cert| der_to_pem(cert, "CERTIFICATE"))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n");

                tls_config =
                    tls_config.client_ca_root(tonic::transport::Certificate::from_pem(ca_pem));
            }
        }

        Ok(tls_config)
    }
}

/// TLS configuration for clients
pub struct ClientTlsConfig {
    pub ca_certs: Option<Vec<CertificateDer<'static>>>,
    pub client_cert: Option<Vec<CertificateDer<'static>>>,
    pub client_key: Option<PrivateKeyDer<'static>>,
    pub verify_hostname: bool,
    pub accept_invalid_certs: bool,
}

impl ClientTlsConfig {
    /// Create client TLS config from security config
    pub fn from_security_config(security: &SecurityConfig) -> Result<Option<Self>, TlsError> {
        if !security.enable_tls {
            return Ok(None);
        }

        let ca_certs = if let Some(ref ca_file) = security.ca_file {
            Some(load_certificates(ca_file)?)
        } else {
            None
        };

        let (client_cert, client_key) = if security.enable_mtls {
            let cert_file = security.cert_file.as_ref().ok_or_else(|| {
                TlsError::Configuration("cert_file is required for client mTLS".to_string())
            })?;
            let key_file = security.key_file.as_ref().ok_or_else(|| {
                TlsError::Configuration("key_file is required for client mTLS".to_string())
            })?;

            (
                Some(load_certificates(cert_file)?),
                Some(load_private_key(key_file)?),
            )
        } else {
            (None, None)
        };

        Ok(Some(ClientTlsConfig {
            ca_certs,
            client_cert,
            client_key,
            verify_hostname: !security.accept_invalid_certs, // Only verify hostname if we're validating certs
            accept_invalid_certs: security.accept_invalid_certs,
        }))
    }

    /// Create an insecure client TLS config that accepts invalid certificates (for testing)
    pub fn insecure() -> Self {
        ClientTlsConfig {
            ca_certs: None,
            client_cert: None,
            client_key: None,
            verify_hostname: false,
            accept_invalid_certs: true,
        }
    }

    /// Create rustls ClientConfig for HTTP clients
    pub fn create_rustls_client_config(&self) -> Result<Arc<rustls::ClientConfig>, TlsError> {
        // If accepting invalid certificates, use danger_accept_invalid_certs for simplicity
        if self.accept_invalid_certs {
            let config_builder = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(InsecureCertVerifier));

            // Still provide client certificates if available (needed for mTLS)
            let config = if let (Some(client_cert), Some(client_key)) =
                (&self.client_cert, &self.client_key)
            {
                tracing::info!("🔐 Using client certificates in permissive TLS mode for mTLS");
                config_builder
                    .with_client_auth_cert(client_cert.clone(), client_key.clone_key())
                    .map_err(|e| {
                        TlsError::Configuration(format!(
                            "Failed to configure client certificate in permissive mode: {e}"
                        ))
                    })?
            } else {
                tracing::info!("🔓 No client certificates available in permissive TLS mode");
                config_builder.with_no_client_auth()
            };
            return Ok(Arc::new(config));
        }

        let config = rustls::ClientConfig::builder();

        // Configure root certificates
        let config = if let Some(ref ca_certs) = self.ca_certs {
            let mut roots = rustls::RootCertStore::empty();
            for cert in ca_certs {
                roots.add(cert.clone()).map_err(|e| {
                    TlsError::Configuration(format!("Failed to add CA certificate: {e}"))
                })?;
            }
            config.with_root_certificates(roots)
        } else {
            // Load native root certificates
            let mut roots = rustls::RootCertStore::empty();
            let cert_result = rustls_native_certs::load_native_certs();

            // Add valid certificates, warn about any that failed
            for cert_der in cert_result.certs {
                if let Err(e) = roots.add(cert_der) {
                    tracing::warn!("Failed to add native certificate: {}", e);
                }
            }

            // Warn about any errors that occurred during loading
            for error in cert_result.errors {
                tracing::warn!("Error loading native certificate: {}", error);
            }

            config.with_root_certificates(roots)
        };

        // Configure client certificate if mTLS is enabled
        let config = if let (Some(client_cert), Some(client_key)) =
            (&self.client_cert, &self.client_key)
        {
            config
                .with_client_auth_cert(client_cert.clone(), client_key.clone_key())
                .map_err(|e| {
                    TlsError::Configuration(format!("Failed to configure client certificate: {e}"))
                })?
        } else {
            config.with_no_client_auth()
        };

        Ok(Arc::new(config))
    }

    /// Create tonic ClientTlsConfig for gRPC clients
    pub fn create_tonic_client_config(
        &self,
        domain: &str,
    ) -> Result<tonic::transport::ClientTlsConfig, TlsError> {
        let mut tls_config = tonic::transport::ClientTlsConfig::new();

        // Set domain for SNI
        if self.verify_hostname {
            tls_config = tls_config.domain_name(domain);
        }

        // Configure CA certificates
        if let Some(ref ca_certs) = self.ca_certs {
            let ca_pem = ca_certs
                .iter()
                .map(|cert| der_to_pem(cert, "CERTIFICATE"))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");

            tls_config = tls_config.ca_certificate(tonic::transport::Certificate::from_pem(ca_pem));
        }

        // Configure client certificate for mTLS
        if let (Some(client_cert), Some(client_key)) = (&self.client_cert, &self.client_key) {
            let cert_pem = client_cert
                .iter()
                .map(|cert| der_to_pem(cert, "CERTIFICATE"))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            let key_pem = private_key_der_to_pem(client_key)?;

            tls_config =
                tls_config.identity(tonic::transport::Identity::from_pem(cert_pem, key_pem));
        }

        Ok(tls_config)
    }
}

/// Load certificates from a PEM file
fn load_certificates<P: AsRef<Path>>(path: P) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let file = fs::File::open(&path)?;
    let mut reader = BufReader::new(file);

    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::CertificateParse(format!("Failed to parse certificates: {e}")))?;

    if certs.is_empty() {
        return Err(TlsError::CertificateParse(
            "No certificates found in file".to_string(),
        ));
    }

    Ok(certs)
}

/// Load private key from a PEM file
fn load_private_key<P: AsRef<Path>>(path: P) -> Result<PrivateKeyDer<'static>, TlsError> {
    let file = fs::File::open(&path)?;
    let mut reader = BufReader::new(file);

    // Try to read as PKCS8 first, then RSA, then EC
    let keys = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| TlsError::PrivateKeyParse(format!("Failed to parse private key: {e}")))?;

    keys.ok_or(TlsError::NoPrivateKey)
}

/// Convert DER certificate to PEM format
fn der_to_pem(der: &CertificateDer<'_>, label: &str) -> Result<String, TlsError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let encoded = STANDARD.encode(der.as_ref());
    let pem = format!(
        "-----BEGIN {}-----\n{}\n-----END {}-----",
        label,
        encoded
            .chars()
            .collect::<Vec<_>>()
            .chunks(64)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n"),
        label
    );
    Ok(pem)
}

/// Convert private key DER to PEM format
fn private_key_der_to_pem(key: &PrivateKeyDer<'_>) -> Result<String, TlsError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let (label, data) = match key {
        PrivateKeyDer::Pkcs1(_) => ("RSA PRIVATE KEY", key.secret_der()),
        PrivateKeyDer::Pkcs8(_) => ("PRIVATE KEY", key.secret_der()),
        PrivateKeyDer::Sec1(_) => ("EC PRIVATE KEY", key.secret_der()),
        _ => ("PRIVATE KEY", key.secret_der()),
    };

    let encoded = STANDARD.encode(data);
    let pem = format!(
        "-----BEGIN {}-----\n{}\n-----END {}-----",
        label,
        encoded
            .chars()
            .collect::<Vec<_>>()
            .chunks(64)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n"),
        label
    );
    Ok(pem)
}

/// Utility function to validate TLS configuration
pub fn validate_tls_config(security: &SecurityConfig) -> Result<(), TlsError> {
    if !security.enable_tls {
        return Ok(());
    }

    // Check required files exist
    if let Some(ref cert_file) = security.cert_file {
        if !cert_file.exists() {
            return Err(TlsError::Configuration(format!(
                "Certificate file does not exist: {}",
                cert_file.display()
            )));
        }
    } else {
        return Err(TlsError::Configuration(
            "cert_file is required when TLS is enabled".to_string(),
        ));
    }

    if let Some(ref key_file) = security.key_file {
        if !key_file.exists() {
            return Err(TlsError::Configuration(format!(
                "Private key file does not exist: {}",
                key_file.display()
            )));
        }
    } else {
        return Err(TlsError::Configuration(
            "key_file is required when TLS is enabled".to_string(),
        ));
    }

    if security.enable_mtls {
        if let Some(ref ca_file) = security.ca_file {
            if !ca_file.exists() {
                return Err(TlsError::Configuration(format!(
                    "CA certificate file does not exist: {}",
                    ca_file.display()
                )));
            }
        } else {
            return Err(TlsError::Configuration(
                "ca_file is required when mTLS is enabled".to_string(),
            ));
        }
    }

    Ok(())
}

/// Create a self-signed certificate for testing (DO NOT use in production)
/// Note: This function is currently disabled due to rcgen API compatibility issues
#[cfg(not(any()))] // Permanently disabled
#[allow(dead_code)]
pub fn generate_self_signed_cert(
    _subject_name: &str,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    // Disabled due to rcgen API changes - use external certificate generation instead
    Err("Test certificate generation is currently disabled. Use external tools to generate test certificates.".into())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::config::SecurityConfig;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    /// Test utilities for creating certificates and keys
    #[cfg(feature = "test-certs")]
    #[allow(clippy::field_reassign_with_default)]
    mod test_utils {
        use super::*;
        use rcgen::{generate_simple_self_signed, Certificate};
        use std::io::Write;

        #[allow(dead_code)]
        pub struct TestCertificateAuthority {
            pub cert: Certificate,
            pub cert_pem: String,
            pub key_pem: String,
        }

        #[allow(dead_code)]
        pub struct TestCertificate {
            pub cert: Certificate,
            pub cert_pem: String,
            pub key_pem: String,
            pub cert_der: Vec<u8>,
            pub key_der: Vec<u8>,
        }

        impl TestCertificateAuthority {
            pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
                let certified_key = generate_simple_self_signed(vec!["Test CA".to_string()])?;
                let cert_pem = certified_key.cert.pem();
                let key_pem = certified_key.signing_key.serialize_pem();

                Ok(Self {
                    cert: certified_key.cert,
                    cert_pem,
                    key_pem,
                })
            }

            pub fn issue_certificate(
                &self,
                _subject_name: &str,
                san_names: Vec<String>,
            ) -> Result<TestCertificate, Box<dyn std::error::Error>> {
                let certified_key = generate_simple_self_signed(san_names)?;
                let cert_pem = certified_key.cert.pem();
                let key_pem = certified_key.signing_key.serialize_pem();
                let cert_der = certified_key.cert.der().to_vec();
                let key_der = certified_key.signing_key.serialize_der();

                Ok(TestCertificate {
                    cert: certified_key.cert,
                    cert_pem,
                    key_pem,
                    cert_der,
                    key_der,
                })
            }
        }

        pub fn create_test_certificates() -> Result<
            (TestCertificateAuthority, TestCertificate, TestCertificate),
            Box<dyn std::error::Error>,
        > {
            let ca = TestCertificateAuthority::new()?;

            let server_cert = ca.issue_certificate(
                "test-server",
                vec![
                    "localhost".to_string(),
                    "127.0.0.1".to_string(),
                    "test-server".to_string(),
                ],
            )?;

            let client_cert =
                ca.issue_certificate("test-client", vec!["test-client".to_string()])?;

            Ok((ca, server_cert, client_cert))
        }

        pub fn write_temp_file(content: &str) -> Result<NamedTempFile, std::io::Error> {
            let mut temp_file = NamedTempFile::new()?;
            temp_file.write_all(content.as_bytes())?;
            temp_file.flush()?;
            Ok(temp_file)
        }
    }

    #[test]
    fn test_tls_config_validation() {
        let mut security = SecurityConfig::default();

        // Should pass when TLS is disabled
        assert!(validate_tls_config(&security).is_ok());

        // Should fail when TLS is enabled but no cert file
        security.enable_tls = true;
        assert!(validate_tls_config(&security).is_err());

        // Should fail when cert file is provided but no key file
        security.cert_file = Some(PathBuf::from("/tmp/cert.pem"));
        assert!(validate_tls_config(&security).is_err());

        // Should pass when files are provided (even if they don't exist for this test)
        security.cert_file = Some(PathBuf::from("/tmp/cert.pem"));
        security.key_file = Some(PathBuf::from("/tmp/key.pem"));

        // This will fail because files don't exist, but that's expected
        let result = validate_tls_config(&security);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_tls_config_validation_with_real_files() {
        let (ca, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&server_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());

        // Should pass with valid files
        assert!(validate_tls_config(&security).is_ok());

        // Test mTLS validation
        security.enable_mtls = true;
        security.ca_file = Some(ca_file.path().to_path_buf());
        assert!(validate_tls_config(&security).is_ok());

        // Should fail when mTLS is enabled but no CA file
        security.ca_file = None;
        assert!(validate_tls_config(&security).is_err());
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_certificate_loading() {
        let (_, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&server_cert.cert_pem).unwrap();
        let certificates = load_certificates(cert_file.path()).unwrap();

        assert!(!certificates.is_empty());
        assert_eq!(certificates.len(), 1);
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_private_key_loading() {
        let (_, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();
        let private_key = load_private_key(key_file.path()).unwrap();

        // Verify the key was loaded successfully
        assert!(!private_key.secret_der().is_empty());
    }

    #[test]
    fn test_invalid_certificate_file() {
        let invalid_cert = NamedTempFile::new().unwrap();
        std::fs::write(invalid_cert.path(), "not a certificate").unwrap();

        let result = load_certificates(invalid_cert.path());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TlsError::CertificateParse(_)));
    }

    #[test]
    fn test_invalid_private_key_file() {
        let invalid_key = NamedTempFile::new().unwrap();
        std::fs::write(invalid_key.path(), "not a private key").unwrap();

        let result = load_private_key(invalid_key.path());
        assert!(result.is_err());
        // Check that it's either a parse error or no private key error
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            TlsError::PrivateKeyParse(_) | TlsError::NoPrivateKey
        ));
    }

    #[test]
    fn test_empty_certificate_file() {
        let empty_cert = NamedTempFile::new().unwrap();
        std::fs::write(empty_cert.path(), "").unwrap();

        let result = load_certificates(empty_cert.path());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TlsError::CertificateParse(_)));
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_tls_config_creation() {
        let (ca, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&server_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());
        security.ca_file = Some(ca_file.path().to_path_buf());

        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();

        // Verify the TLS config was created successfully
        assert!(!tls_config.certificates.is_empty());
        assert!(!tls_config.private_key.secret_der().is_empty());
        // Note: Certificates loaded successfully (ca_certificates field was removed in newer versions)
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_client_tls_config_creation() {
        let (ca, _, client_cert) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&client_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&client_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.enable_mtls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());
        security.ca_file = Some(ca_file.path().to_path_buf());

        let client_config = ClientTlsConfig::from_security_config(&security)
            .unwrap()
            .unwrap();

        // Verify the client config was created successfully
        assert!(client_config.ca_certs.is_some());
        assert!(client_config.client_cert.is_some());
        assert!(client_config.client_key.is_some());
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_rustls_server_config_creation() {
        // Install default crypto provider for rustls
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .ok(); // Ignore error if already installed

        let (ca, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&server_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.enable_mtls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());
        security.ca_file = Some(ca_file.path().to_path_buf());

        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let rustls_config = tls_config.create_rustls_server_config().unwrap();

        // Verify the rustls config was created - we can't access private fields
        // but we can verify it doesn't panic and returns a valid Arc
        assert!(!std::ptr::eq(rustls_config.as_ref(), std::ptr::null()));
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_rustls_client_config_creation() {
        // Install default crypto provider for rustls
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .ok(); // Ignore error if already installed

        let (ca, _, client_cert) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&client_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&client_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.enable_mtls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());
        security.ca_file = Some(ca_file.path().to_path_buf());

        let client_config = ClientTlsConfig::from_security_config(&security)
            .unwrap()
            .unwrap();
        let rustls_config = client_config.create_rustls_client_config().unwrap();

        // Verify the client config was created
        // Verify client config is valid - we can't easily access root_store in newer versions
        assert!(!std::ptr::eq(rustls_config.as_ref(), std::ptr::null()));
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_tonic_server_config_creation() {
        let (ca, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&server_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.enable_mtls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());
        security.ca_file = Some(ca_file.path().to_path_buf());

        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let tonic_config = tls_config.create_tonic_server_config().unwrap();

        // Verify the tonic server config was created without panicking
        drop(tonic_config); // Just verify it was created successfully
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_tonic_client_config_creation() {
        let (ca, _, client_cert) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&client_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&client_cert.key_pem).unwrap();
        let ca_file = test_utils::write_temp_file(&ca.cert_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.enable_mtls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());
        security.ca_file = Some(ca_file.path().to_path_buf());

        let client_config = ClientTlsConfig::from_security_config(&security)
            .unwrap()
            .unwrap();
        let tonic_config = client_config
            .create_tonic_client_config("localhost")
            .unwrap();

        // Verify the tonic client config was created without panicking
        drop(tonic_config); // Just verify it was created successfully
    }

    #[test]
    fn test_der_to_pem_conversion() {
        let test_der = vec![0x30, 0x82, 0x01, 0x22]; // Dummy DER data
        let cert_der = CertificateDer::from(test_der);

        let pem = der_to_pem(&cert_der, "CERTIFICATE").unwrap();
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(pem.ends_with("-----END CERTIFICATE-----"));
        assert!(pem.contains("MIIBIg=="));
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_private_key_der_to_pem_conversion() {
        let (_, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();
        let private_key = load_private_key(key_file.path()).unwrap();

        let pem = private_key_der_to_pem(&private_key).unwrap();
        assert!(pem.starts_with("-----BEGIN "));
        assert!(pem.ends_with("-----")); // Fixed: PEM ends with "-----", not "-----END "
        assert!(pem.contains("PRIVATE KEY"));
        assert!(pem.contains("-----END "));
    }

    #[test]
    fn test_tls_error_display() {
        let error = TlsError::Configuration("test error".to_string());
        assert!(format!("{error}").contains("test error"));

        let error = TlsError::CertificateParse("parse error".to_string());
        assert!(format!("{error}").contains("parse error"));

        let error = TlsError::PrivateKeyParse("key error".to_string());
        assert!(format!("{error}").contains("key error"));

        let error = TlsError::NoPrivateKey;
        assert!(format!("{error}").contains("private key"));

        let error = TlsError::Configuration("I/O error: file not found".to_string());
        assert!(format!("{error}").contains("I/O error"));
    }

    #[cfg(feature = "test-certs")]
    #[test]
    fn test_tls_config_clone() {
        let (_, server_cert, _) = test_utils::create_test_certificates().unwrap();

        let cert_file = test_utils::write_temp_file(&server_cert.cert_pem).unwrap();
        let key_file = test_utils::write_temp_file(&server_cert.key_pem).unwrap();

        let mut security = SecurityConfig::default();
        security.enable_tls = true;
        security.cert_file = Some(cert_file.path().to_path_buf());
        security.key_file = Some(key_file.path().to_path_buf());

        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let cloned_config = tls_config.clone();

        // Verify the clone has the same data
        assert_eq!(
            tls_config.certificates.len(),
            cloned_config.certificates.len()
        );
        assert_eq!(
            tls_config.private_key.secret_der(),
            cloned_config.private_key.secret_der()
        );
    }
}
