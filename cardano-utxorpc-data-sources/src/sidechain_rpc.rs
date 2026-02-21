// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{proto::query::*, types::*, DataSourceError, UtxoRpcClient};
use async_trait::async_trait;
use derive_new::new;
use pallet_sidechain_rpc::SidechainRpcDataSource;
use sidechain_domain::{MainchainBlock, McBlockNumber, McEpochNumber, McSlotNumber};

#[derive(Clone, new)]
pub struct UtxoRpcSidechainRpcDataSource {
    client: UtxoRpcClient,
}

#[async_trait]
impl SidechainRpcDataSource for UtxoRpcSidechainRpcDataSource {
    async fn get_latest_block_info(
        &self,
    ) -> Result<MainchainBlock, Box<dyn std::error::Error + Send + Sync>> {
        let mut client = self.client.query_client.clone();

        let response = client
            .get_chain_tip(GetChainTipRequest {})
            .await
            .map_err(DataSourceError::from)?;

        let tip = response.into_inner();

        Ok(MainchainBlock {
            number: McBlockNumber(tip.slot as u32),
            hash: bytes_to_mc_block_hash(&tip.hash)?,
            epoch: McEpochNumber((tip.slot / self.client.config.security_parameter) as u32),
            slot: McSlotNumber(tip.slot),
            timestamp: 0, // TODO: Get actual timestamp if available
        })
    }
}
