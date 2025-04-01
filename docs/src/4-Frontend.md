# Frontend

The public version of tycho-orderbook is limited in its ability to handle all orderbook requests and update them dynamically.
To solve this problem, and to allow more flexibility, we provide an open-source Next JS frontend, and a Rust API with it (Axum).
You can find it in this repository: https://github.com/0xMerso/tycho-orderbook-web

This project provides a website that lets you run your own order book simulations and use your own solver to customize the order book construction algorithm.
The entire system is designed to be modular and can be launched effortlessly using `docker compose`.

### Features

- **Simulations**: Run infinite simulations with a local stream.
- **Solver**s: Integrate your own solver module to build orderbooks.
- **Deployment**: Launch the entire setup with docker.

### Prerequisites

- **Docker Compose**: Ensure both are installed on your machine.
- **Rust**: Required for building and modifying the code (optional).
- **Solver** Custom Solver Implementation: your solver module (optional).

### Docker Setup Instructions

1. **Clone the Repository**

   ```bash
   
   git clone https://github.com/0xMerso/tycho-orderbook-web
   cd tycho-orderbook-web

2. **Docker Compose**

   Make sure Docker is installed and active.

   ```bash
   # Start
   docker compose up --build -d
   docker compose logs -f
   # Stop it
   docker compose stop
   # Remove it
   docker compose down

3. **Load the NextJS front**

   UI: http://localhost:3000/
   API Swagger: http://localhost:42001/swagger

You can use you own WALLET_CONNECT_PROJECT_ID key, used with WalletConnect provider.