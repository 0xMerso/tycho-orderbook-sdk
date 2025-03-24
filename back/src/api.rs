use axum::{
    extract::{Json as AxumExJson, Query},
    response::IntoResponse,
    routing::{get, post},
    Extension, Json as AxumJson, Router,
};
use env_logger::Env;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tap2::shd::{
    self,
    data::fmt::{SrzProtocolComponent, SrzToken},
    types::{EnvConfig, ExecutionPayload, ExecutionRequest, Network, OrderbookRequestBody, PairSimulatedOrderbook, ProtoTychoState, SharedTychoStreamState, SyncState},
};

use utoipa::OpenApi;
use utoipa::ToSchema;

use utoipa_swagger_ui::SwaggerUi;

/// OpenAPI documentation for the API.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "TAP-2 Orderbook API",
        version = "1.0.0",
        description = "An Rust Axum API serving different Tycho Streams, providing orderbook and liquidity data for one network",
    ),
    paths(
        version,
        network,
        status,
        tokens,
        components,
        orderbook,
        execute
    ),
    components(
        schemas(APIVersion, Network, Status, SrzToken, SrzProtocolComponent, PairSimulatedOrderbook, ExecutionPayload, ExecutionRequest)
    ),
    tags(
        (name = "API", description = "Endpoints")
    )
)]
struct ApiDoc;

// A simple structure for the API version.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct APIVersion {
    #[schema(example = "0.1.0")]
    pub version: String,
}

// GET / => "Hello, Tycho!"
async fn root() -> impl IntoResponse {
    AxumJson(json!({ "data": "HeLLo Tycho" }))
}

/// Version endpoint: returns the API version.
#[utoipa::path(
    get,
    path = "/version",
    summary = "API version",
    responses(
        (status = 200, description = "API Version", body = APIVersion)
    ),
    tag = (
        "API"
    )
)]
async fn version() -> impl IntoResponse {
    log::info!("👾 API: GET /version");
    AxumJson(APIVersion { version: "0.1.0".into() })
}

// GET /network => Get network object and its configuration
#[utoipa::path(
    get,
    path = "/network",
    summary = "Network configuration",
    responses(
        (status = 200, description = "Network configuration", body = Network)
    ),
    tag = (
        "API"
    )
)]
async fn network(Extension(network): Extension<Network>) -> impl IntoResponse {
    log::info!("👾 API: GET /network on {} network", network.name);
    AxumJson(network)
}

#[derive(Serialize, Deserialize, ToSchema)]
struct Status {
    #[schema(example = "Running")]
    status: String,
    #[schema(example = "22051447")]
    latest: String,
    #[schema(example = "[0xUNI-ETH, 0xUSDC-ETH]")]
    updated: Vec<String>,
}

// GET /status => Get network status + last block synced
#[utoipa::path(
    get,
    path = "/status",
    summary = "API status and latest block synchronized",
    description = "API is 'running' when Redis and Stream are ready. Block updated at each new header after processing state updates",
    responses(
        (status = 200, description = "Current API status and latest block synchronized, along with last block updated components", body = Status)
    ),
    tag = (
        "API"
    )
)]
async fn status(Extension(network): Extension<Network>) -> impl IntoResponse {
    log::info!("👾 API: GET /status on {} network", network.name);
    let key1 = shd::r#static::data::keys::stream::status(network.name.clone());
    let key2 = shd::r#static::data::keys::stream::latest(network.name.clone());
    let key3 = shd::r#static::data::keys::stream::updatedcps(network.name.clone());
    let status = shd::data::redis::get::<u128>(key1.as_str()).await;
    let latest = shd::data::redis::get::<u64>(key2.as_str()).await;
    let updatedcps = shd::data::redis::get::<Vec<String>>(key3.as_str()).await;
    match (status, latest, updatedcps) {
        (Some(status), Some(latest), Some(updatedcps)) => AxumJson(json!(Status {
            status: status.to_string(),
            latest: latest.to_string(),
            updated: updatedcps
        })),
        _ => AxumJson(json!(Status {
            status: SyncState::Error.to_string(),
            latest: "0".to_string(),
            updated: vec![]
        })),
    }
}

