// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::error::DataSourceError;
use sidechain_domain::{McBlockHash, McBlockNumber, McTxHash};

/// Helper to convert block hash bytes to McBlockHash
pub fn bytes_to_mc_block_hash(bytes: &[u8]) -> Result<McBlockHash, DataSourceError> {
    if bytes.len() != 32 {
        return Err(DataSourceError::Generic(format!(
            "Invalid block hash length: expected 32, got {}",
            bytes.len()
        )));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(bytes);
    Ok(McBlockHash(hash))
}

/// Helper to convert tx hash bytes to McTxHash
pub fn bytes_to_mc_tx_hash(bytes: &[u8]) -> Result<McTxHash, DataSourceError> {
    if bytes.len() != 32 {
        return Err(DataSourceError::Generic(format!(
            "Invalid tx hash length: expected 32, got {}",
            bytes.len()
        )));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(bytes);
    Ok(McTxHash(hash))
}

/// Helper to convert slot to block number (simplified - may need adjustment)
pub fn slot_to_block_number(slot: u64) -> McBlockNumber {
    McBlockNumber(slot as u32)
}

/// Decode Cardano address bytes to hex string
pub fn address_bytes_to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Encode hex string to address bytes
pub fn hex_to_address_bytes(hex_str: &str) -> Result<Vec<u8>, DataSourceError> {
    hex::decode(hex_str).map_err(DataSourceError::HexDecodingError)
}
