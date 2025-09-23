use crate::{
    crypto::{self, KeyPair},
    KvStoreTxPool, State, Storage, Transaction, TransactionKind, TransactionWithAccount,
    UnsignedTransaction,
};
use bytes::buf::Reader;
use rustyline::{error::ReadlineError, DefaultEditor};
use rustyline::Editor;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use std::{fs::File, io::BufReader, sync::Arc};
use tokio::sync::RwLock;

pub struct Shell {
    state: Arc<RwLock<State>>,
    storage: Arc<dyn Storage>,
    mempool: KvStoreTxPool,
    keypair: Option<KeyPair>,
}

impl Shell {
    pub fn new(
        state: Arc<RwLock<State>>,
        storage: Arc<dyn Storage>,
        mempool: KvStoreTxPool,
    ) -> Self {
        Self {
            state,
            storage,
            mempool,
            keypair: None,
        }
    }

    pub async fn run(&mut self) {
        let mut rl = DefaultEditor::new().unwrap();
        if rl.load_history("history.txt").is_err() {
            println!("No previous history.");
        }

        loop {
            let prompt = if let Some(keypair) = &self.keypair {
                let address = crypto::public_key_to_address(&keypair.public_key);
                let address_str = format!("{}", address);
                let short_address = if address_str.len() > 10 {
                    format!(
                        "{}...{}",
                        &address_str[..6],
                        &address_str[address_str.len() - 4..]
                    )
                } else {
                    address_str
                };
                format!("[{}]>> ", short_address)
            } else {
                ">> ".to_string()
            };
            let readline = rl.readline(&prompt);
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str()).unwrap();
                    let args: Vec<&str> = line.trim().split_whitespace().collect();
                    if args.is_empty() {
                        continue;
                    }
                    self.handle_command(args).await;
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }
    }

    async fn handle_command(&mut self, args: Vec<&str>) {
        match args[0] {
            "user" => self.handle_user_command(args).await,
            "set" => self.handle_set_command(args).await,
            "get" => self.handle_get_command(args).await,
            "query_txn" => self.handle_query_txn_command(args).await,
            "help" => self.print_help(),
            "?" => self.print_help(),
            "exit" => {
                println!("Exiting.");
                std::process::exit(0);
            }
            _ => {
                println!("Unknown command: {}", args[0]);
                self.print_help();
            }
        }
    }

    async fn handle_user_command(&mut self, args: Vec<&str>) {
        if args.len() < 2 {
            println!("Usage: user <private_key_hex>");
            return;
        }

        let private_key_hex = args[1];
        let private_key_bytes = match hex::decode(private_key_hex) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("Error: Invalid private key hex: {}", e);
                return;
            }
        };

        let secret_key = match SecretKey::from_slice(&private_key_bytes) {
            Ok(sk) => sk,
            Err(e) => {
                println!("Error: Invalid private key: {}", e);
                return;
            }
        };

        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);

        self.keypair = Some(KeyPair {
            secret_key,
            public_key,
        });

        let address = crypto::public_key_to_address(&public_key);
        println!("Switched user to: {}", address);
    }

    async fn handle_set_command(&mut self, args: Vec<&str>) {
        if args.len() < 3 {
            println!("Usage: set <key> <value>");
            return;
        }

        let key = args[1].to_string();
        let value = args[2].to_string();

        let keypair = match &self.keypair {
            Some(kp) => kp,
            None => {
                println!("Error: No user context. Please use 'user <private_key>' to set a user.");
                return;
            }
        };

        let address = crypto::public_key_to_address(&keypair.public_key);

        let unsigned_transaction = UnsignedTransaction {
            nonce: self
                .state
                .read()
                .await
                .get_account(&address)
                .map(|s| s.nonce)
                .unwrap_or(0),
            kind: TransactionKind::SetKV { key, value },
        };

        let signature = crypto::sign_transaction(&unsigned_transaction, &keypair.secret_key);

        let transaction = Transaction {
            unsigned: unsigned_transaction,
            signature,
        };

        let txn_with_account = TransactionWithAccount {
            txn: transaction,
            address,
        };

        let txn_hash = self.mempool.add_raw_txn(txn_with_account);
        println!("Transaction sent! Hash: {}", hex::encode(txn_hash.0));
    }

    async fn handle_get_command(&mut self, args: Vec<&str>) {
        if args.len() < 2 {
            println!("Usage: get <key>");
            return;
        }
        let key = args[1];

        let keypair = match &self.keypair {
            Some(kp) => kp,
            None => {
                println!("Error: No user context. Please use 'user <private_key>' to set a user.");
                return;
            }
        };
        let address = crypto::public_key_to_address(&keypair.public_key);

        match self.state.read().await.get_account(&address) {
            Some(account) => match account.kv_store.get(key) {
                Some(value) => println!("Value: {}", value),
                None => println!("Error: Key not found '{}' for account {}", key, address),
            },
            None => println!("Error: Account not found {}", address),
        }
    }

    async fn handle_query_txn_command(&self, args: Vec<&str>) {
        if args.len() < 2 {
            println!("Usage: query_txn <txn_hash>");
            return;
        }
        let res = hex::decode(args[1]);
        if res.is_err() {
            println!("Error: Invalid transaction hash: {}", res.err().unwrap());
            return;
        }
        let mut txn_hash = [0u8; 32];
        txn_hash.copy_from_slice(res.unwrap().as_slice());
        let res = self.storage.get_transaction_receipt(txn_hash).await;
        match res {
            Ok(Some(receipt)) => println!("Transaction receipt: {:?}", receipt),
            Ok(None) => println!("Transaction receipt not found"),
            Err(e) => println!("Error: {}", e),
        }
    }

    fn print_help(&self) {
        println!("Available commands:");
        println!("  user <private_key_hex>   - Switch user context by providing a private key.");
        println!("  set <key> <value>        - Set a key-value pair for the current user.");
        println!("  get <key>                - Get a value for a key for the current user.");
        println!("  query_txn <txn_hash>     - Query the status of a transaction (not implemented yet).");
        println!("  help                     - Show this help message.");
        println!("  exit                     - Exit the shell.");
    }
}
