// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{proto::query::*, DataSourceError, UtxoRpcClient};
use async_trait::async_trait;
use cardano_serialization_lib::PlutusData;
use derive_new::new;
use midnight_primitives_federated_authority_observation::{
    AuthoritiesData, AuthorityMemberPublicKey, FederatedAuthorityData,
    FederatedAuthorityObservationConfig, GovernanceAuthorityDatums,
};
use midnight_primitives_mainchain_follower::FederatedAuthorityObservationDataSource;
use sidechain_domain::{McBlockHash, PolicyId};

#[derive(Clone, new)]
pub struct UtxoRpcFederatedAuthorityDataSource {
    client: UtxoRpcClient,
}

#[async_trait]
impl FederatedAuthorityObservationDataSource for UtxoRpcFederatedAuthorityDataSource {
    async fn get_federated_authority_data(
        &self,
        config: &FederatedAuthorityObservationConfig,
        mc_block_hash: &McBlockHash,
    ) -> Result<FederatedAuthorityData, Box<dyn std::error::Error + Send + Sync>> {
        let mut client = self.client.query_client.clone();

        // Query council governance UTxO
        let council_authorities = self
            .query_governance_body(
                &mut client,
                &config.council.address,
                &config.council.policy_id,
            )
            .await?;

        // Query technical committee governance UTxO
        let technical_committee_authorities = self
            .query_governance_body(
                &mut client,
                &config.technical_committee.address,
                &config.technical_committee.policy_id,
            )
            .await?;

        Ok(FederatedAuthorityData {
            council_authorities,
            technical_committee_authorities,
            mc_block_hash: mc_block_hash.clone(),
        })
    }
}

impl UtxoRpcFederatedAuthorityDataSource {
    async fn query_governance_body(
        &self,
        client: &mut crate::proto::query::query_service_client::QueryServiceClient<tonic::transport::Channel>,
        governance_address: &str,
        policy_id: &PolicyId,
    ) -> Result<AuthoritiesData, Box<dyn std::error::Error + Send + Sync>> {
        // Decode Bech32 address to bytes
        let address_bytes = self.decode_bech32_address(governance_address)?;

        // Query UTxOs at the governance address
        let request = ReadUtxosRequest {
            addresses: vec![address_bytes],
        };

        let response = client.read_utxos(request).await.map_err(DataSourceError::from)?;
        let result = response.into_inner();

        // Find UTxO with the governance policy ID
        let policy_bytes = &policy_id.0;

        for utxo in result.items {
            // Check if this UTxO contains the governance policy asset
            if self.has_policy(&utxo, policy_bytes) {
                // Extract and decode datum
                if !utxo.datum.is_empty() {
                    match self.decode_governance_datum(&utxo.datum) {
                        Ok(datum) => return Ok(AuthoritiesData::from(datum)),
                        Err(e) => {
                            log::warn!(
                                "Failed to decode governance datum at address {}: {}. Using empty list.",
                                governance_address,
                                e
                            );
                            return Ok(AuthoritiesData { authorities: vec![], round: 0 });
                        }
                    }
                }
            }
        }

        // No governance UTxO found
        log::warn!(
            "No governance UTXO found for address {} with policy {}. Using empty list.",
            governance_address,
            policy_id
        );
        Ok(AuthoritiesData { authorities: vec![], round: 0 })
    }

    fn has_policy(&self, utxo: &Utxo, policy_bytes: &[u8]) -> bool {
        for asset in &utxo.assets {
            if asset.policy_id == policy_bytes {
                return true;
            }
        }
        false
    }

    fn decode_bech32_address(&self, bech32_addr: &str) -> Result<Vec<u8>, DataSourceError> {
        use cardano_serialization_lib::Address;

        let addr = Address::from_bech32(bech32_addr)
            .map_err(|e| DataSourceError::InvalidAddress(e.to_string()))?;

        Ok(addr.to_bytes())
    }

    fn decode_governance_datum(
        &self,
        datum_bytes: &[u8],
    ) -> Result<GovernanceAuthorityDatums, Box<dyn std::error::Error + Send + Sync>> {
        // Decode CBOR to PlutusData
        let plutus_data = PlutusData::from_bytes(datum_bytes.to_vec())
            .map_err(|e| DataSourceError::DatumDecodingError(e.to_string()))?;

        // Parse the datum structure (see midnight-node implementation for details)
        // Expected format (VersionedMultisig):
        // {
        //   data: [total_signers: Int, {...(CborBytes, Sr25519Keys)}],
        //   round: Int
        // }
        self.parse_versioned_multisig(&plutus_data)
    }

