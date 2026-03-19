#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token::TokenClient, Address,
    BytesN, Env, String, Vec,
};

// ---------------------------------------------------------------------------
// TTL constants (~5 s per ledger)
// ---------------------------------------------------------------------------
const INSTANCE_TTL_THRESHOLD: u32 = 17_280; // ~1 day
const INSTANCE_TTL_EXTEND: u32 = 518_400; // ~30 days
const PERSISTENT_TTL_THRESHOLD: u32 = 17_280;
const PERSISTENT_TTL_EXTEND: u32 = 518_400;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    InvalidPrice = 1,
    InvalidPeriod = 2,
    AlreadySubscribed = 3,
    SubscriptionNotFound = 4,
    ServiceNotFound = 5,
    Unauthorized = 6,
    AlreadyCancelled = 7,
    TimestampOverflow = 8,
    NotServiceOwner = 9,
    InvalidServiceName = 10,
    SubscriptionExpired = 11,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    // Instance storage
    Admin,
    Token,
    NextServiceId,
    NextSubId,
    // Persistent storage
    Service(u64),
    MerchantServices(Address),
    Sub(u64),
    SubscriberSubs(Address),
    ServiceSubs(u64),
    SubServicePair(Address, u64),
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------
#[derive(Clone, PartialEq, Debug)]
#[contracttype]
pub struct Service {
    pub service_id: u64,
    pub merchant: Address,
    pub name: String,
    pub price: i128,
    pub period_secs: u64,
    pub trial_period_secs: u64,
    pub approve_periods_secs: u64,
    pub is_active: bool,
    pub created_at: u64,
}

#[derive(Clone, PartialEq, Debug)]
#[contracttype]
pub struct Subscription {
    pub sub_id: u64,
    pub subscriber: Address,
    pub service_id: u64,
    pub price: i128,
    pub period_secs: u64,
    pub trial_period_secs: u64,
    pub trial_end_ts: u64,
    pub pay_upfront: bool,
    pub service_end_ts: u64,
    pub next_charge_ts: u64,
    pub created_at: u64,
}

#[derive(Clone, PartialEq, Debug)]
#[contracttype]
pub struct ProcessResult {
    pub charged: u32,
    pub failed: u32,
    pub skipped: u32,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------
#[contract]
pub struct SubscriptionContract;

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------
fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
}

fn bump_persistent(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
}

fn next_service_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NextServiceId)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&DataKey::NextServiceId, &(id + 1));
    id
}

fn next_sub_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NextSubId)
        .unwrap_or(0);
    env.storage().instance().set(&DataKey::NextSubId, &(id + 1));
    id
}

fn checked_add_ts(a: u64, b: u64) -> Result<u64, ContractError> {
    a.checked_add(b).ok_or(ContractError::TimestampOverflow)
}

fn get_token(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Token).unwrap()
}