async fn _tokens(network: Network) -> Option<Vec<SrzToken>> {
    let key = shd::r#static::data::keys::stream::tokens(network.name.clone());
    shd::data::redis::get::<Vec<SrzToken>>(key.as_str()).await
}

// GET /tokens => Get tokens object from Tycho
#[utoipa::path(
    get,
    path = "/tokens",
    summary = "All Tycho tokens on the network",
    description = "Only quality tokens are listed here (evaluated at 100 by Tycho = no rebasing, etc)",
    responses(
        (status = 200, description = "Tycho Tokens on the network", body = Vec<SrzToken>)
    ),
    tag = (
        "API"
    )
)]
async fn tokens(Extension(network): Extension<Network>) -> impl IntoResponse {
    log::info!("👾 API: GET /tokens on {} network", network.name);
    match _tokens(network.clone()).await {
        Some(tokens) => AxumJson(json!({ "tokens": tokens })),
        _ => AxumJson(json!({ "tokens": [] })),
    }
}

pub async fn _components(network: Network) -> Option<Vec<SrzProtocolComponent>> {
    let key = shd::r#static::data::keys::stream::components(network.name.clone());
    shd::data::redis::get::<Vec<SrzProtocolComponent>>(key.as_str()).await
}

// GET /components => Get all existing components
#[utoipa::path(
    get,
    path = "/components",
    summary = "Tycho components (= liquidity pools)",
    description = "Returns all components available on the network",
    responses(
        (status = 200, description = "Tycho Components (= liquidity pools)", body = SrzProtocolComponent)
    ),
    tag = (
        "API"
    )
)]

async fn components(Extension(network): Extension<Network>) -> impl IntoResponse {
    log::info!("👾 API: GET /components on {} network", network.name);
    match _components(network).await {
        Some(cps) => {
            log::info!("Returning {} components", cps.len());
            AxumJson(json!({ "components": cps }))
        }
        _ => AxumJson(json!({ "components": [] })),
    }
}

// GET /execute => Execute a trade
#[utoipa::path(
    get,
    path = "/execute",
    summary = "Build transaction for a given orderbook point",
    request_body = ExecutionRequest,
    description = "Using Tycho execution engine, build a transaction according to a given orderbook point/distribution",
    responses(
        (status = 200, description = "The trade result", body = ExecutionPayload)
    ),
    tag = ("API")
)]
async fn execute(Extension(network): Extension<Network>, Extension(config): Extension<EnvConfig>, AxumExJson(execution): AxumExJson<ExecutionRequest>) -> impl IntoResponse {
    log::info!("👾 API: Querying execute endpoint: {:?}", execution);
    let response = match shd::core::execute::swap(network.clone(), execution.clone(), config.clone()).await {
        Ok(result) => AxumJson(json!({ "execute": result })),
        Err(e) => AxumJson(json!({ "execute": e.to_string() })),
    };
    response
}

