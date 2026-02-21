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

        // Calculate stable slot (current tip - security parameter)
        let stable_slot = tip.slot.saturating_sub(self.client.config.security_parameter);

        // Calculate approximate timestamp for stable slot
        // Each slot is 1 second, so subtract (tip.slot - stable_slot) seconds from tip timestamp
        let slot_diff = tip.slot.saturating_sub(stable_slot);
        let stable_timestamp = tip.timestamp.saturating_sub(slot_diff * 1000); // milliseconds

        // Note: This uses the tip's hash which is not the actual stable block hash
        // This is a limitation since we can only query by hash, not by slot
        Ok(Some(MainchainBlock {
            hash: bytes_to_mc_block_hash(&tip.hash)?,
            number: McBlockNumber(stable_slot as u32),
            slot: McSlotNumber(stable_slot),
            epoch: McEpochNumber((stable_slot / self.client.config.security_parameter) as u32),
            timestamp: stable_timestamp,
        }))
    }

    async fn get_stable_block_for(
        &self,
        block_hash: McBlockHash,
        _timestamp: Timestamp,
    ) -> Result<Option<MainchainBlock>, Box<dyn std::error::Error + Send + Sync>> {
        // Query the block by hash
        let block = match self.get_block_by_hash(block_hash).await? {
            Some(b) => b,
            None => return Ok(None),
        };

        // Get current chain tip to verify the block is stable
        let mut client = self.client.query_client.clone();
        let response = client
            .get_chain_tip(GetChainTipRequest {})
            .await
            .map_err(DataSourceError::from)?;

        let tip = response.into_inner();

        // Check if block is stable (beyond security parameter from tip)
        let stable_slot = tip.slot.saturating_sub(self.client.config.security_parameter);

        if block.slot.0 <= stable_slot {
            Ok(Some(block))
        } else {
            // Block is not yet stable
            Ok(None)
        }
    }

    async fn get_block_by_hash(
        &self,
        block_hash: McBlockHash,
    ) -> Result<Option<MainchainBlock>, Box<dyn std::error::Error + Send + Sync>> {
        let mut client = self.client.query_client.clone();

        let request = GetBlockByHashRequest {
            hash: block_hash.0.to_vec(),
        };

        let response = match client.get_block_by_hash(request).await {
            Ok(resp) => resp,
            Err(e) => {
                // If block not found, return None
                if e.code() == tonic::Code::NotFound {
                    return Ok(None);
                }
                return Err(Box::new(DataSourceError::from(e)));
            }
        };

        let block_info = response.into_inner();

        Ok(Some(MainchainBlock {
            hash: block_hash,
            number: McBlockNumber(block_info.slot as u32),
            slot: McSlotNumber(block_info.slot),
            epoch: McEpochNumber((block_info.slot / self.client.config.security_parameter) as u32),
            timestamp: block_info.timestamp,
        }))
    }
}
