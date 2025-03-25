use reqwest::Client;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

/// Wrapped Orderbook Provider (OBP) is a wrapper around a stream provider.
/// When instantiated via `new()`, it spawns a task that pings a given endpoint
/// every second with a payload. The resulting status messages are sent over an
/// asynchronous channel so consumers can log the streamed data.
pub struct OBP {
    pub endpoint: String,
    // The spawned task handle is kept to ensure the task remains running.
    _handle: JoinHandle<()>,
    /// Receiver side of the channel where stream messages are sent.
    pub stream: mpsc::Receiver<String>,
}

impl OBP {
    /// Creates a new `OBP` instance.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The local HTTP endpoint URL to ping.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// let provider = OBP::new("http://127.0.0.1:4444/ping".to_string());
    /// ```
    pub fn new(endpoint: String) -> Self {
        let client = Client::new();
        let endpoint_clone = endpoint.clone();
        // Create a channel to stream messages to the consumer.
        let (tx, rx) = mpsc::channel(100);
        // Spawn an asynchronous task that pings the endpoint every second.
        let handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;
                let payload = "orderbook update";
                // let message = match client.post(&endpoint_clone).body(payload.to_string()).send().await {
                //     Ok(response) => format!("Pinged {}: Response status: {}", endpoint_clone, response.status()),
                //     Err(e) => format!("Error pinging {}: {:?}", endpoint_clone, e),
                // };
                // Send the message to the consumer. If the channel is closed, ignore the error.
                // let _ = tx.send(message).await;
                let _ = tx.send("Gm".to_string()).await;
            }
        });

        Self { endpoint, _handle: handle, stream: rx }
    }
}
