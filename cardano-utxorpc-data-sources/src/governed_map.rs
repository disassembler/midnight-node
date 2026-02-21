// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{DataSourceError, UtxoRpcClient};
use async_trait::async_trait;
use cardano_serialization_lib::PlutusData;
use derive_new::new;
use sidechain_domain::{byte_string::ByteString, McBlockHash};
use sp_governed_map::{GovernedMapDataSource, MainChainScriptsV1};
use std::collections::BTreeMap;

#[derive(Clone, new)]
pub struct UtxoRpcGovernedMapDataSource {
    client: UtxoRpcClient,
}

impl UtxoRpcGovernedMapDataSource {
    /// Decodes a governed map datum from PlutusData
    /// Expected format: List([key_bytes, value_bytes])
    /// where key_bytes is UTF-8 encoded string
    fn decode_governed_map_datum(
        &self,
        datum_bytes: &[u8],
    ) -> Result<(String, ByteString), DataSourceError> {
        // Decode CBOR to PlutusData
        let plutus_data = PlutusData::from_bytes(datum_bytes.to_vec())
            .map_err(|e| DataSourceError::DatumDecodingError(e.to_string()))?;

        // Datum should be a list with 2 elements
        let list = plutus_data.as_list().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Expected PlutusData to be a list".to_string())
        })?;

        // First element is the key (UTF-8 string as bytes)
        let key_bytes = list.get(0).as_bytes().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Key must be bytes".to_string())
        })?;

        let key = String::from_utf8(key_bytes).map_err(|e| {
            DataSourceError::DatumDecodingError(format!("Key is not valid UTF-8: {}", e))
        })?;

        // Second element is the value (arbitrary bytes)
        let value_bytes = list.get(1).as_bytes().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Value must be bytes".to_string())
        })?;

        Ok((key, ByteString(value_bytes)))
    }

    /// Reconstructs governed map state at a specific block by querying events
    async fn query_governed_map_at_block(
        &self,
        block_hash: &McBlockHash,
        scripts: &MainChainScriptsV1,
    ) -> Result<BTreeMap<String, ByteString>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::proto::query::{GetBlockByHashRequest, ReadUtxoEventsRequest};
        use std::collections::HashMap;

        let mut client = self.client.query_client.clone();

        // Get the slot for this block hash using GetBlockByHash
        let block_request = GetBlockByHashRequest {
            hash: block_hash.0.to_vec(),
        };

        let block_response = client
            .get_block_by_hash(block_request)
            .await
            .map_err(DataSourceError::from)?;
        let block_info = block_response.into_inner();
        let end_slot = block_info.slot;

        // Decode validator address to bytes
        use pallas_addresses::Address as PallasAddress;
        let addr = PallasAddress::from_bech32(&scripts.validator_address.to_string())
            .map_err(|e| DataSourceError::InvalidAddress(e.to_string()))?;

        let address_bytes = match addr {
            PallasAddress::Shelley(shelley) => shelley.to_vec(),
            _ => {
                return Err(Box::new(DataSourceError::InvalidAddress(
                    "Expected Shelley address".to_string(),
                )));
            }
        };

        // Query all CREATE and SPEND events for this address from genesis to end_slot
        let request = ReadUtxoEventsRequest {
            start_slot: 0,
            end_slot,
            addresses: vec![address_bytes],
            max_events: 0, // No limit
        };

        let response = client
            .read_utxo_events(request)
            .await
            .map_err(DataSourceError::from)?;

        let events = response.into_inner().events;

        // Build a map of (tx_hash, output_index) -> UTxO data for CREATE events
        let mut utxo_map: HashMap<(Vec<u8>, u32), crate::proto::query::Utxo> = HashMap::new();

        for event in &events {
            if event.event_type == 0 {
                // CREATED
                if let Some(utxo) = &event.utxo {
                    // Only store if it has the governed map asset
                    let has_governed_asset = utxo
                        .assets
                        .iter()
                        .any(|asset| asset.policy_id == scripts.asset_policy_id.0);

                    if has_governed_asset {
                        utxo_map.insert((event.tx_hash.clone(), event.output_index), utxo.clone());
                    }
                }
            }
        }

        // Remove spent UTxOs
        for event in &events {
            if event.event_type == 1 {
                // SPENT
                utxo_map.remove(&(event.tx_hash.clone(), event.output_index));
            }
        }

        // Decode datums from remaining UTxOs to build the governed map
        let mut mappings = BTreeMap::new();

        for ((tx_hash, _), utxo) in utxo_map {
            if utxo.datum.is_empty() {
                log::warn!(
                    "Governed map UTxO at {} has no datum, skipping",
                    hex::encode(&tx_hash)
                );
                continue;
            }

            match self.decode_governed_map_datum(&utxo.datum) {
                Ok((key, value)) => {
                    mappings.insert(key, value);
                }
                Err(e) => {
                    log::error!(
                        "Failed to decode governed map datum for tx {}: {}",
                        hex::encode(&tx_hash),
                        e
                    );
                    continue;
                }
            }
        }

        Ok(mappings)
    }
}

#[async_trait]
impl GovernedMapDataSource for UtxoRpcGovernedMapDataSource {
    async fn get_state_at_block(
        &self,
        block_hash: McBlockHash,
        scripts: MainChainScriptsV1,
    ) -> Result<BTreeMap<String, ByteString>, Box<dyn std::error::Error + Send + Sync>> {
        self.query_governed_map_at_block(&block_hash, &scripts).await
    }

    async fn get_mapping_changes(
        &self,
        since_mc_block: Option<McBlockHash>,
        up_to_mc_block: McBlockHash,
        scripts: MainChainScriptsV1,
    ) -> Result<Vec<(String, Option<ByteString>)>, Box<dyn std::error::Error + Send + Sync>> {
        // Get current mappings
        let current_mappings = self
            .query_governed_map_at_block(&up_to_mc_block, &scripts)
            .await?;

        // If there's no previous block, all current mappings are new
        let Some(since_block) = since_mc_block else {
            let changes = current_mappings
                .into_iter()
                .map(|(key, value)| (key, Some(value)))
                .collect();
            return Ok(changes);
        };

        // Get previous mappings
        let previous_mappings = self.query_governed_map_at_block(&since_block, &scripts).await?;

        // Calculate changes
        let mut changes = vec![];

        // Check for new or updated mappings
        for (key, value) in current_mappings.iter() {
            if previous_mappings.get(key) != Some(value) {
                changes.push((key.clone(), Some(value.clone())));
            }
        }

        // Check for deleted mappings
        for key in previous_mappings.keys() {
            if !current_mappings.contains_key(key) {
                changes.push((key.clone(), None));
            }
        }

        Ok(changes)
    }
}
