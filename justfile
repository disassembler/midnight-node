# Justfile for Midnight Node
# This Justfile is used to define tasks for building, testing, and running the Midnight Node.

# List available recipes
default:
  @just --list

# Run hardfork end-to-end test
hardfork-e2e NODE_IMAGE UPGRADER_IMAGE:
  @scripts/tests/hardfork-e2e.sh {{NODE_IMAGE}} {{UPGRADER_IMAGE}}
  @echo "✅ Hardfork E2E test completed successfully."

# Run ledger rollback end-to-end test
ledger-rollback-e2e NODE_IMAGE UPGRADER_IMAGE:
  @scripts/tests/ledger-rollback-e2e.sh {{NODE_IMAGE}} {{UPGRADER_IMAGE}}
  @echo "✅ Ledger rollback E2E test completed successfully."

# Run node end-to-end test
node-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/node-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Node E2E test completed successfully."

# Run toolkit update ledger parameters end-to-end test
toolkit-update-ledger-parameters-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-update-ledger-parameters-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit Update Ledger Parameters E2E test completed successfully."

# Run toolkit end-to-end test
toolkit-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit E2E test completed successfully."

# Run toolkit maintenance end-to-end test
toolkit-maintenance-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-maintenance-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit Maintenance E2E test completed successfully."

# Run toolkit contracts end-to-end test
toolkit-contracts-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-contracts-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit Contracts E2E test completed successfully."

# Run toolkit mint end-to-end test
toolkit-mint-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-mint-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit Mint E2E test completed successfully."

# Run toolkit tokens minter end-to-end test
toolkit-tokens-minter-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-tokens-minter-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit Tokens Minter E2E test completed successfully."

# Run toolkit multi-destination URL end-to-end test
toolkit-multi-dest-e2e TOOLKIT_IMAGE:
  @scripts/tests/toolkit-multi-dest-e2e.sh {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit Multi-Destination URL E2E test completed successfully."

# Run toolkit unshielded token end-to-end test
toolkit-ut-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/toolkit-ut-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Toolkit UnshieldedToken E2E test completed successfully."

# Run startup end-to-end test in dev mode
startup-dev-e2e NODE_IMAGE:
  @scripts/tests/startup-dev-e2e.sh {{NODE_IMAGE}}
  @echo "✅ Startup E2E test in dev mode completed successfully."

# Run startup end-to-end test in qanet mode
startup-qanet-e2e NODE_IMAGE:
  @scripts/tests/startup-qanet-e2e.sh {{NODE_IMAGE}}
  @echo "✅ Startup E2E test in qanet mode completed successfully."

# Run genesis wallet end-to-end test on undeployed network
genesis-wallets-undeployed-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/genesis-wallets-undeployed-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Genesis wallet E2E test in undeployed network completed successfully."

# Run genesis wallet end-to-end test on devnet
genesis-wallets-devnet-e2e NODE_IMAGE TOOLKIT_IMAGE:
  @scripts/tests/genesis-wallets-devnet-e2e.sh {{NODE_IMAGE}} {{TOOLKIT_IMAGE}}
  @echo "✅ Genesis wallet E2E test in devnet network completed successfully."

# Run indexer GraphQL API end-to-end test
indexer-api-e2e:
  @scripts/tests/indexer-api-e2e.sh
  @echo "✅ Indexer GraphQL API E2E test completed successfully."