fn do_approve(env: &Env, subscriber: &Address, service: &Service, periods: u64) {
    let token = get_token(env);
    let token_client = TokenClient::new(env, &token);
    let contract_addr = env.current_contract_address();

    let approve_amount = service.price * (periods as i128);
    let secs_to_approve = service.period_secs.saturating_mul(periods);
    let ledgers_to_approve = secs_to_approve / 5;
    let capped_ledgers = if ledgers_to_approve > u32::MAX as u64 {
        u32::MAX
    } else {
        ledgers_to_approve as u32
    };
    let expiration_ledger = env.ledger().sequence().saturating_add(capped_ledgers);

    token_client.approve(
        subscriber,
        &contract_addr,
        &approve_amount,
        &expiration_ledger,
    );

    env.events().publish(
        (symbol_short!("approve"),),
        (subscriber.clone(), service.service_id, approve_amount, expiration_ledger),
    );
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------
#[contractimpl]
impl SubscriptionContract {
    // ---- Constructor ------------------------------------------------------

    pub fn __constructor(env: Env, admin: Address, token: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::NextServiceId, &0u64);
        env.storage().instance().set(&DataKey::NextSubId, &0u64);
        bump_instance(&env);
    }

    // ---- Service management -----------------------------------------------

    pub fn register_service(
        env: Env,
        merchant: Address,
        name: String,
        price: i128,
        period_secs: u64,
        trial_period_secs: u64,
        approve_periods_secs: u64,
    ) -> Result<Service, ContractError> {
        if price <= 0 {
            return Err(ContractError::InvalidPrice);
        }
        if period_secs == 0 {
            return Err(ContractError::InvalidPeriod);
        }
        if name.len() == 0 {
            return Err(ContractError::InvalidServiceName);
        }
        if approve_periods_secs == 0 {
            return Err(ContractError::InvalidPeriod);
        }

        merchant.require_auth();

        let service_id = next_service_id(&env);
        let now = env.ledger().timestamp();

        let service = Service {
            service_id,
            merchant: merchant.clone(),
            name,
            price,
            period_secs,
            trial_period_secs,
            approve_periods_secs,
            is_active: true,
            created_at: now,
        };

        let svc_key = DataKey::Service(service_id);
        env.storage().persistent().set(&svc_key, &service);
        bump_persistent(&env, &svc_key);

        // Append to merchant's service list
        let ms_key = DataKey::MerchantServices(merchant);
        let mut svc_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&ms_key)
            .unwrap_or_else(|| Vec::new(&env));
        svc_ids.push_back(service_id);
        env.storage().persistent().set(&ms_key, &svc_ids);
        bump_persistent(&env, &ms_key);

        bump_instance(&env);

        env.events()
            .publish((symbol_short!("srv_reg"),), service.clone());

        Ok(service)
    }

    // ---- Subscription lifecycle -------------------------------------------

    /// Subscribe to a service.
    ///
    /// `pay_upfront` controls whether the subscription will auto-renew via
    /// merchant-initiated `process()` calls.
    ///
    /// **With trial period:**
    /// - `pay_upfront = true`  – approves the contract for `approve_periods_secs`
    ///   future periods; no immediate payment. After the trial, `process()`
    ///   charges each period.
    /// - `pay_upfront = false` – subscription covers the trial period only;
    ///   no approval, no payment.  Expires when the trial ends.
    ///
    /// **Without trial period:**
    /// - `pay_upfront = true`  – immediately transfers the first period's price
    ///   and approves the contract for `approve_periods_secs` future periods.
    /// - `pay_upfront = false` – immediately transfers the first period's price
    ///   and approves the contract for 1 period.  `process()` will skip this
    ///   subscription, so it expires after the paid period unless extended via
    ///   `extend_subscription`.
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        service_id: u64,
        pay_upfront: bool,
    ) -> Result<Subscription, ContractError> {
        subscriber.require_auth();

        let svc_key = DataKey::Service(service_id);
        let service: Service = env
            .storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)?;

        if !service.is_active {
            return Err(ContractError::ServiceNotFound);
        }

        // ---- Dedup check ----
        let pair_key = DataKey::SubServicePair(subscriber.clone(), service_id);
        if let Some(existing_sub_id) = env.storage().persistent().get::<_, u64>(&pair_key) {
            let sub_key = DataKey::Sub(existing_sub_id);
            if let Some(existing) = env.storage().persistent().get::<_, Subscription>(&sub_key) {
                if existing.pay_upfront || env.ledger().timestamp() < existing.service_end_ts {
                    return Err(ContractError::AlreadySubscribed);
                }
            }
        }

        let now = env.ledger().timestamp();
        let sub_id = next_sub_id(&env);
        let token = get_token(&env);
        let token_client = TokenClient::new(&env, &token);

        let has_trial = service.trial_period_secs > 0;

        let sub = if has_trial {
            let trial_end = checked_add_ts(now, service.trial_period_secs)?;

            if pay_upfront {
                // Trial + pay_upfront: approve for 12 periods, no immediate payment
                do_approve(&env, &subscriber, &service, service.approve_periods_secs);

                let balance = token_client.balance(&subscriber);
                if balance < service.price {
                    env.events().publish(
                        (symbol_short!("low_bal"),),
                        (subscriber.clone(), service_id, balance, service.price),
                    );
                }
            }
            // Trial + !pay_upfront: no approval, no payment – trial only

            Subscription {
                sub_id,
                subscriber: subscriber.clone(),
                service_id,
                price: service.price,
                period_secs: service.period_secs,
                trial_period_secs: service.trial_period_secs,
                trial_end_ts: trial_end,
                pay_upfront,
                service_end_ts: trial_end,
                next_charge_ts: trial_end,
                created_at: now,
            }
        } else {
            // No trial – immediate first payment
            token_client.transfer(&subscriber, &service.merchant, &service.price);

            let period_end = checked_add_ts(now, service.period_secs)?;

            // Approve for future charges
            let periods = if pay_upfront { service.approve_periods_secs } else { 1 };
            do_approve(&env, &subscriber, &service, periods);

            if pay_upfront {
                let balance = token_client.balance(&subscriber);
                if balance < service.price {
                    env.events().publish(
                        (symbol_short!("low_bal"),),
                        (subscriber.clone(), service_id, balance, service.price),
                    );
                }
            }

            Subscription {
                sub_id,
                subscriber: subscriber.clone(),
                service_id,
                price: service.price,
                period_secs: service.period_secs,
                trial_period_secs: 0,
                trial_end_ts: 0,
                pay_upfront,
                service_end_ts: period_end,
                next_charge_ts: period_end,
                created_at: now,
            }
        };

        // ---- Persist subscription ----
        let sub_key = DataKey::Sub(sub_id);
        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(&env, &sub_key);

        env.storage().persistent().set(&pair_key, &sub_id);
        bump_persistent(&env, &pair_key);

        // Append to subscriber's list
        let ss_key = DataKey::SubscriberSubs(subscriber.clone());
        let mut sub_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&ss_key)
            .unwrap_or_else(|| Vec::new(&env));
        sub_ids.push_back(sub_id);
        env.storage().persistent().set(&ss_key, &sub_ids);
        bump_persistent(&env, &ss_key);

        // Append to service's subscriber list
        let svc_subs_key = DataKey::ServiceSubs(service_id);
        let mut svc_sub_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&svc_subs_key)
            .unwrap_or_else(|| Vec::new(&env));
        svc_sub_ids.push_back(sub_id);
        env.storage().persistent().set(&svc_subs_key, &svc_sub_ids);
        bump_persistent(&env, &svc_subs_key);

        bump_instance(&env);

        env.events()
            .publish((symbol_short!("sub"),), (subscriber, service_id, sub_id));

        Ok(sub)
    }

    pub fn cancel(env: Env, subscriber: Address, sub_id: u64) -> Result<(), ContractError> {
        subscriber.require_auth();

        let sub_key = DataKey::Sub(sub_id);
        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(ContractError::SubscriptionNotFound)?;

        if sub.subscriber != subscriber {
            return Err(ContractError::Unauthorized);
        }
        if !sub.pay_upfront {
            return Err(ContractError::AlreadyCancelled);
        }

        sub.pay_upfront = false;
        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(&env, &sub_key);
        bump_instance(&env);

        let now = env.ledger().timestamp();
        let remaining_secs = sub.service_end_ts.saturating_sub(now);

        env.events().publish(
            (symbol_short!("cancel"),),
            (subscriber, sub_id, sub.service_id, remaining_secs),
        );

        Ok(())
    }

    /// Toggle pay-upfront on or off.  Cannot re-enable on an expired
    /// subscription.
    pub fn toggle_pay_upfront(
        env: Env,
        subscriber: Address,
        sub_id: u64,
    ) -> Result<bool, ContractError> {
        subscriber.require_auth();

        let sub_key = DataKey::Sub(sub_id);
        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(ContractError::SubscriptionNotFound)?;

        if sub.subscriber != subscriber {
            return Err(ContractError::Unauthorized);
        }

        let now = env.ledger().timestamp();

        // Prevent re-enabling on a fully expired subscription
        if !sub.pay_upfront && now >= sub.service_end_ts {
            return Err(ContractError::SubscriptionExpired);
        }

        sub.pay_upfront = !sub.pay_upfront;

        // If re-enabling, refresh the token approval so process() can charge
        if sub.pay_upfront {
            let svc_key = DataKey::Service(sub.service_id);
            let service: Service = env
                .storage()
                .persistent()
                .get(&svc_key)
                .ok_or(ContractError::ServiceNotFound)?;

            do_approve(&env, &subscriber, &service, service.approve_periods_secs);
        }

        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(&env, &sub_key);
        bump_instance(&env);

        env.events().publish(
            (symbol_short!("renew"),),
            (subscriber, sub_id, sub.service_id, sub.pay_upfront),
        );

        Ok(sub.pay_upfront)
    }

    /// Extend an active subscription by refreshing the token approval.
    ///
    /// Call this when your allowance is running low and you want the
    /// subscription to continue renewing.  Sets `pay_upfront` to `true`
    /// and approves the contract for `approve_periods_secs` future periods.
    pub fn extend_subscription(
        env: Env,
        subscriber: Address,
        sub_id: u64,
    ) -> Result<Subscription, ContractError> {
        subscriber.require_auth();

        let sub_key = DataKey::Sub(sub_id);
        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(ContractError::SubscriptionNotFound)?;

        if sub.subscriber != subscriber {
            return Err(ContractError::Unauthorized);
        }

        let now = env.ledger().timestamp();
        if now >= sub.service_end_ts {
            return Err(ContractError::SubscriptionExpired);
        }

        let svc_key = DataKey::Service(sub.service_id);
        let service: Service = env
            .storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)?;

        do_approve(&env, &subscriber, &service, service.approve_periods_secs);

        sub.pay_upfront = true;
        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(&env, &sub_key);
        bump_instance(&env);

        env.events().publish(
            (symbol_short!("extend"),),
            (subscriber, sub_id, sub.service_id),
        );

        Ok(sub)
    }

    pub fn process(
        env: Env,
        merchant: Address,
        service_id: u64,
    ) -> Result<ProcessResult, ContractError> {
        merchant.require_auth();

        let svc_key = DataKey::Service(service_id);
        let service: Service = env
            .storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)?;

        if service.merchant != merchant {
            return Err(ContractError::NotServiceOwner);
        }

        let token = get_token(&env);
        let token_client = TokenClient::new(&env, &token);
        let contract_addr = env.current_contract_address();
        let now = env.ledger().timestamp();

        let svc_subs_key = DataKey::ServiceSubs(service_id);
        let sub_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&svc_subs_key)
            .unwrap_or_else(|| Vec::new(&env));

        let mut charged: u32 = 0;
        let mut failed: u32 = 0;
        let mut skipped: u32 = 0;

        for i in 0..sub_ids.len() {
            let sid = sub_ids.get(i).unwrap();
            let sub_key = DataKey::Sub(sid);

            let mut sub: Subscription = env
                .storage()
                .persistent()
                .get(&sub_key)
                .expect("subscription not found");

            if !sub.pay_upfront {
                skipped += 1;
                continue;
            }

            if now < sub.next_charge_ts {
                skipped += 1;
                continue;
            }

            let payment_result = token_client.try_transfer_from(
                &contract_addr,
                &sub.subscriber,
                &merchant,
                &sub.price,
            );

            if payment_result.is_ok() {
                // Detect trial -> paid transition (first real charge)
                let was_trial = sub.trial_period_secs > 0
                    && sub.next_charge_ts == sub.trial_end_ts;

                let new_next = checked_add_ts(sub.next_charge_ts, sub.period_secs)?;
                sub.next_charge_ts = new_next;
                sub.service_end_ts = new_next;
                env.storage().persistent().set(&sub_key, &sub);
                bump_persistent(&env, &sub_key);
                charged += 1;

                env.events().publish(
                    (symbol_short!("charge"),),
                    (sub.subscriber.clone(), service_id, sub.price),
                );

                // Trial just ended -> first paid period started
                if was_trial {
                    env.events().publish(
                        (symbol_short!("trl_end"),),
                        (sub.subscriber.clone(), service_id, sub.sub_id),
                    );
                }

                // Check remaining allowance for next cycle
                let remaining_allowance =
                    token_client.allowance(&sub.subscriber, &contract_addr);
                if remaining_allowance < sub.price {
                    env.events().publish(
                        (symbol_short!("low_alw"),),
                        (sub.subscriber.clone(), service_id, remaining_allowance, sub.price),
                    );
                }

                // Check subscriber balance for next cycle
                let balance = token_client.balance(&sub.subscriber);
                if balance < sub.price {
                    env.events().publish(
                        (symbol_short!("low_bal"),),
                        (sub.subscriber.clone(), service_id, balance, sub.price),
                    );
                }
            } else {
                sub.pay_upfront = false;
                env.storage().persistent().set(&sub_key, &sub);
                bump_persistent(&env, &sub_key);
                failed += 1;

                env.events().publish(
                    (symbol_short!("chg_fail"),),
                    (sub.subscriber.clone(), service_id, sub.sub_id),
                );
            }
        }

        bump_instance(&env);

        Ok(ProcessResult {
            charged,
            failed,
            skipped,
        })
    }

    // ---- Queries ----------------------------------------------------------

    pub fn get_subscription(
        env: Env,
        caller: Address,
        sub_id: u64,
    ) -> Result<Subscription, ContractError> {
        caller.require_auth();

        let sub_key = DataKey::Sub(sub_id);
        let sub: Subscription = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(ContractError::SubscriptionNotFound)?;

        if caller != sub.subscriber {
            let svc_key = DataKey::Service(sub.service_id);
            let service: Service = env
                .storage()
                .persistent()
                .get(&svc_key)
                .ok_or(ContractError::ServiceNotFound)?;
            if caller != service.merchant {
                return Err(ContractError::Unauthorized);
            }
        }

        Ok(sub)
    }

    pub fn get_subscriber_subs(env: Env, subscriber: Address) -> Vec<Subscription> {
        subscriber.require_auth();

        let ss_key = DataKey::SubscriberSubs(subscriber);
        let sub_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&ss_key)
            .unwrap_or_else(|| Vec::new(&env));

        let mut result: Vec<Subscription> = Vec::new(&env);
        for i in 0..sub_ids.len() {
            let sid = sub_ids.get(i).unwrap();
            let sub: Subscription = env
                .storage()
                .persistent()
                .get(&DataKey::Sub(sid))
                .expect("subscription not found");
            result.push_back(sub);
        }
        result
    }

    pub fn get_merchant_subs(
        env: Env,
        merchant: Address,
        service_id: u64,
    ) -> Result<Vec<Subscription>, ContractError> {
        merchant.require_auth();

        let svc_key = DataKey::Service(service_id);
        let service: Service = env
            .storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)?;

        if service.merchant != merchant {
            return Err(ContractError::NotServiceOwner);
        }

        let svc_subs_key = DataKey::ServiceSubs(service_id);
        let sub_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&svc_subs_key)
            .unwrap_or_else(|| Vec::new(&env));

        let mut result: Vec<Subscription> = Vec::new(&env);
        for i in 0..sub_ids.len() {
            let sid = sub_ids.get(i).unwrap();
            let sub: Subscription = env
                .storage()
                .persistent()
                .get(&DataKey::Sub(sid))
                .expect("subscription not found");
            result.push_back(sub);
        }
        Ok(result)
    }

    pub fn get_service(env: Env, service_id: u64) -> Result<Service, ContractError> {
        let svc_key = DataKey::Service(service_id);
        env.storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)
    }

    pub fn get_merchant_services(env: Env, merchant: Address) -> Vec<Service> {
        let ms_key = DataKey::MerchantServices(merchant);
        let svc_ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&ms_key)
            .unwrap_or_else(|| Vec::new(&env));

        let mut result: Vec<Service> = Vec::new(&env);
        for i in 0..svc_ids.len() {
            let sid = svc_ids.get(i).unwrap();
            let svc: Service = env
                .storage()
                .persistent()
                .get(&DataKey::Service(sid))
                .expect("service not found");
            result.push_back(svc);
        }
        result
    }

    pub fn is_subscription_active(env: Env, subscriber: Address, service_id: u64) -> bool {
        let pair_key = DataKey::SubServicePair(subscriber, service_id);
        let sub_id: u64 = match env.storage().persistent().get(&pair_key) {
            Some(id) => id,
            None => return false,
        };
        let sub: Subscription = env
            .storage()
            .persistent()
            .get(&DataKey::Sub(sub_id))
            .expect("subscription not found");
        env.ledger().timestamp() < sub.service_end_ts
    }

    // ---- Admin ------------------------------------------------------------

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("upgrade"),), new_wasm_hash);
    }

    pub fn version(_env: Env) -> u32 {
        3
    }
}

mod test;
