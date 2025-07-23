# Real Cluster Integration Testing

This document describes the comprehensive real cluster integration test that starts actual Ferrium server processes and uses the FerriumClient to perform read/write operations.

## 🎯 **Overview**

The `real_cluster_integration_test.rs` provides end-to-end testing of Ferrium clusters by:
1. **Starting real ferrium-server processes** (not mocks)
2. **Using the actual FerriumClient** to connect and operate
3. **Testing complete cluster functionality** including leader election
4. **Validating TLS/mTLS communication** in real environments

## 🔧 **Test Architecture**

### **RealClusterTestEnvironment**

The test uses a comprehensive environment manager that:
- **Generates TOML configuration files** for each node
- **Creates TLS certificates** when needed (using OpenSSL)
- **Starts actual ferrium-server processes**
- **Manages process lifecycle** and cleanup
- **Provides client connection addresses**

### **Test Categories**

1. **`test_ferrium_client_real_cluster_operations()`**
   - Starts a 3-node cluster with plain HTTP
   - Uses FerriumClient for read/write/delete operations
   - Tests concurrent operations and batch processing

2. **`test_ferrium_client_real_cluster_with_tls()`**  
   - Starts a 3-node cluster with TLS encryption
   - Tests encrypted communication through FerriumClient
   - Validates TLS certificate handling

3. **`test_ferrium_client_cluster_stress()`**
   - Performs stress testing with 50 concurrent operations
   - Measures performance under load
   - Validates cluster stability

## 🚀 **Test Flow**

### **Cluster Setup**
```rust
// Create test environment
let mut cluster_env = RealClusterTestEnvironment::new(
    vec![1, 2, 3],  // 3 nodes
    false           // TLS disabled
)?;

// Start actual ferrium-server processes
cluster_env.start_cluster()?;

// Initialize cluster (auto-join enabled)
cluster_env.initialize_cluster().await?;

// Wait for cluster ready state
cluster_env.wait_for_cluster_ready(30).await;
```

### **Client Operations**
```rust
// Create FerriumClient with cluster addresses
let mut client = FerriumClient::new(cluster_env.get_client_addresses());

// Wait for connection
client.wait_for_ready(Duration::from_secs(15)).await?;

// Perform operations
client.set("key".to_string(), "value".to_string()).await?;
let value = client.get("key".to_string()).await?;
client.delete("key".to_string()).await?;
```

## 🔐 **TLS Integration**

### **Certificate Generation**
The test automatically generates:
- **CA certificate and key** for the test environment
- **Node-specific certificates** for each cluster member
- **Proper certificate chains** for validation

### **TLS Configuration**
```rust
let mut cluster_env = RealClusterTestEnvironment::new(
    vec![1, 2, 3],  // 3 nodes  
    true            // TLS enabled
)?;
```

When TLS is enabled:
- All HTTP communication uses HTTPS
- Certificates are automatically configured
- Client connections use TLS encryption

## 📊 **Test Validation**

### **Cluster Health Checks**
- **Leader election validation**
- **Auto-join functionality** 
- **Process health monitoring**
- **Connection stability**

### **Data Consistency**
- **Write operation success**
- **Read-after-write consistency**
- **Delete operation verification**
- **Cross-node data replication**

### **Performance Metrics**
- **Operation latency measurement**
- **Concurrent operation handling**
- **Stress test completion times**
- **Resource cleanup verification**

## 🛠️ **Configuration Details**

### **Node Configuration**
Each node gets a unique configuration:
```toml
[node]
id = 1
http_addr = "127.0.0.1:20011"
grpc_addr = "127.0.0.1:30011" 
data_dir = "/tmp/test-cluster/data/node1"

[cluster]
name = "test-cluster"
expected_size = 3
enable_auto_join = true

[[cluster.peers]]
http_addr = "127.0.0.1:20011"
grpc_addr = "127.0.0.1:30011"
# ... additional peers

[security]
enable_tls = false  # or true for TLS tests
enable_mtls = false
```

### **TLS Configuration (when enabled)**
```toml
[security]
enable_tls = true
cert_file = "/tmp/test-cluster/certs/node1-cert.pem"
key_file = "/tmp/test-cluster/certs/node1-key.pem"
ca_file = "/tmp/test-cluster/certs/ca-cert.pem"  # for mTLS
```

