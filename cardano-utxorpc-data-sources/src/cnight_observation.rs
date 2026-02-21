// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{proto::query::*, types::*, DataSourceError, UtxoRpcClient};
use async_trait::async_trait;
use cardano_serialization_lib::PlutusData;
use derive_new::new;
use midnight_primitives_cnight_observation::{
    CNightAddresses, CardanoPosition, CardanoRewardAddressBytes, CreateData, DeregistrationData,
    DustPublicKeyBytes, ObservedUtxo, ObservedUtxoData, ObservedUtxoHeader, ObservedUtxos,
    RedemptionCreateData, RedemptionSpendData, RegistrationData, SpendData, UtxoIndexInTx,
    CARDANO_REWARD_ADDRESS_LENGTH,
};
use midnight_primitives_mainchain_follower::MidnightCNightObservationDataSource;
use pallas_addresses::Address as PallasAddress;
use sidechain_domain::McBlockHash;

#[derive(Clone, new)]
pub struct UtxoRpcCNightObservationDataSource {
    client: UtxoRpcClient,
}

#[async_trait]
impl MidnightCNightObservationDataSource for UtxoRpcCNightObservationDataSource {
    async fn get_utxos_up_to_capacity(
        &self,
        config: &CNightAddresses,
        start_position: &CardanoPosition,
        _current_tip: McBlockHash,
        capacity: usize,
    ) -> Result<ObservedUtxos, Box<dyn std::error::Error + Send + Sync>> {
        let mut client = self.client.query_client.clone();

        // Get current chain tip to determine end position
        let tip_response = client
            .get_chain_tip(GetChainTipRequest {})
            .await
            .map_err(DataSourceError::from)?;

        let tip = tip_response.into_inner();

        // Calculate end position
        use midnight_primitives_cnight_observation::TimestampUnixMillis;
        let end = CardanoPosition {
            block_hash: bytes_to_mc_block_hash(&tip.hash)?,
            block_number: tip.slot as u32,
            block_timestamp: TimestampUnixMillis(0), // TODO: Get actual timestamp
            tx_index_in_block: 0, // TODO: Get actual tx index
        };

        // Extract network ID from mapping validator address
        use cardano_serialization_lib::Address;
        let mapping_addr = Address::from_bech32(&config.mapping_validator_address)
            .map_err(|e| DataSourceError::InvalidAddress(e.to_string()))?;
        let network = mapping_addr.network_id().map_err(|_| {
            DataSourceError::InvalidAddress("Failed to get network ID".to_string())
        })?;

        // Query all UTxO types
        log::info!(
            "Querying CNight UTxOs: mapping_validator={}, auth_token={}, cnight_policy={}, capacity={}",
            config.mapping_validator_address,
            config.auth_token_asset_name,
            hex::encode(&config.cnight_policy_id),
            capacity
        );

        // Query all CNight UTxO types
        // 1. Registration UTxOs (UTxOs created at mapping validator with auth token)
        let registration_utxos = self._query_registration_utxos(&mut client, config, network).await?;

        // 2. Deregistration UTxOs (spent mapping validator UTxOs)
        let deregistration_utxos = self._query_deregistration_utxos(&mut client, config, network, start_position, &end).await?;

        // 3. Asset Create UTxOs (cNIGHT minting events)
        let asset_create_utxos = self._query_asset_create_utxos(&mut client, config, network, start_position, &end).await?;

        // 4. Asset Spend UTxOs (cNIGHT burning events)
        let asset_spend_utxos = self._query_asset_spend_utxos(&mut client, config, network, start_position, &end).await?;

        // 5. Redemption Create UTxOs
        let redemption_create_utxos = self._query_redemption_create_utxos(&mut client, config, network, start_position, &end).await?;

        // 6. Redemption Spend UTxOs
        let redemption_spend_utxos = self._query_redemption_spend_utxos(&mut client, config, network, start_position, &end).await?;

        // Combine all UTxOs
        let mut all_utxos = Vec::new();
        all_utxos.extend(registration_utxos);
        all_utxos.extend(deregistration_utxos);
        all_utxos.extend(asset_create_utxos);
        all_utxos.extend(asset_spend_utxos);
        all_utxos.extend(redemption_create_utxos);
        all_utxos.extend(redemption_spend_utxos);

        // Sort by position and truncate to capacity
        all_utxos.sort();
        all_utxos.truncate(capacity);

        log::info!("Found {} CNight UTxOs (capacity: {})", all_utxos.len(), capacity);

        Ok(ObservedUtxos {
            start: start_position.clone(),
            end,
            utxos: all_utxos,
        })
    }
}

