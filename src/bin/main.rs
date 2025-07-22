use std::path::PathBuf;
use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{App, HttpServer, Result, middleware::Logger, web};
use clap::Parser;
use openraft::Raft;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

// gRPC imports
use ferrite::grpc::{
    KvServiceServer, ManagementServiceServer, RaftServiceServer,
    services::{KvServiceImpl, ManagementServiceImpl, RaftServiceImpl},
};
use tonic::transport::Server;

use ferrite::{
    config::{ConfigError, FerriteConfig, NodeId, create_raft_config},
    network::{HttpNetworkFactory, api, management::ManagementApi},
    storage::new_storage,
};

#[derive(Parser)]
#[command(name = "ferrite-server")]
#[command(about = "A distributed KV storage server using Raft consensus")]
#[command(version)]
pub struct Args {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Node ID (overrides config file)
    #[arg(long)]
    pub id: Option<NodeId>,

    /// HTTP API bind address (overrides config file)
    #[arg(long)]
    pub http_addr: Option<String>,

    /// gRPC API bind address (overrides config file)
    #[arg(long)]
    pub grpc_addr: Option<String>,

    /// Data directory (overrides config file)
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Log level (overrides config file)
    #[arg(long)]
    pub log_level: Option<String>,

    /// Generate default configuration file and exit
    #[arg(long)]
    pub generate_config: Option<PathBuf>,

    /// Validate configuration file and exit
    #[arg(long)]
    pub validate_config: bool,

    /// List default configuration file locations
    #[arg(long)]
    pub list_config_paths: bool,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    // Handle utility commands first
    if let Some(config_path) = args.generate_config {
        return generate_default_config(config_path);
    }

    if args.list_config_paths {
        return list_config_paths();
    }

