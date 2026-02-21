// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DataSourceError {
    #[error("gRPC connection error: {0}")]
    ConnectionError(#[from] tonic::transport::Error),

    #[error("gRPC status error: {0}")]
    StatusError(#[from] tonic::Status),

    #[error("Chain tip not found")]
    NoTipFound,

    #[error("Block not found for hash: {0}")]
    BlockNotFound(String),

    #[error("Block not found for slot: {0}")]
    BlockNotFoundForSlot(u64),

    #[error("Invalid address format: {0}")]
    InvalidAddress(String),

    #[error("Invalid datum format: {0}")]
    InvalidDatum(String),

    #[error("Datum decoding error: {0}")]
    DatumDecodingError(String),

    #[error("CBOR decoding error: {0}")]
    CborDecodingError(String),

    #[error("Hex decoding error: {0}")]
    HexDecodingError(#[from] hex::FromHexError),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid policy ID: expected 28 bytes, got {0}")]
    InvalidPolicyId(usize),

    #[error("Invalid asset name format")]
    InvalidAssetName,

    #[error("UTxO not found")]
    UtxoNotFound,

    #[error("No governance datum found at script address")]
    NoGovernanceDatum,

    #[error("Invalid Cardano credential: {0}")]
    InvalidCardanoCredential(String),

    #[error("Generic error: {0}")]
    Generic(String),
}

// Note: Don't need explicit From impl - Error trait provides automatic conversion
