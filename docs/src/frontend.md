# Frontend

The public version of tycho-orderbook is limited in its ability to handle all orderbook requests and update them dynamically.
To solve this problem, and to allow more flexibility, we provide an open-source Next JS frontend.
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
   git clone <repository-url>
   cd <repository-directory>


