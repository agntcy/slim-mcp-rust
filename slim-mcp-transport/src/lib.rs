// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

pub mod auth;
pub mod client;
pub mod error;
pub mod server;

use slim_auth::auth_provider::{AuthProvider, AuthVerifier};
use slim_service::app::App;

/// The SLIM app type used by both the client and server transports.
pub type SlimApp = App<AuthProvider, AuthVerifier>;

pub use auth::IdentityConfig;
pub use client::{SlimClientConfig, SlimClientTransport, SlimClientWorker};
pub use error::SlimTransportError;
pub use server::{SlimServerListener, SlimServerTransport, SlimServerWorker};
