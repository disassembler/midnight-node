// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! UTxO RPC data sources for Midnight Node
//!
//! This crate provides implementations of Midnight Node data source traits
//! using the UTxO RPC protocol backed by the Hayate LSM tree indexer.
//!
//! ## Architecture
//!
//! ```text
//! Cardano Node (SanchoNet)
//!     ↓ chain-sync (Pallas)
//! Hayate Indexer (LSM tree implementation)
//!     ↓ Internal Rust API
//! UTxO RPC gRPC Server (port 50051)
//!     ↓ gRPC protocol
//! midnight-node Data Sources (this crate)
//!     ↓ Trait interface
//! midnight-node Pallets
//! ```

// Include generated protobuf code
pub mod proto {
    pub mod query {
        tonic::include_proto!("utxorpc.query.v1");
    }
    pub mod watch {
        tonic::include_proto!("utxorpc.watch.v1");
    }
    pub mod submit {
        tonic::include_proto!("utxorpc.submit.v1");
    }
}

mod error;
mod types;
mod cnight_observation;
mod federated_authority_observation;
mod mc_hash;
mod sidechain_rpc;
mod governed_map;
mod bridge;
mod authority_selection;

pub use error::*;
pub use types::*;
pub use cnight_observation::*;
pub use federated_authority_observation::*;
pub use mc_hash::*;
pub use sidechain_rpc::*;
pub use governed_map::*;
pub use bridge::*;
pub use authority_selection::*;

use tonic::transport::Channel;

/// Configuration for UTxO RPC data sources
#[derive(Clone, Debug)]
pub struct UtxoRpcConfig {
    /// gRPC endpoint (e.g., "http://localhost:50051")
    pub endpoint: String,
    /// Cardano security parameter (k) for stable block calculation
    /// - SanchoNet: 432
    /// - Mainnet: 2160
    pub security_parameter: u64,
    /// Network magic number
    /// - SanchoNet: 4
    /// - Mainnet: 764824073
    pub network_magic: u64,
}

impl Default for UtxoRpcConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:50051".to_string(),
            security_parameter: 432,  // SanchoNet default
            network_magic: 4,         // SanchoNet default
        }
    }
}

/// Shared client state for UTxO RPC connections
#[derive(Clone)]
pub struct UtxoRpcClient {
    pub query_client: proto::query::query_service_client::QueryServiceClient<Channel>,
    pub watch_client: proto::watch::watch_service_client::WatchServiceClient<Channel>,
    pub config: UtxoRpcConfig,
}

impl UtxoRpcClient {
    /// Create a new UTxO RPC client
    pub async fn new(config: UtxoRpcConfig) -> Result<Self, tonic::transport::Error> {
        // Increase message size limits to handle large native token datasets
        // Default is 4MB, but CNight token data can exceed 8MB
        // Set to 128MB to match server-side limits with ample headroom
        const MAX_MESSAGE_SIZE: usize = 128 * 1024 * 1024; // 128MB

        let query_client = proto::query::query_service_client::QueryServiceClient::connect(
            config.endpoint.clone()
        ).await?
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE);

        let watch_client = proto::watch::watch_service_client::WatchServiceClient::connect(
            config.endpoint.clone()
        ).await?
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE);

        Ok(Self {
            query_client,
            watch_client,
            config,
        })
    }
}