    // Load configuration
    let config = load_configuration(&args).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Configuration error: {}", e),
        )
    })?;

    if args.validate_config {
        println!("✅ Configuration is valid");
        println!("📍 Node ID: {}", config.node.id);
        println!("🌐 HTTP Address: {}", config.node.http_addr);
        println!("🔌 gRPC Address: {}", config.node.grpc_addr);
        println!("💾 Data Directory: {}", config.node.data_dir.display());
        println!("📊 Log Level: {}", config.logging.level);
        return Ok(());
    }

    // Initialize logging based on configuration
    setup_logging(&config)?;

    tracing::info!("🚀 Starting Ferrite node {}", config.node.id);
    tracing::info!(
        "📁 Configuration loaded from: {}",
        args.config
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "defaults".to_string())
    );
    tracing::info!("🌐 HTTP API: http://{}", config.node.http_addr);
    tracing::info!("🔌 gRPC API: http://{}", config.node.grpc_addr);
    tracing::info!("💾 Data directory: {}", config.node.data_dir.display());
    tracing::info!("🏷️  Cluster: {}", config.cluster.name);

    // Log peer information
    let all_peers = config.cluster.get_all_peers();
    if !all_peers.is_empty() {
        tracing::info!("👥 Known peers:");
        for (id, peer) in &all_peers {
            tracing::info!(
                "   Node {}: HTTP={}, gRPC={}, Voting={}",
                id,
                peer.http_addr,
                peer.grpc_addr,
                peer.voting
            );
        }
    }

    // Initialize storage with configuration
    let (log_store, state_machine_store) =
        new_storage(&config.node.data_dir).await.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Storage error: {}", e))
        })?;

    // Initialize network
    let network_factory = HttpNetworkFactory::new();

    // Create Raft instance with configuration
    let raft_config = Arc::new(create_raft_config(&config.raft));
    let raft = Arc::new(
        Raft::new(
            config.node.id,
            raft_config,
            network_factory,
            log_store,
            state_machine_store,
        )
        .await
        .map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Raft error: {}", e))
        })?,
    );

    // Create management API
    let management = Arc::new(ManagementApi::new(
        (*raft).clone(),
        config.node.id,
        config.clone(),
    ));

    // Create gRPC services
    let kv_service = KvServiceImpl::new(management.clone());
    let management_service = ManagementServiceImpl::new(management.clone());
    let raft_service = RaftServiceImpl::new(raft.clone());

    // Start gRPC server in a separate task
    let grpc_addr = config.node.grpc_addr;
    let grpc_server = tokio::spawn(async move {
        tracing::info!("🔌 Starting gRPC server on {}", grpc_addr);

        Server::builder()
            .add_service(KvServiceServer::new(kv_service))
            .add_service(ManagementServiceServer::new(management_service))
            .add_service(RaftServiceServer::new(raft_service))
            .serve(grpc_addr)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });

    // Start auto-join process in background if enabled
    if config.cluster.enable_auto_join && !config.cluster.get_all_peers().is_empty() {
        let auto_join_mgmt = management.clone();
        let auto_join_config = config.clone();

        tokio::spawn(async move {
            // Wait for servers to be fully ready
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            match auto_join_mgmt.auto_join_cluster(&auto_join_config).await {
                Ok(joined) => {
                    if joined {
                        tracing::info!("🤝 Successfully auto-joined cluster");
                    } else {
                        tracing::info!(
                            "🏷️  Auto-join not needed (likely we are leader or no leader found)"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("❌ Auto-join failed: {}", e);
                }
            }
        });
    }

    // Start HTTP server
    let http_addr = config.node.http_addr;
    let app_config = config.clone();
    let http_server = async move {
        tracing::info!("🌐 Starting HTTP server on {}", http_addr);

        HttpServer::new(move || {
            let cors = Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .max_age(3600);

            App::new()
                .app_data(web::Data::new((*raft).clone()))
                .app_data(web::Data::new((*management).clone()))
                .app_data(web::Data::new(app_config.clone()))
                .wrap(cors)
                .wrap(Logger::default())
                // Health and status endpoints
                .route("/health", web::get().to(api::health))
                .route("/metrics", web::get().to(api::metrics))
                .route("/is-leader", web::get().to(api::is_leader))
                .route("/leader", web::get().to(api::leader))
                // Management endpoints
                .route("/init", web::post().to(api::init))
                .route("/add-learner", web::post().to(api::add_learner))
                .route("/change-membership", web::post().to(api::change_membership))
                // KV endpoints
                .route("/write", web::post().to(api::write))
                .route("/read", web::post().to(api::read))
                // Raft RPC endpoints
                .route("/raft/append-entries", web::post().to(api::append_entries))
                .route("/raft/vote", web::post().to(api::vote))
                .route(
                    "/raft/install-snapshot",
                    web::post().to(api::install_snapshot),
                )
        })
        .bind(http_addr)?
        .run()
        .await
    };

    // Run both servers concurrently
    tokio::select! {
        result = http_server => {
            tracing::error!("❌ HTTP server stopped: {:?}", result);
            result
        }
        result = grpc_server => {
            match result {
                Ok(Ok(())) => {
                    tracing::info!("✅ gRPC server completed successfully");
                    Ok(())
                }
                Ok(Err(e)) => {
                    tracing::error!("❌ gRPC server error: {:?}", e);
                    Err(e)
                }
                Err(e) => {
                    tracing::error!("❌ gRPC server task error: {:?}", e);
                    Err(std::io::Error::new(std::io::ErrorKind::Other, e))
                }
            }
        }
    }
}

/// Load configuration with CLI overrides
fn load_configuration(args: &Args) -> Result<FerriteConfig, ConfigError> {
    // Load base configuration
    let mut config = match &args.config {
        Some(path) => {
            tracing::info!("📄 Loading config from: {}", path.display());
            FerriteConfig::from_file(path)?
        }
        None => FerriteConfig::load_default()?,
    };

    // Apply CLI overrides
    if let Some(id) = args.id {
        config.node.id = id;
    }

    if let Some(ref http_addr) = args.http_addr {
        config.node.http_addr = http_addr
            .parse()
            .map_err(|e| ConfigError::Validation(format!("Invalid HTTP address: {}", e)))?;
    }

    if let Some(ref grpc_addr) = args.grpc_addr {
        config.node.grpc_addr = grpc_addr
            .parse()
            .map_err(|e| ConfigError::Validation(format!("Invalid gRPC address: {}", e)))?;
    }

    if let Some(ref data_dir) = args.data_dir {
        config.node.data_dir = data_dir.clone();
    }

    if let Some(ref log_level) = args.log_level {
        config.logging.level = log_level.clone();
    }

    // Validate final configuration
    config.validate()?;
    Ok(config)
}

/// Setup logging based on configuration
fn setup_logging(config: &FerriteConfig) -> std::io::Result<()> {
    use tracing_subscriber::fmt::time::ChronoUtc;

    let level = config
        .logging
        .level
        .parse::<tracing::Level>()
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid log level: {}", e),
            )
        })?;

    let env_filter = EnvFilter::from_default_env()
        .add_directive(format!("ferrite={}", level).parse().unwrap())
        .add_directive(
            format!(
                "openraft={}",
                if level <= tracing::Level::DEBUG {
                    "debug"
                } else {
                    "info"
                }
            )
            .parse()
            .unwrap(),
        );

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_timer(ChronoUtc::rfc_3339())
        .with_span_events(FmtSpan::CLOSE)
        .with_target(config.logging.structured);

    match (&config.logging.format, config.logging.enable_colors) {
        (ferrite::config::LogFormat::Json, _) => subscriber.json().init(),
        (ferrite::config::LogFormat::Compact, true) => subscriber.compact().with_ansi(true).init(),
        (ferrite::config::LogFormat::Compact, false) => {
            subscriber.compact().with_ansi(false).init()
        }
        (ferrite::config::LogFormat::Pretty, true) => subscriber.pretty().with_ansi(true).init(),
        (ferrite::config::LogFormat::Pretty, false) => subscriber.pretty().with_ansi(false).init(),
    }

    Ok(())
}

