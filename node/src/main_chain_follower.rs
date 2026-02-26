// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use authority_selection_inherents::AuthoritySelectionDataSource;
use midnight_primitives_mainchain_follower::CandidatesDataSourceImpl;
use pallet_sidechain_rpc::SidechainRpcDataSource;
use partner_chains_db_sync_data_sources::{
	BlockDataSourceImpl, CachedTokenBridgeDataSourceImpl, DbSyncBlockDataSourceConfig,
	McFollowerMetrics, McHashDataSourceImpl, SidechainRpcDataSourceImpl,
};
use partner_chains_mock_data_sources::{
	AuthoritySelectionDataSourceMock, BlockDataSourceMock, McHashDataSourceMock,
	SidechainRpcDataSourceMock, TokenBridgeDataSourceMock,
};
use sc_service::error::Error as ServiceError;
use sidechain_mc_hash::McHashDataSource;
use sp_governed_map::GovernedMapDataSource;
use sp_partner_chains_bridge::TokenBridgeDataSource;
use sqlx::{Pool, Postgres};

use super::cfg::midnight_cfg::{CardanoBackend, MidnightCfg};
use midnight_primitives::BridgeRecipient;
use partner_chains_mock_data_sources::MockRegistrationsConfig;
use sidechain_domain::mainchain_epoch::{Duration, MainchainEpochConfig, Timestamp};
use std::{error::Error, str::FromStr as _, sync::Arc};

use midnight_primitives_mainchain_follower::{
	CNightObservationDataSourceMock, FederatedAuthorityObservationDataSource,
	FederatedAuthorityObservationDataSourceImpl, FederatedAuthorityObservationDataSourceMock,
	MidnightCNightObservationDataSource, MidnightCNightObservationDataSourceImpl,
};

#[cfg(feature = "utxorpc")]
use cardano_utxorpc_data_sources::{
	NoOpAuthoritySelectionDataSource, UtxoRpcCNightObservationDataSource,
	UtxoRpcClient, UtxoRpcConfig, UtxoRpcFederatedAuthorityDataSource,
	UtxoRpcGovernedMapDataSource, UtxoRpcMcHashDataSource,
	UtxoRpcSidechainRpcDataSource, UtxoRpcTokenBridgeDataSource,
};

// TODO: Decide if it should be experimental
// #[cfg(feature = "experimental")]

#[derive(Clone)]
pub struct DataSources {
	pub mc_hash: Arc<dyn McHashDataSource + Send + Sync>,
	pub authority_selection: Arc<dyn AuthoritySelectionDataSource + Send + Sync>,
	pub cnight_observation: Arc<dyn MidnightCNightObservationDataSource + Send + Sync>,
	pub sidechain_rpc: Arc<dyn SidechainRpcDataSource + Send + Sync>,
	pub governed_map: Arc<dyn GovernedMapDataSource + Send + Sync>,
	pub federated_authority_observation:
		Arc<dyn FederatedAuthorityObservationDataSource + Send + Sync>,
	pub bridge: Arc<dyn TokenBridgeDataSource<BridgeRecipient> + Send + Sync>,
}

#[derive(Clone)]
pub struct DbPoolCfg {
	acquire_timeout: std::time::Duration,
	max_connections: u32,
}

pub(crate) async fn create_cached_main_chain_follower_data_sources(
	cfg: MidnightCfg,
	metrics_opt: Option<McFollowerMetrics>,
) -> std::result::Result<DataSources, ServiceError> {
	// Check for UTxO RPC mode (via config or environment variable)
	#[cfg(feature = "utxorpc")]
	if cfg.cardano_backend == CardanoBackend::Utxorpc
		|| std::env::var("CARDANO_BACKEND").as_deref() == Ok("utxorpc")
	{
		log::info!("Using UTxO RPC data sources");
		return create_utxorpc_data_sources(cfg).await.map_err(|err| {
			ServiceError::Application(
				format!("Failed to create UTxO RPC data sources: {err}").into(),
			)
		});
	}

	if cfg.use_main_chain_follower_mock {
		let mock = create_mock_data_sources(cfg.clone()).await.map_err(|err| {
			ServiceError::Application(
				format!("Failed to create main chain follower mock: {err}. Check configuration.")
					.into(),
			)
		})?;

		Ok(mock)
	} else {
		create_cached_data_sources(cfg, metrics_opt).await.map_err(|err| {
			ServiceError::Application(
				format!("Failed to create db-sync main chain follower: {err}").into(),
			)
		})
	}
}

