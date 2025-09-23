pub mod app;
pub mod cli;
pub mod crypto;
pub mod executor;
pub mod state;
pub mod txpool;

pub use crypto::*;
pub use executor::*;
pub use state::*;
pub use txpool::*;

use app::run_tui;
use app::ServerApp;
use clap::Parser;
use cli::Cli;
use gravity_sdk::api::{
    check_bootstrap_config,
    consensus_api::{ConsensusEngine, ConsensusEngineArgs},
};
use gravity_sdk::gaptos::api_types::on_chain_config::validator_set::ValidatorSet;
use gravity_sdk::gaptos::api_types::{
        config_storage::{ConfigStorage, OnChainConfig, OnChainConfigResType},
        on_chain_config::{validator_config::ValidatorConfig, validator_info::ValidatorInfo},
        u256_define::AccountAddress,
    };
use std::{error::Error, fs::File, path::PathBuf, sync::Arc};

pub struct KvOnChainConfig;

impl ConfigStorage for KvOnChainConfig {
    fn fetch_config_bytes(
        &self,
        config_name: OnChainConfig,
        _block_number: u64,
    ) -> Option<OnChainConfigResType> {
        let gravity_validator_set: ValidatorSet = ValidatorSet {
            active_validators: vec![
                ValidatorInfo::new(
                    AccountAddress::from_bytes(hex::decode("2d86b40a1d692c0749a0a0426e2021ee24e2430da0f5bb9c2ae6c586bf3e0a0f").unwrap().as_ref()),
                    1,
                    ValidatorConfig {
                        consensus_public_key: "851d41932d866f5fabed6673898e15473e6a0adcf5033d2c93816c6b115c85ad3451e0bac61d570d5ed9f23e1e7f77c4".as_bytes().to_vec(),
                        validator_network_addresses: bcs::to_bytes("/ip4/127.0.0.1/tcp/2024/noise-ik/2d86b40a1d692c0749a0a0426e2021ee24e2430da0f5bb9c2ae6c586bf3e0a0f/handshake/0").unwrap(),
                        fullnode_network_addresses: bcs::to_bytes("/ip4/127.0.0.1/tcp/2024/noise-ik/2d86b40a1d692c0749a0a0426e2021ee24e2430da0f5bb9c2ae6c586bf3e0a0f/handshake/0").unwrap(),
                        validator_index: 0,
                    }
                )
            ],
            pending_inactive: vec![],
            pending_active: vec![],
            total_voting_power: 1,
            total_joining_power: 1,
        };
        match config_name {
            OnChainConfig::ValidatorSet => Some(OnChainConfigResType::from(bytes::Bytes::from(
                bcs::to_bytes(&gravity_validator_set).unwrap(),
            ))),
            OnChainConfig::ConsensusConfig => {
                let bytes = vec![
                    3, 1, 1, 10, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 10,
                    0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
                ];
                let bytes = bytes::Bytes::from(bytes);
                let res: OnChainConfigResType = bytes.into();
                Some(res)
            }
            OnChainConfig::Epoch => Some(OnChainConfigResType::from(bytes::Bytes::from(
                1u64.to_le_bytes().to_vec(),
            ))),
            _ => None,
        }
    }
}

/// **Note:** This code serves as a minimum viable implementation for demonstrating how to build a DApp using `gravity-sdk`.
/// It does not include account balance validation, comprehensive error handling, or robust runtime fault tolerance.
/// Current limitations and future tasks include:
/// 1. Block Synchronization: Block synchronization is not yet implemented.
/// A basic Recover API implementation is required for block synchronization functionality.
///
/// 2. State Persistence: The server does not load persisted state data on restart,
/// leading to state resets after each restart.
///
/// 3. Execution Pipeline: Although the execution layer pipeline is designed with
/// five stages, it currently executes blocks serially instead of in a pipelined manner.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let log_dir = cli.log_dir.clone();
    let log_dir = PathBuf::from(log_dir);
    let log_file = log_dir.join("kv.log");
    let file = File::create(&log_file)
        .unwrap_or_else(|_| panic!("无法创建日志文件: {}", log_file.display()));

    tracing_subscriber::fmt()
        .with_writer(file)
        .with_ansi(false) // 文件中不使用颜色代码
        .init();
    let gcei_config = check_bootstrap_config(cli.gravity_node_config.node_config_path.clone());
    let storage = Arc::new(SledStorage::new(cli.db_dir.clone())?);
    let genesis_path = cli.genesis_path.clone();
    let blockchain = Blockchain::new(storage.clone(), genesis_path);
    let listen_url = cli.listen_url.clone();
    let state = blockchain.state();
    let mempool = KvStoreTxPool::new();
    let mempool_clone = mempool.clone();
    let state_clone = state.clone();
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        let server = ServerApp::new(state_clone, storage_clone, mempool_clone);
        server.start(listen_url.as_str()).await.unwrap();
    });
    let mempool_clone = mempool.clone();
    let tui_task = tokio::spawn(async move {
        run_tui(state, storage, mempool_clone).await.unwrap();
    });

    let mempool_clone = mempool.clone();
    let _consensus_engine = ConsensusEngine::init(
        ConsensusEngineArgs {
            node_config: gcei_config,
            chain_id: 1337,
            latest_block_number: 0,
            config_storage: Some(Arc::new(KvOnChainConfig)),
        },
        Box::new(mempool_clone),
    )
    .await;

    let blockchain_task = tokio::spawn(async move {
        blockchain.run(mempool).await;
    });

    tokio::select! {
        _ = tui_task => {},
        _ = blockchain_task => {},
    }

    tokio::signal::ctrl_c().await.unwrap();
    Ok(())
}
