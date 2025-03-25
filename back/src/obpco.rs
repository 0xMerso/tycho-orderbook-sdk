use tap2::shd::{self, obp::OBP};

#[tokio::main]
async fn main() {
    shd::utils::misc::log::new("obpco".to_string());
    // Instantiate the Wrapped Orderbook Provider with your local endpoint.
    let provider = OBP::new("http://tycho-beta.propellerheads.xyz".to_string());
    log::info!("OBP provider started with endpoint: {}", provider.endpoint);
    let mut stream = provider.stream;
    // Log each message that is streamed by the OBP provider.
    while let Some(message) = stream.recv().await {
        log::info!("Received stream message: {}", message);
    }
}
