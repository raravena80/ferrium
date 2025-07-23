# TLS-Enabled Cluster Testing with test-cluster.sh

The enhanced `scripts/test-cluster.sh` script now supports TLS and mTLS configurations for comprehensive security testing of your Ferrium clusters.

## 🔐 New TLS Features

### Command Line Options

- `--tls`: Enable TLS encryption for all cluster communication
- `--mtls`: Enable mutual TLS (mTLS) for mutual authentication + encryption

### Automatic Certificate Generation

The script automatically generates:
- **Certificate Authority (CA)** for the test environment
- **Node-specific certificates** signed by the test CA
- **Client certificates** for mTLS authentication
- **Subject Alternative Names (SANs)** for localhost testing

## 🚀 Usage Examples

### 1. Standard Cluster Test (Plain HTTP)
```bash
./scripts/test-cluster.sh --ci
```

### 2. TLS-Encrypted Cluster Test
```bash
# Test with TLS encryption
./scripts/test-cluster.sh --tls --ci

# Keep TLS cluster running for manual testing
./scripts/test-cluster.sh --tls --keep-running
```

### 3. mTLS Mutual Authentication Test
```bash
# Test with mutual TLS authentication  
./scripts/test-cluster.sh --mtls --ci

# Keep mTLS cluster running with 30-second auto-cleanup
./scripts/test-cluster.sh --mtls --auto-cleanup=30
```

## 🔍 What Gets Tested

### TLS Mode (`--tls`)
- ✅ **Certificate Generation**: OpenSSL-generated CA and node certificates
- ✅ **HTTPS Endpoints**: All HTTP APIs secured with TLS
- ✅ **Encrypted Communication**: KV operations over HTTPS
- ✅ **Certificate Validation**: Real certificate verification
- ✅ **Cluster Formation**: TLS-secured auto-join and leader election
- ✅ **Performance Testing**: TLS overhead measurement

### mTLS Mode (`--mtls`)
All TLS features plus:
- ✅ **Mutual Authentication**: Client certificate verification
- ✅ **Node Identity Verification**: Each node has unique certificates
- ✅ **Access Control**: Only nodes with valid certificates can join
- ✅ **End-to-End Security**: Full bidirectional authentication

## 📁 Generated Files

When TLS is enabled, certificates are automatically created in:

```
./test-cluster-automated/certs/
├── ca-cert.pem          # Root Certificate Authority
├── ca-key.pem           # CA Private Key
├── node1-cert.pem       # Node 1 Certificate
├── node1-key.pem        # Node 1 Private Key
├── node2-cert.pem       # Node 2 Certificate  
├── node2-key.pem        # Node 2 Private Key
├── node3-cert.pem       # Node 3 Certificate
└── node3-key.pem        # Node 3 Private Key
```

## 🛠️ Manual Testing Examples

After starting a TLS cluster with `--keep-running`:

### TLS Testing Commands
```bash
# Health check over HTTPS
curl -k https://127.0.0.1:21001/health

# Write data over TLS
curl -k -X POST -H 'Content-Type: application/json' \
  -d '{"Set":{"key":"tls-test","value":"encrypted!"}}' \
  https://127.0.0.1:21001/write

# Read data over TLS
curl -k -X POST -H 'Content-Type: application/json' \
  -d '{"key":"tls-test"}' \
  https://127.0.0.1:21001/read

# Cluster metrics over HTTPS
curl -k https://127.0.0.1:21001/metrics | jq
```

### Certificate Inspection
```bash
# View certificate details
openssl x509 -in ./test-cluster-automated/certs/node1-cert.pem -text -noout

# Test TLS connection
echo | openssl s_client -connect 127.0.0.1:21001 -servername localhost

# Verify certificate chain
openssl verify -CAfile ./test-cluster-automated/certs/ca-cert.pem \
  ./test-cluster-automated/certs/node1-cert.pem
```

## 🔧 Dependencies

The script automatically checks for required tools:

- **OpenSSL**: For certificate generation (required for `--tls` and `--mtls`)
- **curl**: For HTTP/HTTPS requests
- **jq**: For JSON parsing
- **nc**: For port connectivity testing

### Installing Dependencies

```bash
# macOS
brew install openssl curl jq netcat

# Ubuntu/Debian
sudo apt-get install openssl curl jq netcat-openbsd

# RHEL/CentOS
sudo yum install openssl curl jq nc
```

## 🎯 Integration with CI/CD

The enhanced script maintains full CI/CD compatibility:

```bash
# CI pipeline with TLS testing
./scripts/test-cluster.sh --tls --ci

# CI pipeline with mTLS testing  
./scripts/test-cluster.sh --mtls --ci

# Auto-cleanup after extended testing
./scripts/test-cluster.sh --mtls --auto-cleanup=60
```

## 🚨 Security Notes

### For Testing Only
- Uses self-signed certificates for testing
- Includes `-k` flag to skip certificate verification
- Not suitable for production use

### Production Recommendations
- Use proper CA-signed certificates
- Implement certificate rotation
- Use hardware security modules (HSMs) for key storage
- Enable certificate revocation checking

## 🎊 Success Output

When TLS testing completes successfully, you'll see:

```
🔐 FERRIUM TLS CLUSTER TEST
=================================
🔧 Mode: CI/Non-Interactive (immediate cleanup after tests)
🔐 Security: TLS Enabled (encryption only)

🔐 Setting up TLS certificates...
INFO: Generating Certificate Authority (CA)...
SUCCESS: CA certificate generated
INFO: Generating certificate for Node 1...
SUCCESS: Certificate generated for Node 1
[... certificate generation continues ...]
SUCCESS: 🔐 TLS certificates ready - encryption enabled

[... test execution ...]

🔐 Testing TLS connectivity...
INFO: Testing TLS connection to Node 1...
SUCCESS:   ✅ TLS health check passed
[... connectivity tests continue ...]
SUCCESS: 🔐 TLS connectivity verified - encryption working

[... all tests pass ...]

🎉 ALL TESTS PASSED SUCCESSFULLY!
=================================
```

## 🤝 Contributing

The TLS enhancements maintain backward compatibility while adding powerful security testing capabilities. All existing functionality works unchanged, with TLS features activated only when explicitly requested. 