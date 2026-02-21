// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::UtxoRpcClient;
use async_trait::async_trait;
use derive_new::new;
use sidechain_domain::McBlockHash;
use sp_partner_chains_bridge::{
    BridgeDataCheckpoint, BridgeTransferV1, MainChainScripts, TokenBridgeDataSource,
};

#[derive(Clone, new)]
pub struct UtxoRpcTokenBridgeDataSource<Recipient> {
    client: UtxoRpcClient,
    _phantom: std::marker::PhantomData<Recipient>,
}

#[async_trait]
impl<Recipient: Send + Sync> TokenBridgeDataSource<Recipient>
    for UtxoRpcTokenBridgeDataSource<Recipient>
{
    async fn get_transfers(
        &self,
        _scripts: MainChainScripts,
        _checkpoint: BridgeDataCheckpoint,
        _lookahead: u32,
        _to_block: McBlockHash,
    ) -> Result<
        (Vec<BridgeTransferV1<Recipient>>, BridgeDataCheckpoint),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        // TODO: Implement bridge transfer query
        log::warn!(
            "UtxoRpcTokenBridgeDataSource::get_transfers not fully implemented, returning empty transfers"
        );
        // Return empty transfers with the same checkpoint
        Ok((vec![], _checkpoint))
    }
}