// GET /orderbook/{0xt0-0xt1} => Simulate the orderbook
#[utoipa::path(
    get,
    path = "/orderbook",
    summary = "Orderbook for a given pair of tokens",
    description = "Aggregate liquidity across AMMs, simulates an orderbook (bids/asks). Depending on the number of components (pool having t0 AND t1) and simulation input config, the orderbook can be more or less accurate, and the simulation can take up to severals minutes",
    request_body = OrderbookRequestBody,
    responses(
        (status = 200, description = "Contains trade simulations, results and components", body = PairSimulatedOrderbook)
    ),
    tag = (
        "API"
    )
)]
async fn orderbook(Extension(shtss): Extension<SharedTychoStreamState>, Extension(network): Extension<Network>, AxumExJson(params): AxumExJson<OrderbookRequestBody>) -> impl IntoResponse {
    let single = params.sps.is_some();
    log::info!("👾 API: Querying orderbook endpoint: {:?} | OrderbookRequestBody: {:?} | Single point: {}", params.tag, params, single);
    match (_tokens(network.clone()).await, _components(network.clone()).await) {
        (Some(atks), Some(acps)) => {
            let target = params.tag.clone();
            let tokens = target.split("-").map(|x| x.to_string().to_lowercase()).collect::<Vec<String>>();
            let srzt0 = atks.iter().find(|x| x.address.to_lowercase() == tokens[0].clone());
            let srzt1 = atks.iter().find(|x| x.address.to_lowercase() == tokens[1].clone());
            if srzt0.is_none() {
                log::error!("Couldn't find tokens[0]: {}", tokens[0]);
                return AxumJson(json!({ "orderbook": "Couldn't find tokens for pair tag given" }));
            } else if srzt1.is_none() {
                log::error!("Couldn't find  tokens[1]: {}", tokens[1]);
                return AxumJson(json!({ "orderbook": "Couldn't find tokens for pair tag given" }));
            }
            let srzt0 = srzt0.unwrap();
            let srzt1 = srzt1.unwrap();
            let tokens = vec![srzt0.clone(), srzt1.clone()];
            let mtx = shtss.read().await;
            let balances = mtx.balances.clone();
            drop(mtx);
            let (t0_to_eth_path, t0_to_eth_comps) = shd::maths::path::routing(acps.clone(), srzt0.address.to_string().to_lowercase(), network.eth.to_lowercase()).unwrap_or_default();
            let (t1_to_eth_path, t1_to_eth_comps) = shd::maths::path::routing(acps.clone(), srzt1.address.to_string().to_lowercase(), network.eth.to_lowercase()).unwrap_or_default();
            // log::info!("Path from {} to network.ETH is {:?}", srzt0.symbol, t0_to_eth_path);
            if tokens.len() == 2 {
                let mut ptss: Vec<ProtoTychoState> = vec![];
                let mut to_eth_ptss: Vec<ProtoTychoState> = vec![];
                for cp in acps.clone() {
                    let cptks = cp.tokens.clone();
                    if shd::utils::misc::matchcp(cptks.clone(), tokens.clone()) {
                        let mtx = shtss.read().await;
                        match mtx.protosims.get(&cp.id.to_lowercase()) {
                            Some(protosim) => {
                                ptss.push(ProtoTychoState {
                                    component: cp.clone(),
                                    protosim: protosim.clone(),
                                });
                            }
                            None => {
                                log::error!("Couldn't find protosim for component {}", cp.id);
                            }
                        }
                        drop(mtx);
                    }
                    if t0_to_eth_comps.contains(&cp.id.to_lowercase()) || t1_to_eth_comps.contains(&cp.id.to_lowercase()) {
                        let mtx = shtss.read().await;
                        match mtx.protosims.get(&cp.id.to_lowercase()) {
                            Some(protosim) => {
                                to_eth_ptss.push(ProtoTychoState {
                                    component: cp.clone(),
                                    protosim: protosim.clone(),
                                });
                            }
                            None => {
                                log::error!("Couldn't find protosim for component {}", cp.id);
                            }
                        }
                        drop(mtx);
                    }
                }
                if ptss.is_empty() {
                    return AxumJson(json!({ "orderbook": {} }));
                }
                // Token 0
                let utk0_ethworth = shd::maths::path::quote(to_eth_ptss.clone(), atks.clone(), t0_to_eth_path.clone()).unwrap_or_default();
                let utk1_ethworth = shd::maths::path::quote(to_eth_ptss.clone(), atks.clone(), t1_to_eth_path.clone()).unwrap_or_default();
                log::info!(" - One unit of base token ({}) quoted to ETH = {}", srzt0.symbol, utk0_ethworth);
                log::info!(" - One unit of quote token ({}) quoted to ETH = {}", srzt1.symbol, utk1_ethworth);
                // let ptss = vec![ptss[0].clone()];
                let result = shd::core::orderbook::build(network.clone(), balances.clone(), ptss.clone(), tokens.clone(), params.clone(), utk0_ethworth, utk1_ethworth).await;
                if !single {
                    let path = format!("misc/data-front-v2/orderbook.{}.{}-{}.json", network.name, srzt0.symbol.to_lowercase(), srzt1.symbol.to_lowercase());
                    crate::shd::utils::misc::save1(result.clone(), path.as_str());
                }
                AxumJson(json!({ "orderbook": result.clone() }))
            } else {
                log::error!("Query param Tag must contain only 2 tokens separated by a dash '-'");
                AxumJson(json!({ "orderbook": "Query param Tag must contain only 2 tokens separated by a dash '-'." }))
            }
        }
        _ => {
            log::error!("Couldn't not read internal components");
            AxumJson(json!({ "orderbook": "Couldn't not read internal components" }))
        }
    }
}

