use ferrium::grpc::{
    kv::kv_service_client::KvServiceClient,
    kv::{GetRequest, SetRequest},
    management::management_service_client::ManagementServiceClient,
    management::{HealthRequest, InitializeRequest},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();

    let addr = "http://127.0.0.1:9001";

    // Test Management Service
    let mut management_client = ManagementServiceClient::connect(addr).await?;

    println!("Testing gRPC Management Service...");

    // Test health check
    let health_response = management_client
        .health_check(tonic::Request::new(HealthRequest {}))
        .await?;
    println!("Health: {:?}", health_response.get_ref());

    // Test initialization
    let init_response = management_client
        .initialize(tonic::Request::new(InitializeRequest {}))
        .await?;
    println!("Init: {:?}", init_response.get_ref());

    // Test KV Service
    let mut kv_client = KvServiceClient::connect(addr).await?;

    println!("Testing gRPC KV Service...");

    // Test set
    let set_response = kv_client
        .set(tonic::Request::new(SetRequest {
            key: "test_key".to_string(),
            value: "test_value".to_string(),
        }))
        .await?;
    println!("Set: {:?}", set_response.get_ref());

    // Test get
    let get_response = kv_client
        .get(tonic::Request::new(GetRequest {
            key: "test_key".to_string(),
        }))
        .await?;
    println!("Get: {:?}", get_response.get_ref());

    println!("gRPC tests completed successfully!");

    Ok(())
}
