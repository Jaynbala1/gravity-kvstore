# README for `gravity_sdk_kvstore`

## Overview

`gravity_sdk_kvstore` is a lightweight, Rust-based key-value store server, designed to emulate certain functionalities of Celestia. It provides three HTTP endpoints (`add_txn`, `get_receipt`, and `get_value`) for network interaction. This document guides you through the compilation, deployment, and usage of the server.

**Note:** This code serves as a minimum viable implementation for demonstrating how to build a DApp using `gravity-sdk`. It does not include account balance validation, comprehensive error handling, or robust runtime fault tolerance. Current limitations and future tasks include:

* **Block Synchronization:** Block synchronization is not yet implemented. A basic Recover API is required for block synchronization functionality.
* **State Persistence:** The server does not load persisted state data on restart, leading to state resets after each restart.
* **Execution Pipeline:** Although the execution layer pipeline is designed with five stages, it currently executes blocks serially instead of in a pipelined manner.

---

## Installation

### Building the Binary

To compile the project, execute the following command after cloning the repository:

```bash
cargo build --release
```

This command generates the optimized executable gravity_sdk_kvstore in the target/release/ directory.

---

## Deployment

### Single-Node Deployment

To deploy and start the server in single-node mode, perform the following steps from the project root:

```bash
 ./deploy_utils/deploy.sh --mode single --node node1 --bin_version release
 /tmp/node1/script/start.sh

```


## Usage

### Network
Interact with the server using the following HTTP endpoints, assuming the server address is 127.0.0.1:9006.

#### add_txn

Submit a transaction to the system via the add_txn endpoint. Upon successful submission, the transaction hash is returned.

```shell
curl -X POST -H "Content-Type: application/json" -d '{
  "unsigned": {
    "nonce": 1,
    "kind": {
      "Transfer": {
        "receiver": "0x1234567890abcdef1234567890abcdef12345678",
        "amount": 100
      }
    }
  },
  "signature": "your_signature_here"
}' http://127.0.0.1:9006/add_txn
```

#### get_receipt

Retrieve the transaction receipt using the transaction hash.

``` bash
curl -X POST -H "Content-Type: application/json" -d '{
  "transaction_hash": "your_transaction_hash_here"
}' http://127.0.0.1:9006/get_receipt
```

#### get_value

Set a key-value pair under an account namespace and retrieve it using the get_value endpoint.

```bash
curl -X POST -H "Content-Type: application/json" -d '[
  "$(openssl rand -hex 20)",
  "key"
]' http://127.0.0.1:9006/get_value
```


### Shell

The application includes an interactive shell for direct interaction. To start the shell, run the binary with the `shell` subcommand:

Once in the shell, you can use the following commands:

- **`user <private_key_hex>`**: Switch the current user context by providing a private key in hexadecimal format.
  ```
  >> user 289c2857d4598e37fb9647507e47a309d6133539bf21a8b9cb6df88fd5232032
  Switched user to: 0x7e5f4552091a69125d5dfcb7b8c2659029395bdf
  ```

- **`set <key> <value>`**: Set a key-value pair for the currently active user. This will create and send a transaction to the mempool.
  ```
  [7e5f...5bdf]>> set mykey myvalue
  Transaction sent! Hash: 28c823812f564f35873111e3c81e28b212d0005d15c2a472c1c6e611802aaf21
  ```

- **`get <key>`**: Retrieve the value associated with a key for the current user.
  ```
  [7e5f...5bdf]>> get mykey
  Value: myvalue
  ```

- **`query_txn <txn_hash>`**: Query the status of a submitted transaction using its hash.
  ```
  [7e5f...5bdf]>> query_txn 28c823812f564f35873111e3c81e28b212d0005d15c2a472c1c6e611802aaf21
  Transaction receipt: Receipt { ... }
  ```

- **`help` or `?`**: Display the list of available commands.

- **`exit`**: Exit the interactive shell.

---
