// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{proto::query::*, types::*, DataSourceError, UtxoRpcClient};
use async_trait::async_trait;
use derive_new::new;
use sidechain_domain::{MainchainBlock, McBlockHash, McBlockNumber, McEpochNumber, McSlotNumber};
use sidechain_mc_hash::McHashDataSource;
use sp_timestamp::Timestamp;

#[derive(Clone, new)]
pub struct UtxoRpcMcHashDataSource {
    client: UtxoRpcClient,
}

#[async_trait]
impl McHashDataSource for UtxoRpcMcHashDataSource {
    async fn get_latest_stable_block_for(
        &self,
        _timestamp: Timestamp,
    ) -> Result<Option<MainchainBlock>, Box<dyn std::error::Error + Send + Sync>> {
        // Get current chain tip and calculate stable block
        let mut client = self.client.query_client.clone();

        let response = client
            .get_chain_tip(GetChainTipRequest {})
            .await
            .map_err(DataSourceError::from)?;

        let tip = response.into_inner();

        // Calculate stable slot
        let stable_slot = tip.slot.saturating_sub(self.client.config.security_parameter);

        Ok(Some(MainchainBlock {
            hash: bytes_to_mc_block_hash(&tip.hash)?,
            number: McBlockNumber(stable_slot as u32),
            slot: McSlotNumber(stable_slot),
            epoch: McEpochNumber((stable_slot / self.client.config.security_parameter) as u32),
            timestamp: 0, // TODO: Calculate actual timestamp
        }))
    }

    async fn get_stable_block_for(
        &self,
        block_hash: McBlockHash,
        _timestamp: Timestamp,
    ) -> Result<Option<MainchainBlock>, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Query specific block by hash
        // For now, return None as we don't have block-by-hash query
        log::warn!(
            "get_stable_block_for({:?}) not fully implemented, returning None",
            block_hash
        );
        Ok(None)
    }

    async fn get_block_by_hash(
        &self,
        block_hash: McBlockHash,
    ) -> Result<Option<MainchainBlock>, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: Implement block-by-hash query
        // This requires additional UTxO RPC functionality
        log::warn!(
            "get_block_by_hash({:?}) not fully implemented, returning None",
            block_hash
        );
        Ok(None)
    }
}
