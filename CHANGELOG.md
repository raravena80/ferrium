# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial Ferrite distributed key-value storage implementation
- Raft consensus protocol using openraft library
- RocksDB persistent storage backend
- HTTP REST API for cluster management and data operations
- gRPC API for high-performance inter-service communication
- TOML-based configuration system with auto-join support
- Docker containerization with multi-stage builds
- Comprehensive CI/CD pipeline with GitHub Actions
- Cross-platform binary releases (Linux, macOS, Windows)
- GitHub Container Registry publishing
- Unit and integration testing
- Security auditing and dependency management
- Code formatting and linting enforcement
- Memory safety testing with Miri
- Auto-join functionality for dynamic cluster formation
- Linearizable read operations
- TLS/mTLS support preparation (configuration ready)

### Dependencies
- Updated to Rust 1.82+ compatibility
- Updated major dependencies:
  - actix-cors: 0.6 → 0.7
  - toml: 0.8 → 0.9
  - rocksdb: 0.21 → 0.23
  - thiserror: 1.0 → 2.0
  - tonic: 0.10 → 0.13
  - reqwest: 0.11 → 0.12 (with rustls-tls)
  - dirs: 5.0 → 6.0
  - tower: 0.4 → 0.5

### Security
- Added dependency vulnerability scanning
- Enabled memory safety checks
- Switched to rustls for TLS instead of OpenSSL
- Implemented proper container security practices

## [0.1.0] - TBD

### Added
- Initial release of Ferrite distributed key-value storage system
- Basic Raft consensus implementation
- HTTP and gRPC API support
- Docker containerization
- Comprehensive testing and CI/CD pipeline 