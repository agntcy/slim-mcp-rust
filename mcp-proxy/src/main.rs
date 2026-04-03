// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use slim::config;
use slim_datapath::messages::Name;
use std::time::Duration;
use tracing::{error, info};

mod proxy;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// SLIM configuration file
    #[arg(short, long, value_name = "configuration", required = true)]
    config: String,

    /// Service name to look for in the configuration file
    #[arg(long, value_name = "svc_name", required = true)]
    svc_name: String,

    /// MCP Proxy name in the form org/ns/type
    #[arg(short, long, value_name = "proxy_name", required = true)]
    name: String,

    /// MCP Proxy instance ID
    #[arg(short, long, value_name = "id", required = false)]
    id: Option<u64>,

    /// MCP Server address (e.g http://localhost:8000/sse)
    #[arg(short, long, value_name = "address", required = true)]
    mcp_server: String,

    /// MCP Proxy shared secret
    #[arg(short = 's', long, value_name = "secret", required = false)]
    secret: Option<String>,

    /// SPIRE Workload API socket path (e.g. unix:/tmp/spire-agent/public/api.sock)
    #[arg(long, value_name = "socket_path", required = false)]
    spire_socket_path: Option<String>,

    /// SPIRE target SPIFFE ID
    #[arg(long, value_name = "spiffe_id", required = false)]
    spire_target_spiffe_id: Option<String>,

    /// SPIRE JWT audiences (comma-separated)
    #[arg(long, value_name = "audiences", required = false)]
    spire_jwt_audience: Option<String>,
}

impl Args {
    pub fn config(&self) -> &String {
        &self.config
    }

    pub fn svc_name(&self) -> &String {
        &self.svc_name
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn id(&self) -> Option<&u64> {
        self.id.as_ref()
    }

    pub fn mcp_server(&self) -> &String {
        &self.mcp_server
    }

    pub fn secret(&self) -> Option<&String> {
        self.secret.as_ref()
    }

    pub fn spire_socket_path(&self) -> Option<&String> {
        self.spire_socket_path.as_ref()
    }

    pub fn spire_target_spiffe_id(&self) -> Option<&String> {
        self.spire_target_spiffe_id.as_ref()
    }

    pub fn spire_jwt_audience(&self) -> Option<&String> {
        self.spire_jwt_audience.as_ref()
    }
}

#[tokio::main]
async fn main() {
    // parse command line
    let args = Args::parse();

    let config_file = args.config();
    let svc_name = args.svc_name();
    let name = args.name();
    let _id = args.id();
    let server = args.mcp_server();
    let secret = args.secret();
    let spire_socket_path = args.spire_socket_path();
    let spire_target_spiffe_id = args.spire_target_spiffe_id();
    let spire_jwt_audience = args.spire_jwt_audience();

    let v_name: Vec<&str> = name.split('/').collect();
    if v_name.len() != 3 {
        error!("error processing the MCP proxy name, invalid format");
        return;
    }

    let mut config = config::ConfigLoader::new(config_file).expect("failed to load configuration");
    let svc_id = slim_config::component::id::ID::new_with_str(svc_name).unwrap();
    let _guard = config
        .tracing()
        .expect("failed to get tracing configuration")
        .setup_tracing_subscriber();

    let services = config.services().expect("error loading services");
    let service = services.shift_remove(&svc_id).expect("service not found");

    // Create identity configuration based on command line arguments
    let identity_config = if let Some(socket_path) = spire_socket_path {
        // Use SPIRE authentication
        let jwt_audiences = spire_jwt_audience
            .map(|s| s.split(',').map(|a| a.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["slim".to_string()]);

        proxy::IdentityConfig::Spire {
            socket_path: Some(socket_path.clone()),
            target_spiffe_id: spire_target_spiffe_id.cloned(),
            jwt_audiences,
        }
    } else if let Some(secret_str) = secret {
        // Use shared secret authentication
        proxy::IdentityConfig::SharedSecret(secret_str.clone())
    } else {
        error!("No authentication method provided");
        return;
    };

    let mut proxy = proxy::Proxy::new(
        Name::from_strings([v_name[0], v_name[1], v_name[2]]),
        server.clone(),
    );

    info!("starting MCP proxy");
    proxy
        .start(service, identity_config, Duration::from_secs(10))
        .await;
}
