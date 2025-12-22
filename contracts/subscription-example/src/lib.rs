#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token::TokenClient, Address, Env, Symbol,
};

#[derive(Clone)]
#[contracttype]
pub struct Subscription {
    pub subscriber: Address,
    pub merchant: Address,
    pub token: Address,
    pub price: i128,
    pub period_secs: u64,
    pub next_charge_ts: u64,
    pub is_active: bool,
}

const KEY: Symbol = symbol_short!("SUB");

fn key_for(subscriber: &Address) -> (Symbol, Address) {
    (KEY, subscriber.clone())
}

#[contract]
pub struct SubscriptionContract;

#[contractimpl]
impl SubscriptionContract {
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        merchant: Address,
        token: Address,
        price: i128,
        period_secs: u64,
    ) {
        subscriber.require_auth();

        let now = env.ledger().timestamp();
        let key = key_for(&subscriber);

        let sub: Subscription = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Subscription {
                subscriber: subscriber.clone(),
                merchant: merchant.clone(),
                token: token.clone(),
                price,
                period_secs,
                next_charge_ts: now + period_secs,
                is_active: true,
            });

        env.storage().persistent().set(&key, &sub);
    }

    pub fn process(env: Env, subscriber: Address) {
        let now = env.ledger().timestamp();
        let key = key_for(&subscriber);

        let mut sub: Subscription = match env.storage().persistent().get(&key) {
            Some(s) => s,
            None => return,
        };

        if !sub.is_active {
            return;
        }

        if now < sub.next_charge_ts {
            return;
        }

        let token_client = TokenClient::new(&env, &sub.token);
        let contract_addr = env.current_contract_address();

        // try pay for subscription
        let payment_result =
            token_client.try_transfer_from(&contract_addr, &subscriber, &sub.merchant, &sub.price);

        if payment_result.is_ok() {
            sub.next_charge_ts += sub.period_secs;
            env.storage().persistent().set(&key, &sub);
        }
    }

    pub fn get(env: Env, subscriber: Address) -> Option<Subscription> {
        env.storage().persistent().get(&key_for(&subscriber))
    }

    pub fn cancel(env: Env, user: Address) {
        user.require_auth();

        let key = key_for(&user);

        let mut sub: Subscription = match env.storage().persistent().get(&key) {
            Some(s) => s,
            None => return,
        };

        sub.is_active = false;

        env.storage().persistent().set(&key, &sub);
    }
}