pub async fn start(n: Network, shared: SharedTychoStreamState, config: EnvConfig) {
    log::info!("👾 Launching API for '{}' network | 🧪 Testing mode: {:?} | Port: {}", n.name, config.testing, n.port);
    // shd::utils::misc::log::logtest();
    let rstate = shared.read().await;
    log::info!("Testing SharedTychoStreamState read = {:?} with {:?}", rstate.protosims.keys(), rstate.protosims.values());
    log::info!(" => rstate.states.keys and rstate.states.values => {:?} with {:?}", rstate.protosims.keys(), rstate.protosims.values());
    log::info!(" => rstate.components.keys and rstate.components.values => {:?} with {:?}", rstate.components.keys(), rstate.components.values());
    log::info!(" => rstate.balances.keys and rstate.balances.values => {:?} with {:?}", rstate.balances.keys(), rstate.balances.values());
    drop(rstate);

    // Add /api prefix
    let inner = Router::new()
        .route("/", get(root))
        .route("/version", get(version))
        .route("/network", get(network))
        .route("/status", get(status))
        .route("/tokens", get(tokens))
        .route("/components", get(components))
        .route("/orderbook", get(orderbook))
        .route("/execute", get(execute))
        // Swagger
        .layer(Extension(shared.clone())) // Shared state
        .layer(Extension(n.clone()))
        .layer(Extension(config.clone())); // EnvConfig

    let app = Router::new().nest("/api", inner).merge(SwaggerUi::new("/swagger").url("/api-docs/openapi.json", ApiDoc::openapi()));

    match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", n.port)).await {
        Ok(listener) => match axum::serve(listener, app).await {
            Ok(_) => {
                log::info!("(Logs never displayed in theory): API for '{}' network is running on port {}", n.name, n.port);
            }
            Err(e) => {
                log::error!("Failed to start API for '{}' network: {}", n.name, e);
            }
        },
        Err(e) => {
            log::error!("Failed to bind to port {}: {}", n.port, e);
        }
    }
}

// ================================== API Endpoints ==================================
// A pool has an idea and can be an address "0x1234" or a bytes (uniswap v4) "0x96646936b91d6b9d7d0c47c496afbf3d6ec7b6f8000200000000000000000019"
// A pair is "0xToken0-0xToken1" and can have multiple liquidity pool attached to it
// - / => "Hello, Tycho!"
// - /version => Get version of the API 📕
// - /network => Get network object, its configuration 📕
// - /status => Get network status + last block synced 📕
// - /tokens => Get all tokens of the network 📕
// - /pairs => Get all existing pairs in database (vector of strings of token0-token1) + optional FILTER on address 📕
// - /component/:id => Get the component the given pool  📍
// - /state/:id => Get the component the given pool 📍
// - /components/:pair => Get ALL components for 1 pair 📍
