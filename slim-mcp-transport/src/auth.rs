// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use slim_auth::auth_provider::{AuthProvider, AuthVerifier};
use slim_auth::shared_secret::SharedSecret;

use crate::error::SlimTransportError;

/// Authentication configuration for SLIM connections.
#[derive(Clone)]
pub enum IdentityConfig {
    /// Shared secret (pre-shared key) authentication.
    SharedSecret {
        identity: String,
        secret: String,
    },
    /// SPIRE-based mTLS/JWT authentication.
    #[cfg(all(not(target_family = "windows"), not(target_arch = "wasm32")))]
    Spire {
        socket_path: Option<String>,
        target_spiffe_id: Option<String>,
        jwt_audiences: Vec<String>,
    },
}

impl IdentityConfig {
    /// Convenience constructor for shared secret.
    pub fn shared_secret(identity: impl Into<String>, secret: impl Into<String>) -> Self {
        Self::SharedSecret {
            identity: identity.into(),
            secret: secret.into(),
        }
    }

    /// Build an `(AuthProvider, AuthVerifier)` pair from this config.
    pub async fn into_auth_pair(
        self,
    ) -> Result<(AuthProvider, AuthVerifier), SlimTransportError> {
        match self {
            Self::SharedSecret { identity, secret } => {
                let provider = SharedSecret::new(&identity, &secret)
                    .map_err(|e| SlimTransportError::Auth(e.to_string()))?;
                let verifier = SharedSecret::new(&identity, &secret)
                    .map_err(|e| SlimTransportError::Auth(e.to_string()))?;
                Ok((
                    AuthProvider::shared_secret(provider),
                    AuthVerifier::shared_secret(verifier),
                ))
            }
            #[cfg(all(not(target_family = "windows"), not(target_arch = "wasm32")))]
            Self::Spire {
                socket_path,
                target_spiffe_id,
                jwt_audiences,
            } => {
                use slim_auth::spire::SpireIdentityManager;

                let mut pb = SpireIdentityManager::builder();
                let mut vb = SpireIdentityManager::builder();
                if let Some(path) = &socket_path {
                    pb = pb.with_socket_path(path.clone());
                    vb = vb.with_socket_path(path.clone());
                }
                if let Some(id) = &target_spiffe_id {
                    pb = pb.with_target_spiffe_id(id.clone());
                    vb = vb.with_target_spiffe_id(id.clone());
                }
                if !jwt_audiences.is_empty() {
                    pb = pb.with_jwt_audiences(jwt_audiences.clone());
                    vb = vb.with_jwt_audiences(jwt_audiences);
                }
                let mut pm = pb.build().map_err(|e| SlimTransportError::Auth(e.to_string()))?;
                pm.initialize()
                    .await
                    .map_err(|e| SlimTransportError::Auth(e.to_string()))?;
                let mut vm = vb.build().map_err(|e| SlimTransportError::Auth(e.to_string()))?;
                vm.initialize()
                    .await
                    .map_err(|e| SlimTransportError::Auth(e.to_string()))?;
                Ok((AuthProvider::spire(pm), AuthVerifier::spire(vm)))
            }
        }
    }
}
