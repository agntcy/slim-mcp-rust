// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

pub mod auth;
pub mod client;
pub mod connection;
pub mod error;
pub mod server;

use slim_auth::auth_provider::{AuthProvider, AuthVerifier};
use slim_service::app::App;

/// The SLIM app type used by both the client and server transports.
pub type SlimApp = App<AuthProvider, AuthVerifier>;

pub use auth::IdentityConfig;
pub use client::{SlimClientConfig, SlimClientTransport, SlimClientWorker};
pub use connection::{SlimConnection, insecure_client_config};
pub use error::SlimTransportError;
pub use server::{SlimServerListener, SlimServerTransport, SlimServerWorker};

// SLIM types that appear in this crate's API, so callers don't need to depend
// on matching versions of the SLIM crates.
pub use slim_config::client::ClientConfig;
pub use slim_datapath::api::{ProtoName, ProtoSessionType};
pub use slim_session::SessionConfig;