/// Generate a default configuration file
fn generate_default_config(path: PathBuf) -> std::io::Result<()> {
    let config = FerriteConfig::default();

    config.to_file(&path).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to write config: {}", e),
        )
    })?;

    println!(
        "✅ Generated default configuration file: {}",
        path.display()
    );
    println!("📝 Edit the file to customize your Ferrite node settings");
    println!("🚀 Start with: ferrite-server --config {}", path.display());

    Ok(())
}

/// List default configuration file locations
fn list_config_paths() -> std::io::Result<()> {
    println!("📍 Default configuration file locations (in order of precedence):");
    println!();

    for (i, path) in FerriteConfig::default_config_paths().iter().enumerate() {
        let exists = if path.exists() { "✅" } else { "❌" };
        println!("  {}. {} {}", i + 1, exists, path.display());
    }

    println!();
    println!("💡 Tips:");
    println!("   • Create a config file in any of these locations");
    println!("   • Use --config <path> to specify a custom location");
    println!("   • Use --generate-config <path> to create a default config");

    Ok(())
}

fn parse_peer(peer: &str) -> Result<(NodeId, String), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = peer.split('=').collect();
    if parts.len() != 2 {
        return Err("Peer format should be 'id=address'".into());
    }

    let id: NodeId = parts[0].parse()?;
    let addr = parts[1].to_string();

    Ok((id, addr))
}

async fn health_check(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("http://{}/health", addr);
    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?;

    if response.status().is_success() {
        tracing::info!("Health check passed for {}", addr);
        Ok(())
    } else {
        Err(format!("Health check failed for {}: {}", addr, response.status()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_peer() {
        assert_eq!(
            parse_peer("1=127.0.0.1:8001").unwrap(),
            (1, "127.0.0.1:8001".to_string())
        );
        assert_eq!(
            parse_peer("2=127.0.0.1:8002").unwrap(),
            (2, "127.0.0.1:8002".to_string())
        );
        assert!(parse_peer("invalid").is_err());
        assert!(parse_peer("abc=127.0.0.1:8001").is_err());
    }
}
