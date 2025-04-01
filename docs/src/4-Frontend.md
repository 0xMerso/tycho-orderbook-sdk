# Frontend

The public version of tycho-orderbook is limited in its ability to handle all orderbook requests and update them dynamically.
To solve this, and to allow more flexibility, we provide an open-source Next JS frontend, and a Rust API with it (Axum).
You can find it in this repository: https://github.com/0xMerso/tycho-orderbook-web

**tycho-orderbook-web** has a NextJS frontend and a Rust backend. The public website has not yet been launched.

By customizing the backend, you can run your own orderbook simulations and use your own solver to customize the orderbook construction algorithm.
The application is designed to be modular and can be launched effortlessly using `docker compose`.

### Features

- **Simulations**: Run infinite simulations with a local stream.
- **Solvers**: Integrate your own solver module to build orderbooks.
- **Deployment**: Launch the entire setup with docker compose.

### Prerequisites

- **Docker Compose**: Ensure both are installed on your machine.
- **Rust**: Required for building and modifying the code (optional).
- **Solver** Custom Solver Implementation: your solver module (optional).

### Clone the Repository

   The repo has the front part as a submodule, so to clone it, do :
   ```bash
   git clone --recurse-submodules tycho-orderbook-web
   cd tycho-orderbook-web
   ```

### Docker Setup Instructions

Make sure Docker is installed and active. Run the application in one command :

   ```bash
   # Start
   docker compose up --build -d
   docker compose logs -f
   # Stop it
   docker compose stop
   # Remove it
   docker compose down
   ```

Then, access to:
- UI: http://localhost:3000/
- Swagger: http://localhost:42001/swagger

You can use you own WALLET_CONNECT_PROJECT_ID env variable in the .env file at the root of the *front* folder, used with the package WalletConnect.

### Local Setup Instructions

If you prefer to build and run the application directly, we provide shell scripts for simple startup.
You will need to install **Rust, Node, Redis and pnpm.**

   ```bash
   cd back
   # Launch 'ethereum' Axum API + Redis. You can use 'base' instead
   sh ops/local.api.start.sh ethereum
   ```

   ```bash
   cd front
   pnpm install
   pnpm dev
   ```