pub async fn create_mock_data_sources(
	cfg: MidnightCfg,
) -> std::result::Result<DataSources, Box<dyn Error + Send + Sync + 'static>> {
	let block = Arc::new(BlockDataSourceMock::new(cfg.mc_epoch_duration_millis as u32));

	let authority_selection_data_source_mock = AuthoritySelectionDataSourceMock {
		registrations_data: MockRegistrationsConfig::read_registrations(
			&cfg.mock_registrations_file.ok_or(missing("mock_registrations_file"))?,
		)?,
	};

	// TODO: Implement proper GovernedMapDataSourceMock
	unimplemented!("Governed map not supported in mock mode. Use UTxORPC mode instead.")
}

#[cfg(feature = "utxorpc")]
pub async fn create_utxorpc_data_sources(
	cfg: MidnightCfg,
) -> std::result::Result<DataSources, Box<dyn Error + Send + Sync + 'static>> {
	// Get UTxO RPC endpoint from config or environment variable
	let endpoint = cfg
		.utxorpc_endpoint
		.or_else(|| std::env::var("UTXORPC_ENDPOINT").ok())
		.unwrap_or_else(|| "http://localhost:50051".to_string());

	log::info!("Connecting to UTxO RPC endpoint: {}", endpoint);

	// Create UTxO RPC config
	let utxo_rpc_config = UtxoRpcConfig {
		endpoint,
		security_parameter: cfg
			.cardano_security_parameter
			.expect("cardano_security_parameter required for UTxORPC") as u64,
		network_magic: cfg
			.utxorpc_network_magic
			.expect("utxorpc_network_magic required for UTxORPC"),
	};

	// Create shared client
	let client = UtxoRpcClient::new(utxo_rpc_config)
		.await
		.map_err(|e| format!("Failed to connect to UTxO RPC: {}", e))?;

	log::info!("Successfully connected to UTxO RPC server");

	Ok(DataSources {
		mc_hash: Arc::new(UtxoRpcMcHashDataSource::new(client.clone())),
		// For federated networks, use NoOp authority selection (no SPO registrations)
		authority_selection: Arc::new(NoOpAuthoritySelectionDataSource),
		cnight_observation: Arc::new(UtxoRpcCNightObservationDataSource::new(client.clone())),
		sidechain_rpc: Arc::new(UtxoRpcSidechainRpcDataSource::new(client.clone())),
		governed_map: Arc::new(UtxoRpcGovernedMapDataSource::new(client.clone())),
		federated_authority_observation: Arc::new(UtxoRpcFederatedAuthorityDataSource::new(
			client.clone(),
		)),
		bridge: Arc::new(UtxoRpcTokenBridgeDataSource::<BridgeRecipient>::new(client)),
	})
}

pub async fn create_index_if_not_exists(pool: &Pool<Postgres>) {
	// Check if index already exists
	let index_exists: bool = sqlx::query_scalar(
		r#"
			SELECT EXISTS (
				SELECT 1 FROM pg_indexes
				WHERE indexname = 'idx_multi_asset_policy_name_hex'
			)
		"#,
	)
	.fetch_one(pool)
	.await
	.unwrap_or(false);

	if index_exists {
		log::info!("Index idx_multi_asset_policy_name_hex already exists, skipping creation.");
	} else {
		log::info!("Creating idx_multi_asset_policy_name_hex index. This may take a while.");
		let index_query_result = sqlx::query(
			r#"
				CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_multi_asset_policy_name_hex
				ON multi_asset ((encode(policy, 'hex')), (encode(name, 'hex')));
			"#,
		)
		.execute(pool)
		.await;

		if let Err(e) = index_query_result {
			log::warn!(
				"Warning: failed to create idx_multi_asset_policy_name_hex index (is your db-sync readonly?). Performance may be degraded: {e}"
			);
		}
	}
}

pub const CANDIDATES_FOR_EPOCH_CACHE_SIZE: usize = 64;
pub const BRIDGE_TRANSFER_CACHE_LOOKAHEAD: u32 = 1000;

// FIXME: these should almost certainly be Cfg in MidnightCfg, so users can tweak as needed
const CANDIDATES_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 5 };
const SIDECHAIN_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 5 };
const MC_HASH_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 5 };
const CNIGHT_OBSERVATION_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 10 };
const FEDERATED_AUTHORITY_OBSERVATION_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 5 };
const BRIDGE_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 5 };
const ICS_POOL_CFG: DbPoolCfg =
	DbPoolCfg { acquire_timeout: std::time::Duration::from_secs(30), max_connections: 5 };

