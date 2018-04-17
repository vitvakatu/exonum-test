// Copyright 2018 The Exonum Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

extern crate bodyparser;
#[macro_use]
extern crate exonum;
extern crate iron;
extern crate router;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

pub mod schema {
    use exonum::storage::{Fork, Entry, Snapshot};

    pub struct BalanceSchema<T> {
        view: T,
    }

    impl<T: AsRef<Snapshot>> BalanceSchema<T> {
        pub fn new(view: T) -> Self {
            Self { view }
        }

        pub fn balance(&self) -> Entry<&Snapshot, u64> {
            Entry::new("balance", self.view.as_ref())
        }
    }

    impl<'a> BalanceSchema<&'a mut Fork> {
        pub fn balance_mut(&mut self) -> Entry<&mut Fork, u64> {
            Entry::new("balance", self.view)
        }
    }
}

pub mod transactions {
    use service::SERVICE_ID;

    transactions! {
        pub BalanceTransactions {
            const SERVICE_ID = SERVICE_ID;

            struct TxAddBalance {
                amount: u64,
                seed: u64
            }
        }
    }
}

pub mod contracts {
    use exonum::blockchain::{ExecutionResult, Transaction};
    use exonum::{storage::Fork};

    use schema::{BalanceSchema};
    use transactions::TxAddBalance;

    impl Transaction for TxAddBalance {
        fn verify(&self) -> bool {
            true
        }

        fn execute(&self, view: &mut Fork) -> ExecutionResult {
            let mut schema = BalanceSchema::new(view);
            let new_balance = schema.balance().get().unwrap() + self.amount();
            schema.balance_mut().set(new_balance);

            Ok(())
        }
    }
}

pub mod api {
    use exonum::blockchain::{Blockchain, Transaction};
    use exonum::node::{ApiSender, TransactionSend};
    use exonum::crypto::{Hash};
    use exonum::api::{Api, ApiError};
    use iron::prelude::*;
    use router::Router;

    use bodyparser;
    use serde_json;
    use schema::{BalanceSchema};
    use transactions::BalanceTransactions;

    #[derive(Clone)]
    pub struct BalanceApi {
        channel: ApiSender,
        blockchain: Blockchain,
    }

    impl BalanceApi {
        pub fn new(channel: ApiSender, blockchain: Blockchain) -> Self {
            Self {
                channel,
                blockchain,
            }
        }
    }

    #[derive(Serialize, Deserialize)]
    pub struct TransactionResponse {
        pub tx_hash: Hash,
    }

    impl BalanceApi {
        /// Endpoint for getting a single wallet.
        fn get_balance(&self, _: &mut Request) -> IronResult<Response> {
            let snapshot = self.blockchain.snapshot();
            let schema = BalanceSchema::new(snapshot);

            let balance = schema.balance().get();
            self.ok_response(&serde_json::to_value(balance).unwrap())
        }

        /// Common processing for transaction-accepting endpoints.
        fn post_transaction(&self, req: &mut Request) -> IronResult<Response> {
            match req.get::<bodyparser::Struct<BalanceTransactions>>() {
                Ok(Some(transaction)) => {
                    let transaction: Box<Transaction> = transaction.into();
                    let tx_hash = transaction.hash();
                    self.channel.send(transaction).map_err(ApiError::from)?;
                    let json = TransactionResponse { tx_hash };
                    self.ok_response(&serde_json::to_value(&json).unwrap())
                }
                Ok(None) => Err(ApiError::BadRequest("Empty request body".into()))?,
                Err(e) => Err(ApiError::BadRequest(e.to_string()))?,
            }
        }
    }

    impl Api for BalanceApi {
        fn wire(&self, router: &mut Router) {
            let self_ = self.clone();
            let post_transaction = move |req: &mut Request| self_.post_transaction(req);
            let self_ = self.clone();
            let get_balance = move |req: &mut Request| self_.get_balance(req);

            router.post("/v1/transaction", post_transaction, "post_transaction");
            router.get("/v1/balance", get_balance, "get_balance");
        }
    }
}

pub mod service {
    use exonum::blockchain::{ApiContext, Service, Transaction, TransactionSet};
    use exonum::{encoding, api::Api, crypto::Hash, messages::RawTransaction, storage::{Snapshot, Fork}};
    use iron::Handler;
    use router::Router;
    use serde_json::Value;

    use transactions::BalanceTransactions;
    use schema::BalanceSchema;
    use api::BalanceApi;

    use exonum::helpers::fabric::{self, Context};

    /// Service ID for the `Service` trait.
    pub const SERVICE_ID: u16 = 1;

    pub struct BalanceService;

    impl Service for BalanceService {

        fn service_id(&self) -> u16 {
            SERVICE_ID
        }

        fn service_name(&self) -> &str {
            "balance"
        }

        fn state_hash(&self, _: &Snapshot) -> Vec<Hash> {
            vec![]
        }

        fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, encoding::Error> {
            let tx = BalanceTransactions::tx_from_raw(raw)?;
            Ok(tx.into())
        }

        fn public_api_handler(&self, ctx: &ApiContext) -> Option<Box<Handler>> {
            let mut router = Router::new();
            let api = BalanceApi::new(ctx.node_channel().clone(), ctx.blockchain().clone());
            api.wire(&mut router);
            Some(Box::new(router))
        }

        fn initialize(&self, fork: &mut Fork) -> Value {
            let mut schema = BalanceSchema::new(fork);
            schema.balance_mut().set(0);
            Value::Null
        }
    }

    #[derive(Debug)]
    pub struct ServiceFactory;

    impl fabric::ServiceFactory for ServiceFactory {
        fn make_service(&mut self, _: &Context) -> Box<Service> {
            Box::new(BalanceService)
        }
    }
}
