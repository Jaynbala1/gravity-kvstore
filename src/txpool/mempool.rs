use gravity_sdk::block_buffer_manager::TxPool;
use gravity_sdk::gaptos::api_types::account::ExternalAccountAddress;
use gravity_sdk::gaptos::api_types::u256_define::TxnHash;
use gravity_sdk::gaptos::api_types::VerifiedTxn;
use gravity_sdk::gaptos::aptos_logger::info;
use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::Arc;
use tracing::warn;

use crate::{compute_transaction_hash, Transaction, TransactionWithAccount};

#[derive(Clone, Debug, PartialEq)]
pub enum TxnStatus {
    Pending,
    Waiting,
}

#[derive(Clone, Debug)]
pub struct MempoolTxn {
    raw_txn: TransactionWithAccount,
    status: TxnStatus,
}

#[derive(Clone)]
pub struct KvStoreTxPool {
    mempool: Arc<MempoolInner>,
}

impl KvStoreTxPool {
    pub fn new() -> Self {
        KvStoreTxPool {
            mempool: MempoolInner::new(),
        }
    }

    pub fn add_verified_txn(&self, txn: VerifiedTxn) -> TxnHash {
        self.mempool.add_verified_txn(txn)
    }

    pub fn add_raw_txn(&self, raw_txn: TransactionWithAccount) -> TxnHash {
        self.mempool.add_raw_txn(raw_txn)
    }

    pub fn remove_txn(&self, sender: &ExternalAccountAddress, seq: u64) {
        self.mempool.remove_txn(sender, seq)
    }
}

struct MempoolInner {
    water_mark: std::sync::Mutex<HashMap<ExternalAccountAddress, u64>>, // next pending sequence number
    mempool: std::sync::Mutex<HashMap<ExternalAccountAddress, BTreeMap<u64, MempoolTxn>>>,
}

impl MempoolInner {
    fn new() -> Arc<Self> {
        Arc::new(MempoolInner {
            water_mark: std::sync::Mutex::new(HashMap::new()),
            mempool: std::sync::Mutex::new(HashMap::new()),
        })
    }

    pub fn remove_txn(&self, sender: &ExternalAccountAddress, seq: u64) {
        let mut pool = self.mempool.lock().unwrap();
        match pool.get_mut(sender) {
            Some(sender_txns) => {
                sender_txns.remove(&seq);
            }
            None => {
                warn!("might be follower");
            }
        }
    }

    pub fn add_verified_txn(&self, txn: VerifiedTxn) -> TxnHash {
        let account = txn.sender().clone();
        let sequence_number = txn.seq_number();
        let status = TxnStatus::Waiting;
        let mempool_txn = MempoolTxn {
            raw_txn: txn.into(),
            status,
        };
        self.mempool
            .lock()
            .unwrap()
            .entry(account.clone())
            .or_insert(BTreeMap::new())
            .insert(sequence_number, mempool_txn);
        self.process_txn(account);
        TxnHash::random()
    }

    pub fn add_raw_txn(&self, raw_txn: TransactionWithAccount) -> TxnHash {
        let sequence_number = raw_txn.sequence_number();
        let status = TxnStatus::Waiting;
        let account = raw_txn.account();
        let txn_hash = TxnHash::from_bytes(&compute_transaction_hash(&raw_txn.txn.unsigned));
        let txn = MempoolTxn { raw_txn, status };
        {
            self.mempool
                .lock()
                .unwrap()
                .entry(account.clone())
                .or_insert(BTreeMap::new())
                .insert(sequence_number, txn);
        }
        self.process_txn(account);
        txn_hash
    }

    pub fn process_txn(&self, account: ExternalAccountAddress) {
        let mut mempool = self.mempool.lock().unwrap();
        let mut water_mark = self.water_mark.lock().unwrap();
        let account_mempool = mempool.get_mut(&account).unwrap();
        let sequence_number = water_mark.entry(account).or_insert(0);
        for txn in account_mempool.values_mut() {
            if txn.raw_txn.sequence_number() == *sequence_number {
                *sequence_number += 1;
                txn.status = TxnStatus::Pending;
            }
        }
    }
}

impl TxPool for KvStoreTxPool {
    fn best_txns(
        &self,
        filter: Option<Box<dyn Fn((ExternalAccountAddress, u64, TxnHash)) -> bool>>,
    ) -> Box<dyn Iterator<Item = VerifiedTxn>> {
        let txns = { (*self.mempool.mempool.lock().unwrap().deref()).clone() };
        let filter = Arc::new(filter);

        let res = Box::new(txns.into_iter().flat_map(move |(addr, txns)| {
            let addr_clone = addr.clone();
            let filter_clone = filter.clone();
            txns.into_iter().filter_map(move |(seq, txn)| {
                let verified_txn = txn.raw_txn.clone().into_verified();
                if let Some(filter) = filter_clone.as_ref() {
                    if !filter((
                        addr_clone.clone(),
                        seq,
                        TxnHash::new(verified_txn.committed_hash()),
                    )) {
                        return None;
                    }
                }
                tracing::info!(
                    "sending txn: sender {:?} nonce {:?}",
                    verified_txn.sender(),
                    verified_txn.seq_number()
                );
                Some(verified_txn)
            })
        }));
        res
    }
}