pub async fn create_cached_data_sources(
	cfg: MidnightCfg,
	metrics_opt: Option<McFollowerMetrics>,
) -> Result<DataSources, Box<dyn Error + Send + Sync + 'static>> {
	let postgres_uri = &cfg
		.db_sync_postgres_connection_string
		.ok_or(missing("db_sync_postgres_connection_string"))?;

	let db_sync_block_data_source_config = DbSyncBlockDataSourceConfig {
		cardano_security_parameter: cfg
			.cardano_security_parameter
			.ok_or(missing("cardano_security_parameter"))?,
		cardano_active_slots_coeff: cfg
			.cardano_active_slots_coeff
			.ok_or(missing("cardano_active_slots_coeff"))?,
		block_stability_margin: cfg
			.block_stability_margin
			.ok_or(missing("block_stability_margin"))?,
	};

	let mc = MainchainEpochConfig {
		first_epoch_timestamp_millis: Timestamp::from_unix_millis(
			cfg.mc_first_epoch_timestamp_millis,
		),
		epoch_duration_millis: Duration::from_millis(cfg.mc_epoch_duration_millis),
		first_epoch_number: cfg.mc_first_epoch_number,
		first_slot_number: cfg.mc_first_slot_number,
		slot_duration_millis: Duration::from_millis(cfg.mc_slot_duration_millis),
	};

	let candidates_pool =
		get_connection(postgres_uri, CANDIDATES_POOL_CFG, cfg.allow_non_ssl).await?;

	// All these pools are connections to the same database, so we can use any pool to create the index
	create_index_if_not_exists(&candidates_pool).await;

	let candidates_data_source =
		CandidatesDataSourceImpl::new(candidates_pool, metrics_opt.clone()).await?;
	let candidates_data_source_cached =
		candidates_data_source.cached(CANDIDATES_FOR_EPOCH_CACHE_SIZE)?;

	let sidechain_pool =
		get_connection(postgres_uri, SIDECHAIN_POOL_CFG, cfg.allow_non_ssl).await?;
	let sidechain_block_data_source = Arc::new(BlockDataSourceImpl::from_config(
		sidechain_pool,
		db_sync_block_data_source_config.clone(),
		&mc,
	));
	let sidechain_rpc =
		SidechainRpcDataSourceImpl::new(sidechain_block_data_source.clone(), metrics_opt.clone());

	let mc_hash_pool = get_connection(postgres_uri, MC_HASH_POOL_CFG, cfg.allow_non_ssl).await?;
	let mc_hash_block_data_source = BlockDataSourceImpl::from_config(
		mc_hash_pool,
		db_sync_block_data_source_config.clone(),
		&mc,
	);
	let mc_hash =
		McHashDataSourceImpl::new(Arc::new(mc_hash_block_data_source), metrics_opt.clone());

	let cnight_observation_pool =
		get_connection(postgres_uri, CNIGHT_OBSERVATION_POOL_CFG, cfg.allow_non_ssl).await?;
	let cnight_observation = MidnightCNightObservationDataSourceImpl::new(
		cnight_observation_pool,
		metrics_opt.clone(),
		1000,
	);

	let federated_authority_observation_pool =
		get_connection(postgres_uri, FEDERATED_AUTHORITY_OBSERVATION_POOL_CFG, cfg.allow_non_ssl)
			.await?;
	let federated_authority_observation = FederatedAuthorityObservationDataSourceImpl::new(
		federated_authority_observation_pool,
		metrics_opt.clone(),
		1000,
	);

	let bridge_pool = get_connection(postgres_uri, BRIDGE_POOL_CFG, cfg.allow_non_ssl).await?;

	let bridge = CachedTokenBridgeDataSourceImpl::new(
		bridge_pool,
		metrics_opt,
		sidechain_block_data_source,
		BRIDGE_TRANSFER_CACHE_LOOKAHEAD,
	);

	// TODO: Implement GovernedMapDataSource for db-sync
	unimplemented!("Governed map not supported in db-sync mode. Use UTxORPC mode instead.")
}

