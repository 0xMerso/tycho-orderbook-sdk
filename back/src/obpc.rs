// client.rs

use std::{collections::HashMap, sync::Arc};
use tap2::shd::obp::{OBPConfig, OBPEvent, OBP};
use tap2::shd::types::{SharedTychoStreamState, TychoStreamState};
use tokio::sync::RwLock;
use tycho_core::models::Chain;

use std::str::FromStr;

use futures::StreamExt;
use tap2::shd;
use tap2::shd::data::fmt::SrzEVMPoolState;
use tap2::shd::data::fmt::SrzProtocolComponent;
use tap2::shd::data::fmt::SrzToken;
use tap2::shd::data::fmt::SrzUniswapV2State;
use tap2::shd::data::fmt::SrzUniswapV3State;
use tap2::shd::data::fmt::SrzUniswapV4State;
use tap2::shd::r#static::data::keys;
use tap2::shd::types::AmmType;
use tap2::shd::types::EnvConfig;
use tap2::shd::types::Network;
use tap2::shd::types::SyncState;
use tap2::shd::types::TychoSupportedProtocol;
use tycho_simulation::evm::protocol::filters::curve_pool_filter;
use tycho_simulation::evm::protocol::filters::uniswap_v4_pool_with_hook_filter;
use tycho_simulation::evm::protocol::uniswap_v3::state::UniswapV3State;
use tycho_simulation::evm::protocol::uniswap_v4::state::UniswapV4State;

use tycho_simulation::models::Token;
use tycho_simulation::tycho_core::Bytes;
use tycho_simulation::{
    evm::{
        engine_db::tycho_db::PreCachedDB,
        protocol::{filters::balancer_pool_filter, uniswap_v2::state::UniswapV2State, vm::state::EVMPoolState},
        stream::ProtocolStreamBuilder,
    },
    tycho_client::feed::component_tracker::ComponentFilter,
};

use tycho_simulation::protocol::models::ProtocolComponent;

pub async fn prebuild(network: Network, config: EnvConfig) -> ProtocolStreamBuilder {
    let (_, _, chain) = shd::types::chain(network.name.clone()).expect("Invalid chain");
    let u4 = uniswap_v4_pool_with_hook_filter;
    let balancer = balancer_pool_filter;
    let curve = curve_pool_filter;
    let filter = ComponentFilter::with_tvl_range(1.0, 500.0); // ! Important. 250 ETH minimum
    let tokens = shd::core::client::tokens(&network, &config).await.unwrap();
    let mut hmt = HashMap::new();
    tokens.iter().for_each(|t| {
        hmt.insert(t.address.clone(), t.clone());
    });
    log::info!("Prebuild. Got {} tokens", hmt.len());
    let mut psb = ProtocolStreamBuilder::new(&network.tycho, chain)
        .exchange::<UniswapV2State>(TychoSupportedProtocol::UniswapV2.to_string().as_str(), filter.clone(), None)
        .exchange::<UniswapV3State>(TychoSupportedProtocol::UniswapV3.to_string().as_str(), filter.clone(), None)
        .exchange::<UniswapV4State>(TychoSupportedProtocol::UniswapV4.to_string().as_str(), filter.clone(), Some(u4))
        .auth_key(Some(config.tycho_api_key.clone()))
        .skip_state_decode_failures(true)
        .set_tokens(hmt.clone()) // ALL Tokens
        .await;

    if network.name.as_str() == "ethereum" {
        log::info!("Prebuild. Adding mainnet-specific exchanges");
        psb = psb
            .exchange::<UniswapV2State>(TychoSupportedProtocol::Sushiswap.to_string().as_str(), filter.clone(), None)
            .exchange::<UniswapV2State>(TychoSupportedProtocol::Pancakeswap.to_string().as_str(), filter.clone(), None)
            .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::BalancerV2.to_string().as_str(), filter.clone(), Some(balancer))
            .exchange::<EVMPoolState<PreCachedDB>>(TychoSupportedProtocol::Curve.to_string().as_str(), filter.clone(), Some(curve));
    }
    psb
}

#[tokio::main]
async fn main() {
    shd::utils::misc::log::new("obpc".to_string());
    dotenv::from_filename(".env.prod").ok(); // Use .env.ex for testing
    let config = EnvConfig::new();
    log::info!("Launching OBP Client on {} | 🧪 Testing mode: {:?}", config.network, config.testing);
    let path = "src/shd/config/networks.json".to_string();
    let networks: Vec<Network> = shd::utils::misc::read(&path);
    let network = networks.clone().into_iter().filter(|x| x.enabled).find(|x| x.name == config.network).expect("Network not found or not enabled");
    log::info!("Tycho Stream for '{}' network", network.name.clone());
    // Create shared state for the protocol stream
    let shared_state: SharedTychoStreamState = Arc::new(RwLock::new(TychoStreamState {
        protosims: HashMap::new(),  // Customize with your actual types
        components: HashMap::new(), // Customize with your actual types
        initialised: false,
    }));
    // Create the OBP provider from the protocol stream builder and shared state.
    let psb = prebuild(network.clone(), config.clone()).await;
    let obp = OBP::new(psb, OBPConfig::default(), shared_state).await.expect("Failed to build OBP");
    let mut stream = obp.stream;
    // Loop indefinitely over the stream, printing received events.
    while let Some(event) = stream.recv().await {
        match event {
            // OBPEvent::BlockUpdate(update) => {
            //     println!("Received block update: {:?}", update);
            // }
            OBPEvent::BlockNumber(number) => {
                log::info!("OBPEvent: Received block number: {:?}", number);
            }
            OBPEvent::Error(err) => {
                log::error!("OBPEvent: Error: {:?}", err);
            }
        }
    }
}
