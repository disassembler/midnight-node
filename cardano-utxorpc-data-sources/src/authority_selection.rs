// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use authority_selection_inherents::{
    AriadneParameters, AuthoritySelectionDataSource,
};
use derive_new::new;
use sidechain_domain::{CandidateRegistrations, DParameter, EpochNonce, MainchainAddress, McEpochNumber, PolicyId};

/// No-op authority selection data source for federated networks
///
/// In federated mode, there are no SPO registrations from the mainchain.
/// All validator authority comes from the governance process.
#[derive(Clone, new)]
pub struct NoOpAuthoritySelectionDataSource;

#[async_trait]
impl AuthoritySelectionDataSource for NoOpAuthoritySelectionDataSource {
    async fn get_ariadne_parameters(
        &self,
        _epoch: McEpochNumber,
        _d_parameter_policy: PolicyId,
        _permissioned_candidates_policy: PolicyId,
    ) -> Result<AriadneParameters, Box<dyn std::error::Error + Send + Sync>> {
        // For federated networks, return empty Ariadne parameters
        Ok(AriadneParameters {
            d_parameter: DParameter {
                num_permissioned_candidates: 0,
                num_registered_candidates: 0,
            },
            permissioned_candidates: None,
        })
    }

    async fn get_candidates(
        &self,
        _epoch: McEpochNumber,
        _scripts_address: MainchainAddress,
    ) -> Result<Vec<CandidateRegistrations>, Box<dyn std::error::Error + Send + Sync>> {
        // For federated networks, no candidates from mainchain
        Ok(vec![])
    }

    async fn get_epoch_nonce(
        &self,
        _epoch: McEpochNumber,
    ) -> Result<Option<EpochNonce>, Box<dyn std::error::Error + Send + Sync>> {
        // For federated networks, nonce not needed
        Ok(None)
    }

    async fn data_epoch(
        &self,
        epoch: McEpochNumber,
    ) -> Result<McEpochNumber, Box<dyn std::error::Error + Send + Sync>> {
        // Return the same epoch (no offset for federated)
        Ok(epoch)
    }
}
