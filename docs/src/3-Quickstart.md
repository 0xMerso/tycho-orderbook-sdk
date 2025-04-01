# Quickstart

The quickstart program is an example usage of the tycho-orderbook crate.
It opens a Tycho stream, listen to it, build or update orderbooks each time states has changed, and prints out the formatted orderbooks.

### Terminal

Clone the tycho-orderbook repository, where you'll find the quickstart code.
The .env.ex file provides the default environment variables needed to launch the quickstart program.

### Local Setup Instructions

You must have Rust installed.

To run the program:

    sh examples/quickstart.sh ethereum

If activated, the logs provided a genered information of what's happening under the hood.

The script  `ops/local.api.start.sh`  can be used to launch the API.
The script `ops/api.test.sh`  can be used to test the API without using the frontend.