// Helper for users who only need native token observation data source
pub async fn create_cnight_observation_data_source(
	cfg: MidnightCfg,
	metrics_opt: Option<McFollowerMetrics>,
) -> Result<Arc<dyn MidnightCNightObservationDataSource>, Box<dyn Error + Send + Sync + 'static>> {
	let pool = get_connection(
		&cfg.db_sync_postgres_connection_string
			.ok_or(missing("db_sync_postgres_connection_string"))?,
		CNIGHT_OBSERVATION_POOL_CFG,
		cfg.allow_non_ssl,
	)
	.await?;

	Ok(Arc::new(MidnightCNightObservationDataSourceImpl::new(pool, metrics_opt.clone(), 1000)))
}

pub async fn create_federated_authority_observation_data_source(
	cfg: MidnightCfg,
	metrics_opt: Option<McFollowerMetrics>,
) -> Result<Arc<dyn FederatedAuthorityObservationDataSource>, Box<dyn Error + Send + Sync + 'static>>
{
	let pool = get_connection(
		&cfg.db_sync_postgres_connection_string
			.ok_or(missing("db_sync_postgres_connection_string"))?,
		FEDERATED_AUTHORITY_OBSERVATION_POOL_CFG,
		cfg.allow_non_ssl,
	)
	.await?;

	Ok(Arc::new(FederatedAuthorityObservationDataSourceImpl::new(pool, metrics_opt.clone(), 1000)))
}

pub async fn create_authority_selection_data_source(
	cfg: MidnightCfg,
	metrics_opt: Option<McFollowerMetrics>,
) -> Result<
	Arc<dyn AuthoritySelectionDataSource + Send + Sync>,
	Box<dyn Error + Send + Sync + 'static>,
> {
	let (data_source, _pool) =
		create_authority_selection_data_source_with_pool(cfg, metrics_opt).await?;
	Ok(data_source)
}

pub async fn create_authority_selection_data_source_with_pool(
	cfg: MidnightCfg,
	metrics_opt: Option<McFollowerMetrics>,
) -> Result<
	(Arc<dyn AuthoritySelectionDataSource + Send + Sync>, sqlx::PgPool),
	Box<dyn Error + Send + Sync + 'static>,
> {
	let pool = get_connection(
		&cfg.db_sync_postgres_connection_string
			.ok_or(missing("db_sync_postgres_connection_string"))?,
		CANDIDATES_POOL_CFG,
		cfg.allow_non_ssl,
	)
	.await?;

	let candidates_data_source =
		CandidatesDataSourceImpl::new(pool.clone(), metrics_opt.clone()).await?;
	let candidates_data_source_cached =
		candidates_data_source.cached(CANDIDATES_FOR_EPOCH_CACHE_SIZE)?;

	Ok((Arc::new(candidates_data_source_cached), pool))
}

/// Create a database pool for ICS genesis queries
pub async fn create_ics_genesis_pool(
	cfg: MidnightCfg,
) -> Result<sqlx::PgPool, Box<dyn Error + Send + Sync + 'static>> {
	let pool = get_connection(
		&cfg.db_sync_postgres_connection_string
			.ok_or(missing("db_sync_postgres_connection_string"))?,
		ICS_POOL_CFG,
		cfg.allow_non_ssl,
	)
	.await?;
	Ok(pool)
}

// Copied from internal utility in partner-chains-db-sync-data-sources
async fn get_connection(
	connection_string: &str,
	pool_cfg: DbPoolCfg,
	allow_non_ssl: bool,
) -> Result<sqlx::PgPool, Box<dyn Error + Send + Sync + 'static>> {
	let connect_options =
		sqlx::postgres::PgConnectOptions::from_str(connection_string)?.ssl_mode(if allow_non_ssl {
			//Note: PgSslMode::Prefer has issues with some environments.
			sqlx::postgres::PgSslMode::Disable
		} else {
			sqlx::postgres::PgSslMode::Require
		});

	let pool = sqlx::postgres::PgPoolOptions::new()
		.max_connections(pool_cfg.max_connections)
		.acquire_timeout(pool_cfg.acquire_timeout)
		.connect_with(connect_options.clone())
		.await
		.map_err(|e| {
			PostgresConnectionError(
				connect_options.get_host().to_string(),
				connect_options.get_port(),
				connect_options.get_database().unwrap_or("cexplorer").to_string(),
				e.to_string(),
			)
			.to_string()
		})?;
	Ok(pool)
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("Could not connect to database: postgres://***:***@{0}:{1}/{2}; error: {3}")]
struct PostgresConnectionError(String, u16, String, String);

fn missing(field: &str) -> sc_service::Error {
	ServiceError::Application(format!("Missing {field}. Check configuration.").into())
}
