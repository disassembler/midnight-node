// This file is part of midnight-node.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)  // We only need the client
        .build_client(true)
        .compile_protos(
            &[
                "proto/utxorpc/query.proto",
                "proto/utxorpc/watch.proto",
                "proto/utxorpc/submit.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