impl UtxoRpcCNightObservationDataSource {
    // Helper methods to implement specific UTxO queries

    /// Decode redemption datum from PlutusData
    /// Expected format: Constructor([owner_address_bytes])
    fn decode_redemption_datum(
        &self,
        datum_bytes: &[u8],
        network: u8,
    ) -> Result<CardanoRewardAddressBytes, DataSourceError> {
        use cardano_serialization_lib::{Address, BaseAddress, PlutusData, RewardAddress};

        // Decode CBOR to PlutusData
        let plutus_data = PlutusData::from_bytes(datum_bytes.to_vec())
            .map_err(|e| DataSourceError::DatumDecodingError(e.to_string()))?;

        let constr = plutus_data.as_constr_plutus_data().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Expected PlutusData to be a constructor".to_string())
        })?;

        let list = constr.data();

        // Extract owner address (first field)
        let owner_bytes = list.get(0).as_bytes().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Owner address must be bytes".to_string())
        })?;

        // Parse the Cardano address
        let cardano_address = Address::from_bytes(owner_bytes.clone())
            .map_err(|e| DataSourceError::DatumDecodingError(format!("Invalid address bytes: {}", e)))?;

        // Extract stake credential from Base Address
        let base_address = BaseAddress::from_address(&cardano_address)
            .ok_or_else(|| DataSourceError::DatumDecodingError("Not a Base Address".to_string()))?;

        let reward_address = RewardAddress::new(network, &base_address.stake_cred());
        let reward_bytes = reward_address.to_address().to_bytes();

        if reward_bytes.len() != CARDANO_REWARD_ADDRESS_LENGTH {
            return Err(DataSourceError::Generic(format!(
                "Reward address length mismatch: expected {}, got {}",
                CARDANO_REWARD_ADDRESS_LENGTH,
                reward_bytes.len()
            )));
        }

        let mut bytes = [0u8; CARDANO_REWARD_ADDRESS_LENGTH];
        bytes.copy_from_slice(&reward_bytes);

        Ok(CardanoRewardAddressBytes(bytes))
    }

    /// Decode registration datum from PlutusData
    /// Expected format: Constructor([Credential, DustPublicKeyBytes])
    /// where Credential = Constructor(tag=0 for KeyHash or tag=1 for ScriptHash, [hash_bytes])
    fn decode_registration_datum(
        &self,
        datum_bytes: &[u8],
    ) -> Result<(cardano_serialization_lib::Credential, DustPublicKeyBytes), DataSourceError> {
        use cardano_serialization_lib::{Credential, Ed25519KeyHash, ScriptHash};

        // Decode CBOR to PlutusData
        let plutus_data = PlutusData::from_bytes(datum_bytes.to_vec())
            .map_err(|e| DataSourceError::DatumDecodingError(e.to_string()))?;

        let constr = plutus_data.as_constr_plutus_data().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Expected PlutusData to be a constructor".to_string())
        })?;

        let list = constr.data();

        // Extract Credential (first field)
        let cardano_credential = list.get(0).as_constr_plutus_data().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Cardano credential must be a constructor".to_string())
        })?;

        let credential = match u64::from(cardano_credential.alternative()) {
            0 => {
                // KeyHash variant
                let hash_bytes = cardano_credential
                    .data()
                    .get(0)
                    .as_bytes()
                    .ok_or_else(|| {
                        DataSourceError::DatumDecodingError("KeyHash bytes not found".to_string())
                    })?;

                let key_hash = Ed25519KeyHash::from_bytes(hash_bytes).map_err(|e| {
                    DataSourceError::DatumDecodingError(format!("Invalid Ed25519KeyHash: {}", e))
                })?;

                Credential::from_keyhash(&key_hash)
            }
            1 => {
                // ScriptHash variant
                let hash_bytes = cardano_credential
                    .data()
                    .get(0)
                    .as_bytes()
                    .ok_or_else(|| {
                        DataSourceError::DatumDecodingError("ScriptHash bytes not found".to_string())
                    })?;

                let script_hash = ScriptHash::from_bytes(hash_bytes).map_err(|e| {
                    DataSourceError::DatumDecodingError(format!("Invalid ScriptHash: {}", e))
                })?;

                Credential::from_scripthash(&script_hash)
            }
            tag => {
                return Err(DataSourceError::DatumDecodingError(format!(
                    "Invalid credential tag: {}",
                    tag
                )));
            }
        };

        // Extract DustPublicKeyBytes (second field)
        let dust_address = list.get(1).as_bytes().ok_or_else(|| {
            DataSourceError::DatumDecodingError("Dust address must be bytes".to_string())
        })?;

        let dust_address_len = dust_address.len();
        let dust_public_key = DustPublicKeyBytes::try_from(dust_address).map_err(|_| {
            DataSourceError::DatumDecodingError(format!(
                "Invalid DUST public key length: expected 33, got {}",
                dust_address_len
            ))
        })?;

        Ok((credential, dust_public_key))
    }

    /// Convert Cardano Credential to CardanoRewardAddressBytes
    fn credential_to_reward_address_bytes(
        &self,
        credential: &cardano_serialization_lib::Credential,
        network: u8,
    ) -> Result<CardanoRewardAddressBytes, DataSourceError> {
        use cardano_serialization_lib::RewardAddress;

        let reward_address = RewardAddress::new(network, credential);
        let address_bytes = reward_address.to_address().to_bytes();

        if address_bytes.len() != CARDANO_REWARD_ADDRESS_LENGTH {
            return Err(DataSourceError::Generic(format!(
                "Reward address length mismatch: expected {}, got {}",
                CARDANO_REWARD_ADDRESS_LENGTH,
                address_bytes.len()
            )));
        }

        let mut bytes = [0u8; CARDANO_REWARD_ADDRESS_LENGTH];
        bytes.copy_from_slice(&address_bytes);

        Ok(CardanoRewardAddressBytes(bytes))
    }

    /// Decode Bech32 address to raw bytes
    fn decode_bech32_address(&self, bech32_addr: &str) -> Result<Vec<u8>, DataSourceError> {
        let addr = PallasAddress::from_bech32(bech32_addr)
            .map_err(|e| DataSourceError::InvalidAddress(e.to_string()))?;

        match addr {
            PallasAddress::Shelley(shelley) => Ok(shelley.to_vec()),
            _ => Err(DataSourceError::InvalidAddress(
                "Expected Shelley address".to_string(),
            )),
        }
    }

    /// Extract script hash (policy ID) from mapping validator address
    fn extract_script_hash_from_address(
        &self,
        bech32_address: &str,
    ) -> Result<Vec<u8>, DataSourceError> {
        use cardano_serialization_lib::{Address, EnterpriseAddress};

        let address = Address::from_bech32(bech32_address)
            .map_err(|e| DataSourceError::InvalidAddress(e.to_string()))?;

        let enterprise_addr = EnterpriseAddress::from_address(&address)
            .ok_or_else(|| DataSourceError::InvalidAddress("Not an EnterpriseAddress".to_string()))?;

        let script_hash = enterprise_addr
            .payment_cred()
            .to_scripthash()
            .ok_or_else(|| {
                DataSourceError::InvalidAddress(
                    "Address does not contain a script hash".to_string(),
                )
            })?;

        Ok(script_hash.to_bytes())
    }

    /// Convert payment address (Base Address) to reward address by extracting stake credential
    fn payment_address_to_reward_address(
        &self,
        address_bytes: &[u8],
        network: u8,
    ) -> Result<CardanoRewardAddressBytes, DataSourceError> {
        use cardano_serialization_lib::{Address, BaseAddress, RewardAddress};

        let address = Address::from_bytes(address_bytes.to_vec())
            .map_err(|e| DataSourceError::InvalidAddress(format!("Invalid address bytes: {}", e)))?;

        let base_address = BaseAddress::from_address(&address)
            .ok_or_else(|| DataSourceError::InvalidAddress("Not a Base Address".to_string()))?;

        let reward_address = RewardAddress::new(network, &base_address.stake_cred());
        let reward_bytes = reward_address.to_address().to_bytes();

        if reward_bytes.len() != CARDANO_REWARD_ADDRESS_LENGTH {
            return Err(DataSourceError::Generic(format!(
                "Reward address length mismatch: expected {}, got {}",
                CARDANO_REWARD_ADDRESS_LENGTH,
                reward_bytes.len()
            )));
        }

        let mut bytes = [0u8; CARDANO_REWARD_ADDRESS_LENGTH];
        bytes.copy_from_slice(&reward_bytes);

        Ok(CardanoRewardAddressBytes(bytes))
    }

    async fn _query_registration_utxos(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<
            tonic::transport::Channel,
        >,
        config: &CNightAddresses,
        network: u8,
    ) -> Result<Vec<ObservedUtxo>, DataSourceError> {
        // Decode address to bytes
        let address_bytes = self.decode_bech32_address(&config.mapping_validator_address)?;

        // Extract auth token policy ID from mapping validator address
        let auth_token_policy_id = self.extract_script_hash_from_address(&config.mapping_validator_address)?;

        // Query UTxOs at mapping validator address
        let request = ReadUtxosRequest {
            addresses: vec![address_bytes],
        };

        let response = client.read_utxos(request).await.map_err(DataSourceError::from)?;
        let result = response.into_inner();

        let mut observed_utxos = Vec::new();

        // Get current tip for position tracking
        // TODO: Improve position tracking with actual tx indices and timestamps from Hayate
        let _tip = client.get_chain_tip(GetChainTipRequest {}).await?.into_inner();

        for utxo in result.items {
            // Check if this UTxO contains the auth token
            let has_auth_token = utxo.assets.iter().any(|asset| {
                asset.policy_id == auth_token_policy_id
                    && asset.asset_name == config.auth_token_asset_name.as_bytes()
            });

            if !has_auth_token {
                continue;
            }

            // Decode registration datum
            if utxo.datum.is_empty() {
                log::warn!(
                    "Registration UTxO at {} has no datum, skipping",
                    hex::encode(&utxo.tx_hash)
                );
                continue;
            }

            let (credential, dust_public_key) = match self.decode_registration_datum(&utxo.datum) {
                Ok(data) => data,
                Err(e) => {
                    log::error!(
                        "Failed to decode registration datum for tx {}: {}",
                        hex::encode(&utxo.tx_hash),
                        e
                    );
                    continue;
                }
            };

            let cardano_reward_address = self.credential_to_reward_address_bytes(&credential, network)?;

            // Create observed UTxO with metadata from Hayate
            let header = ObservedUtxoHeader {
                tx_position: CardanoPosition {
                    block_hash: bytes_to_mc_block_hash(&utxo.created_at_block_hash)?,
                    block_number: utxo.created_at_slot as u32,
                    block_timestamp: midnight_primitives_cnight_observation::TimestampUnixMillis(
                        utxo.created_at_block_timestamp as i64
                    ),
                    tx_index_in_block: utxo.created_at_tx_index,
                },
                tx_hash: bytes_to_mc_tx_hash(&utxo.tx_hash)?,
                utxo_tx_hash: bytes_to_mc_tx_hash(&utxo.tx_hash)?,
                utxo_index: UtxoIndexInTx(utxo.output_index as u16),
            };

            observed_utxos.push(ObservedUtxo {
                header,
                data: ObservedUtxoData::Registration(RegistrationData {
                    cardano_reward_address,
                    dust_public_key,
                }),
            });
        }

        Ok(observed_utxos)
    }

    async fn _query_deregistration_utxos(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<
            tonic::transport::Channel,
        >,
        config: &CNightAddresses,
        network: u8,
        start: &CardanoPosition,
        end: &CardanoPosition,
    ) -> Result<Vec<ObservedUtxo>, DataSourceError> {
        use crate::proto::query::ReadUtxoEventsRequest;
        use std::collections::HashMap;

        // Decode mapping validator address to bytes for filtering
        let address_bytes = self.decode_bech32_address(&config.mapping_validator_address)?;

        // Query both CREATE and SPEND events for the mapping validator address
        let request = ReadUtxoEventsRequest {
            start_slot: start.block_number as u64,
            end_slot: end.block_number as u64,
            addresses: vec![address_bytes],
            max_events: 0, // No limit
        };

        let response = client.read_utxo_events(request).await.map_err(DataSourceError::from)?;
        let events = response.into_inner().events;

        // Build a map of CREATE events by (tx_hash, output_index) -> UTxO data
        let mut create_map: HashMap<(Vec<u8>, u32), crate::proto::query::Utxo> = HashMap::new();

        for event in &events {
            if event.event_type == 0 { // CREATED
                if let Some(utxo) = &event.utxo {
                    create_map.insert((event.tx_hash.clone(), event.output_index), utxo.clone());
                }
            }
        }

        // Process SPEND events and match with CREATE events
        let mut observed_utxos = Vec::new();
        let auth_token_policy_id = self.extract_script_hash_from_address(&config.mapping_validator_address)?;

        for event in events {
            if event.event_type == 1 { // SPENT
                // Look up the original CREATE event
                if let Some(original_utxo) = create_map.get(&(event.tx_hash.clone(), event.output_index)) {
                    // Check if this UTxO had the auth token (indicating it was a registration)
                    let has_auth_token = original_utxo.assets.iter().any(|asset| {
                        asset.policy_id == auth_token_policy_id
                            && asset.asset_name == config.auth_token_asset_name.as_bytes()
                    });

                    if !has_auth_token {
                        continue;
                    }

                    // Decode the registration datum to get the deregistration data
                    if original_utxo.datum.is_empty() {
                        log::warn!(
                            "Deregistration UTxO at {} has no datum, skipping",
                            hex::encode(&event.tx_hash)
                        );
                        continue;
                    }

                    let (credential, dust_public_key) = match self.decode_registration_datum(&original_utxo.datum) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!(
                                "Failed to decode deregistration datum for tx {}: {}",
                                hex::encode(&event.tx_hash),
                                e
                            );
                            continue;
                        }
                    };

                    let cardano_reward_address = self.credential_to_reward_address_bytes(&credential, network)?;

                    // Create observed UTxO with spend event metadata
                    let header = ObservedUtxoHeader {
                        tx_position: CardanoPosition {
                            block_hash: bytes_to_mc_block_hash(&event.block_hash)?,
                            block_number: event.slot as u32,
                            block_timestamp: midnight_primitives_cnight_observation::TimestampUnixMillis(
                                event.block_timestamp as i64
                            ),
                            tx_index_in_block: event.tx_index,
                        },
                        tx_hash: bytes_to_mc_tx_hash(&event.spent_by_tx_hash)?,
                        utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                        utxo_index: UtxoIndexInTx(event.output_index as u16),
                    };

                    observed_utxos.push(ObservedUtxo {
                        header,
                        data: ObservedUtxoData::Deregistration(DeregistrationData {
                            cardano_reward_address,
                            dust_public_key,
                        }),
                    });
                }
            }
        }

        Ok(observed_utxos)
    }

    async fn _query_asset_create_utxos(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<
            tonic::transport::Channel,
        >,
        config: &CNightAddresses,
        network: u8,
        start: &CardanoPosition,
        end: &CardanoPosition,
    ) -> Result<Vec<ObservedUtxo>, DataSourceError> {
        use crate::proto::query::ReadUtxoEventsRequest;

        // Query CREATE events for cNIGHT token minting
        let request = ReadUtxoEventsRequest {
            start_slot: start.block_number as u64,
            end_slot: end.block_number as u64,
            addresses: vec![], // No address filter - minting can happen anywhere
            max_events: 0, // No limit
        };

        let response = client.read_utxo_events(request).await.map_err(DataSourceError::from)?;
        let events = response.into_inner().events;

        let mut observed_utxos = Vec::new();

        for event in events {
            if event.event_type != 0 { // Only CREATED events
                continue;
            }

            let Some(utxo) = &event.utxo else {
                continue;
            };

            // Check if this UTxO contains cNIGHT tokens
            let cnight_amount: u64 = utxo.assets.iter()
                .filter(|asset| {
                    asset.policy_id == config.cnight_policy_id
                        && asset.asset_name == config.cnight_asset_name.as_bytes()
                })
                .map(|asset| asset.amount)
                .sum();

            if cnight_amount == 0 {
                continue; // No cNIGHT tokens
            }

            // Extract owner from UTxO address
            let owner = match self.payment_address_to_reward_address(&utxo.address, network) {
                Ok(addr) => addr,
                Err(e) => {
                    log::warn!(
                        "Failed to convert address for asset create at tx {}: {}",
                        hex::encode(&event.tx_hash),
                        e
                    );
                    continue;
                }
            };

            // Create observed UTxO
            let header = ObservedUtxoHeader {
                tx_position: CardanoPosition {
                    block_hash: bytes_to_mc_block_hash(&event.block_hash)?,
                    block_number: event.slot as u32,
                    block_timestamp: midnight_primitives_cnight_observation::TimestampUnixMillis(
                        event.block_timestamp as i64
                    ),
                    tx_index_in_block: event.tx_index,
                },
                tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                utxo_index: UtxoIndexInTx(event.output_index as u16),
            };

            observed_utxos.push(ObservedUtxo {
                header,
                data: ObservedUtxoData::AssetCreate(CreateData {
                    value: cnight_amount as u128,
                    owner,
                    utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                    utxo_tx_index: event.output_index as u16,
                }),
            });
        }

        Ok(observed_utxos)
    }

    async fn _query_asset_spend_utxos(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<
            tonic::transport::Channel,
        >,
        config: &CNightAddresses,
        network: u8,
        start: &CardanoPosition,
        end: &CardanoPosition,
    ) -> Result<Vec<ObservedUtxo>, DataSourceError> {
        use crate::proto::query::ReadUtxoEventsRequest;
        use std::collections::HashMap;

        // Query both CREATE and SPEND events to match spent UTxOs with their original data
        let request = ReadUtxoEventsRequest {
            start_slot: start.block_number as u64,
            end_slot: end.block_number as u64,
            addresses: vec![], // No address filter
            max_events: 0, // No limit
        };

        let response = client.read_utxo_events(request).await.map_err(DataSourceError::from)?;
        let events = response.into_inner().events;

        // Build a map of CREATE events by (tx_hash, output_index) -> UTxO data
        let mut create_map: HashMap<(Vec<u8>, u32), crate::proto::query::Utxo> = HashMap::new();

        for event in &events {
            if event.event_type == 0 { // CREATED
                if let Some(utxo) = &event.utxo {
                    // Only store UTxOs that have cNIGHT tokens
                    let has_cnight = utxo.assets.iter().any(|asset| {
                        asset.policy_id == config.cnight_policy_id
                            && asset.asset_name == config.cnight_asset_name.as_bytes()
                    });

                    if has_cnight {
                        create_map.insert((event.tx_hash.clone(), event.output_index), utxo.clone());
                    }
                }
            }
        }

        // Process SPEND events and match with CREATE events
        let mut observed_utxos = Vec::new();

        for event in events {
            if event.event_type == 1 { // SPENT
                // Look up the original CREATE event
                if let Some(original_utxo) = create_map.get(&(event.tx_hash.clone(), event.output_index)) {
                    // Calculate cNIGHT amount
                    let cnight_amount: u64 = original_utxo.assets.iter()
                        .filter(|asset| {
                            asset.policy_id == config.cnight_policy_id
                                && asset.asset_name == config.cnight_asset_name.as_bytes()
                        })
                        .map(|asset| asset.amount)
                        .sum();

                    // Extract owner from original UTxO address
                    let owner = match self.payment_address_to_reward_address(&original_utxo.address, network) {
                        Ok(addr) => addr,
                        Err(e) => {
                            log::warn!(
                                "Failed to convert address for asset spend at tx {}: {}",
                                hex::encode(&event.tx_hash),
                                e
                            );
                            continue;
                        }
                    };

                    // Create observed UTxO with spend event metadata
                    let header = ObservedUtxoHeader {
                        tx_position: CardanoPosition {
                            block_hash: bytes_to_mc_block_hash(&event.block_hash)?,
                            block_number: event.slot as u32,
                            block_timestamp: midnight_primitives_cnight_observation::TimestampUnixMillis(
                                event.block_timestamp as i64
                            ),
                            tx_index_in_block: event.tx_index,
                        },
                        tx_hash: bytes_to_mc_tx_hash(&event.spent_by_tx_hash)?,
                        utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                        utxo_index: UtxoIndexInTx(event.output_index as u16),
                    };

                    observed_utxos.push(ObservedUtxo {
                        header,
                        data: ObservedUtxoData::AssetSpend(SpendData {
                            value: cnight_amount as u128,
                            owner,
                            utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                            utxo_tx_index: event.output_index as u16,
                            spending_tx_hash: bytes_to_mc_tx_hash(&event.spent_by_tx_hash)?,
                        }),
                    });
                }
            }
        }

        Ok(observed_utxos)
    }

    async fn _query_redemption_create_utxos(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<
            tonic::transport::Channel,
        >,
        config: &CNightAddresses,
        network: u8,
        start: &CardanoPosition,
        end: &CardanoPosition,
    ) -> Result<Vec<ObservedUtxo>, DataSourceError> {
        use crate::proto::query::ReadUtxoEventsRequest;

        // Decode redemption validator address to bytes for filtering
        let address_bytes = self.decode_bech32_address(&config.redemption_validator_address)?;

        // Query CREATE events at the redemption validator address
        let request = ReadUtxoEventsRequest {
            start_slot: start.block_number as u64,
            end_slot: end.block_number as u64,
            addresses: vec![address_bytes],
            max_events: 0, // No limit
        };

        let response = client.read_utxo_events(request).await.map_err(DataSourceError::from)?;
        let events = response.into_inner().events;

        let mut observed_utxos = Vec::new();

        for event in events {
            if event.event_type != 0 { // Only CREATED events
                continue;
            }

            let Some(utxo) = &event.utxo else {
                continue;
            };

            // Check if this UTxO contains cNIGHT tokens
            let cnight_amount: u64 = utxo.assets.iter()
                .filter(|asset| {
                    asset.policy_id == config.cnight_policy_id
                        && asset.asset_name == config.cnight_asset_name.as_bytes()
                })
                .map(|asset| asset.amount)
                .sum();

            if cnight_amount == 0 {
                continue; // No cNIGHT tokens
            }

            // Decode the redemption datum to get the owner
            if utxo.datum.is_empty() {
                log::warn!(
                    "Redemption create UTxO at {} has no datum, skipping",
                    hex::encode(&event.tx_hash)
                );
                continue;
            }

            let owner = match self.decode_redemption_datum(&utxo.datum, network) {
                Ok(addr) => addr,
                Err(e) => {
                    log::error!(
                        "Failed to decode redemption datum for tx {}: {}",
                        hex::encode(&event.tx_hash),
                        e
                    );
                    continue;
                }
            };

            // Create observed UTxO
            let header = ObservedUtxoHeader {
                tx_position: CardanoPosition {
                    block_hash: bytes_to_mc_block_hash(&event.block_hash)?,
                    block_number: event.slot as u32,
                    block_timestamp: midnight_primitives_cnight_observation::TimestampUnixMillis(
                        event.block_timestamp as i64
                    ),
                    tx_index_in_block: event.tx_index,
                },
                tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                utxo_index: UtxoIndexInTx(event.output_index as u16),
            };

            observed_utxos.push(ObservedUtxo {
                header,
                data: ObservedUtxoData::RedemptionCreate(RedemptionCreateData {
                    owner,
                    value: cnight_amount as u128,
                    utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                    utxo_tx_index: event.output_index as u16,
                }),
            });
        }

        Ok(observed_utxos)
    }

    async fn _query_redemption_spend_utxos(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<
            tonic::transport::Channel,
        >,
        config: &CNightAddresses,
        network: u8,
        start: &CardanoPosition,
        end: &CardanoPosition,
    ) -> Result<Vec<ObservedUtxo>, DataSourceError> {
        use crate::proto::query::ReadUtxoEventsRequest;
        use std::collections::HashMap;

        // Decode redemption validator address to bytes for filtering
        let address_bytes = self.decode_bech32_address(&config.redemption_validator_address)?;

        // Query both CREATE and SPEND events for the redemption validator address
        let request = ReadUtxoEventsRequest {
            start_slot: start.block_number as u64,
            end_slot: end.block_number as u64,
            addresses: vec![address_bytes],
            max_events: 0, // No limit
        };

        let response = client.read_utxo_events(request).await.map_err(DataSourceError::from)?;
        let events = response.into_inner().events;

        // Build a map of CREATE events by (tx_hash, output_index) -> UTxO data
        let mut create_map: HashMap<(Vec<u8>, u32), crate::proto::query::Utxo> = HashMap::new();

        for event in &events {
            if event.event_type == 0 { // CREATED
                if let Some(utxo) = &event.utxo {
                    // Only store UTxOs that have cNIGHT tokens
                    let has_cnight = utxo.assets.iter().any(|asset| {
                        asset.policy_id == config.cnight_policy_id
                            && asset.asset_name == config.cnight_asset_name.as_bytes()
                    });

                    if has_cnight {
                        create_map.insert((event.tx_hash.clone(), event.output_index), utxo.clone());
                    }
                }
            }
        }

        // Process SPEND events and match with CREATE events
        let mut observed_utxos = Vec::new();

        for event in events {
            if event.event_type == 1 { // SPENT
                // Look up the original CREATE event
                if let Some(original_utxo) = create_map.get(&(event.tx_hash.clone(), event.output_index)) {
                    // Calculate cNIGHT amount
                    let cnight_amount: u64 = original_utxo.assets.iter()
                        .filter(|asset| {
                            asset.policy_id == config.cnight_policy_id
                                && asset.asset_name == config.cnight_asset_name.as_bytes()
                        })
                        .map(|asset| asset.amount)
                        .sum();

                    // Decode the redemption datum to get the owner
                    if original_utxo.datum.is_empty() {
                        log::warn!(
                            "Redemption spend UTxO at {} has no datum, skipping",
                            hex::encode(&event.tx_hash)
                        );
                        continue;
                    }

                    let owner = match self.decode_redemption_datum(&original_utxo.datum, network) {
                        Ok(addr) => addr,
                        Err(e) => {
                            log::error!(
                                "Failed to decode redemption datum for spent tx {}: {}",
                                hex::encode(&event.tx_hash),
                                e
                            );
                            continue;
                        }
                    };

                    // Create observed UTxO with spend event metadata
                    let header = ObservedUtxoHeader {
                        tx_position: CardanoPosition {
                            block_hash: bytes_to_mc_block_hash(&event.block_hash)?,
                            block_number: event.slot as u32,
                            block_timestamp: midnight_primitives_cnight_observation::TimestampUnixMillis(
                                event.block_timestamp as i64
                            ),
                            tx_index_in_block: event.tx_index,
                        },
                        tx_hash: bytes_to_mc_tx_hash(&event.spent_by_tx_hash)?,
                        utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                        utxo_index: UtxoIndexInTx(event.output_index as u16),
                    };

                    observed_utxos.push(ObservedUtxo {
                        header,
                        data: ObservedUtxoData::RedemptionSpend(RedemptionSpendData {
                            owner,
                            value: cnight_amount as u128,
                            utxo_tx_hash: bytes_to_mc_tx_hash(&event.tx_hash)?,
                            utxo_tx_index: event.output_index as u16,
                            spending_tx_hash: bytes_to_mc_tx_hash(&event.spent_by_tx_hash)?,
                        }),
                    });
                }
            }
        }

        Ok(observed_utxos)
    }
}
