use tap2::shd::{self, wop::WOP};

#[tokio::main]
async fn main() {
    shd::utils::misc::log::new("cowop".to_string());
    // Instantiate the Wrapped Orderbook Provider with your local endpoint.
    let provider = WOP::new("http://tycho-beta.propellerheads.xyz".to_string());
    log::info!("WOP provider started with endpoint: {}", provider.endpoint);
    let mut stream = provider.stream;
    // Log each message that is streamed by the WOP provider.
    while let Some(message) = stream.recv().await {
        println!("Received stream message: {}", message);
    }
}