## 🔧 **Process Management**

### **Startup Sequence**
1. **Build ferrium-server binary** (if not exists)
2. **Generate configurations** for all nodes
3. **Create TLS certificates** (if TLS enabled)
4. **Start ferrium-server processes** with unique configs
5. **Initialize cluster** on node 1
6. **Wait for auto-join** completion

### **Cleanup Process**
```rust
impl Drop for RealClusterTestEnvironment {
    fn drop(&mut self) {
        // Kill all ferrium-server processes
        for mut process in self.node_processes.drain(..) {
            let _ = process.kill();
            let _ = process.wait();
        }
        
        // Clean up temporary files
        // TempDir automatically cleans up directories
    }
}
```

## 📋 **Test Data Scenarios**

### **Basic Operations**
```rust
let test_data = vec![
    ("user:1", "Alice"),
    ("user:2", "Bob"), 
    ("user:3", "Charlie"),
    ("config:timeout", "30"),
    ("config:retries", "3"),
];
```

### **TLS-Specific Data**
```rust
let tls_test_data = vec![
    ("tls:secret1", "encrypted_value_1"),
    ("tls:secret2", "encrypted_value_2"),
    ("tls:config", "tls_enabled"),
];
```

### **Stress Test Data**
```rust
// 50 concurrent operations
for i in 0..50 {
    let key = format!("stress:key:{}", i);
    let value = format!("stress_value_{}", i);
    // ... perform operations
}
```

## 🎯 **Success Criteria**

### **Cluster Formation**
- ✅ All 3 nodes start successfully
- ✅ Leader election completes  
- ✅ Auto-join functionality works
- ✅ Cluster reaches ready state

### **Client Operations**
- ✅ FerriumClient connects successfully
- ✅ All write operations succeed
- ✅ All read operations return correct values
- ✅ Delete operations work correctly
- ✅ Batch operations complete successfully

### **TLS Functionality** 
- ✅ TLS certificates generate successfully
- ✅ HTTPS connections establish
- ✅ Encrypted data transmission works
- ✅ TLS handshakes complete

### **Performance Benchmarks**
- ✅ 50 operations complete within timeout
- ✅ No memory leaks or resource issues
- ✅ Process cleanup works correctly

## 🚨 **Error Handling**

### **Common Failure Modes**
- **Binary build failures**: Clear error messages
- **Port conflicts**: Automatic port selection
- **Certificate generation errors**: OpenSSL validation
- **Process startup failures**: Health check timeouts
- **Client connection issues**: Retry mechanisms

### **Debugging Support**
- **Detailed logging** throughout test execution
- **Process output capture** for diagnostics
- **Temporary file preservation** on failure
- **Clear assertion messages** for failures

## 🔄 **CI/CD Integration**

### **GitHub Workflows**
The test is integrated into:
- **CI Workflow** (`ci.yml`): Runs on every PR/push
- **Integration Workflow** (`integration.yml`): Comprehensive testing

### **Test Execution**
```yaml
- name: Run real cluster integration tests
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  run: cargo test --test real_cluster_integration_test --verbose
```

### **Dependencies**
- **System**: OpenSSL for certificate generation
- **Binary**: ferrium-server built with `cargo build --release`
- **Ports**: Automated selection to avoid conflicts

## 🎉 **Benefits**

### **End-to-End Validation**
- Tests the **complete system** not just components
- Uses **real processes** not mocks or simulations
- Validates **actual network communication**
- Tests **real file I/O and persistence**

### **Client Integration Testing**
- Proves **FerriumClient works** with real clusters
- Tests **connection management** and retry logic
- Validates **operation serialization** and networking
- Ensures **API compatibility** between client and server

### **Real-World Scenarios**
- **Process lifecycle management**
- **Configuration file handling**
- **Certificate generation and usage**
- **Port management and networking**
- **Resource cleanup and safety**

## 🎊 **Production Confidence**

This comprehensive test provides confidence that:
- **Ferrium clusters work** in real environments
- **FerriumClient reliably connects** and operates
- **TLS encryption functions** correctly end-to-end
- **Configuration management** handles all scenarios
- **Process management** is robust and safe

The test bridges the gap between unit tests and production deployment, ensuring the entire system works together seamlessly! 🚀 