    fn parse_versioned_multisig(
        &self,
        datum: &PlutusData,
    ) -> Result<GovernanceAuthorityDatums, Box<dyn std::error::Error + Send + Sync>> {
        let constr = datum
            .as_constr_plutus_data()
            .ok_or(DataSourceError::DatumDecodingError(
                "Expected PlutusData to be a constructor (VersionedMultisig)".to_string(),
            ))?;

        let fields = constr.data();

        if fields.len() < 2 {
            return Err(Box::new(DataSourceError::DatumDecodingError(format!(
                "VersionedMultisig must have at least 2 fields, got {}",
                fields.len()
            ))));
        }

        // Extract round number (second field)
        let round_bignum = fields
            .get(1)
            .as_integer()
            .ok_or(DataSourceError::DatumDecodingError(
                "Round field must be an integer".to_string(),
            ))?;
        let round_str = round_bignum.to_str();
        let round_u64: u64 = round_str.parse().map_err(|_| {
            DataSourceError::DatumDecodingError(format!("Failed to parse round: {}", round_str))
        })?;
        let round = round_u64 as u8;

        // Extract data field (first field is a list: [total_signers, members_map])
        let data_field = fields.get(0);
        let data_list = data_field.as_list().ok_or(DataSourceError::DatumDecodingError(
            "Data field must be a list".to_string(),
        ))?;

        if data_list.len() < 2 {
            return Err(Box::new(DataSourceError::DatumDecodingError(format!(
                "Data list must have at least 2 elements, got {}",
                data_list.len()
            ))));
        }

        let threshold_bignum = data_list
            .get(0)
            .as_integer()
            .ok_or(DataSourceError::DatumDecodingError(
                "Threshold must be an integer".to_string(),
            ))?;
        let threshold_str = threshold_bignum.to_str();
        let threshold_u64: u64 = threshold_str.parse().map_err(|_| {
            DataSourceError::DatumDecodingError(format!("Failed to parse threshold: {}", threshold_str))
        })?;
        let _threshold = threshold_u64 as u32;

        // Parse members map
        let members_map = data_list.get(1).as_map().ok_or(DataSourceError::DatumDecodingError(
            "Members field must be a map".to_string(),
        ))?;

        // Iterate over PlutusMap entries
        let mut authorities = Vec::new();
        let keys = members_map.keys();

        for i in 0..keys.len() {
            let key = keys.get(i);

            // Get the PlutusMapValues for this key
            let values = members_map.get(&key).ok_or(DataSourceError::DatumDecodingError(
                format!("Failed to get value for key {}", i)
            ))?;

            // Each key should map to exactly one value (the Sr25519 pubkey)
            // PlutusMapValues is a Vec, so get the first element
            let value = values.get(0).ok_or(DataSourceError::DatumDecodingError(
                format!("No value at index 0 for key {}", i)
            ))?;

            // Extract mainchain member (key) as PolicyId bytes
            let key_bytes = key.as_bytes().ok_or(DataSourceError::DatumDecodingError(
                "Member key must be bytes (policy ID)".to_string(),
            ))?;

            if key_bytes.len() != 28 {
                return Err(Box::new(DataSourceError::DatumDecodingError(format!(
                    "PolicyId must be 28 bytes, got {}",
                    key_bytes.len()
                ))));
            }

            let mut mainchain_member_bytes = [0u8; 28];
            mainchain_member_bytes.copy_from_slice(&key_bytes);
            let mainchain_member = PolicyId(mainchain_member_bytes);

            // Extract Sr25519 authority public key (value)
            let val_bytes = value.as_bytes().ok_or(DataSourceError::DatumDecodingError(
                "Member value must be bytes (Sr25519 pubkey)".to_string(),
            ))?;

            if val_bytes.len() != 32 {
                return Err(Box::new(DataSourceError::DatumDecodingError(format!(
                    "Sr25519 public key must be 32 bytes, got {}",
                    val_bytes.len()
                ))));
            }

            let pubkey_vec = val_bytes.to_vec();
            authorities.push((AuthorityMemberPublicKey(pubkey_vec), mainchain_member));
        }

        Ok(GovernanceAuthorityDatums::R0(
            midnight_primitives_federated_authority_observation::GovernanceAuthorityDatumR0 {
                authorities,
                round,
            },
        ))
    }
}
