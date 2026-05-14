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
const PERSISTENT_TTL_EXTEND_MIN: u32 = 518_400; // ~30 days floor
const SECS_PER_LEDGER: u64 = 5;

// Max entries per index page. Bounded so a single read/write touches a
// constant-sized entry instead of the full historical set.
const PAGE_SIZE: u32 = 50;

// LIB-13: minimum delay between proposing and applying a contract upgrade.
// Subscribers approve this contract as a token spender, so a malicious or
// compromised admin pushing new WASM can immediately rewrite billing logic
// or drain existing allowances. The timelock + on-chain `up_prop` event
// gives users a known window to revoke their allowances before any new
// bytecode takes effect. 7 days is the same order of magnitude as typical
// DeFi upgrade timelocks and well above the simulate→execute drift.
const UPGRADE_TIMELOCK_SECS: u64 = 7 * 24 * 60 * 60;

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
    ServiceNotActive = 12,
    NoAdminProposed = 13,
    NotProposedAdmin = 14,
    NoUpgradeProposed = 15,
    UpgradeTimelockNotElapsed = 16,
    UpgradeAlreadyPending = 17,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    // Instance storage
    Admin,
    PendingAdmin,
    PendingUpgrade,
    Token,
    NextServiceId,
    NextSubId,
    // Persistent storage
    Service(u64),
    Sub(u64),
    SubServicePair(Address, u64),
    TrialUsed(Address, u64),
    // Paginated indexes: each *Page key holds at most PAGE_SIZE entries; the
    // companion *Count key tracks total live entries (sum of all pages).
    MerchantServicesPage(Address, u32),
    MerchantServicesCount(Address),
    SubscriberSubsPage(Address, u32),
    SubscriberSubsCount(Address),
    ServiceSubsPage(u64, u32),
    ServiceSubsCount(u64),
    // Reverse pointer: per-sub_id, the page index it occupies in
    // ServiceSubs/SubscriberSubs. Lets cancel/re-subscribe locate and
    // remove the entry without scanning every page.
    SubIndex(u64),
    // LIB-8 (full fix): per-sub allowance reservation. Mirrors the share of
    // the (subscriber, contract) on-chain allowance contributed by this sub.
    // Incremented on every do_approve(sub_id), decremented by sub.price on
    // each successful charge, drained on cancel / toggle-off. Lets us reduce
    // the contract's allowance by exactly the cancelled sub's contribution
    // when the subscriber keeps other auto-renewing subs alive.
    SubReservedAmount(u64),
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
    pub approve_periods: u64,
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
    pub auto_renew: bool,
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
    pub total: u32,
}

#[derive(Clone, PartialEq, Debug)]
#[contracttype]
pub struct SubIndex {
    pub service_page: u32,
    pub subscriber_page: u32,
}

/// LIB-13: a queued contract upgrade. Public via `get_pending_upgrade()` so
/// off-chain watchers can inspect the staged WASM hash and the earliest
/// ledger timestamp at which it can be applied.
#[derive(Clone, PartialEq, Debug)]
#[contracttype]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub apply_after_ts: u64,
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

/// Convert a desired live-window in seconds to ledger count, clamped to the
/// network minimum-TTL floor. Callers pass the full window they need; no
/// implicit doubling here.
fn ttl_extend_for_window(window_secs: u64) -> u32 {
    let ledgers = window_secs / SECS_PER_LEDGER;
    let capped = if ledgers > u32::MAX as u64 {
        u32::MAX
    } else {
        ledgers as u32
    };
    core::cmp::max(capped, PERSISTENT_TTL_EXTEND_MIN)
}

/// LIB-7: TTL window for keys whose next re-bump is the first post-trial /
/// post-period `process()` call. Trial-bearing keys (Sub, SubServicePair,
/// TrialUsed, ServiceSubs/SubscriberSubs entries, Service) must outlive the
/// trial window plus one full billing period of slack, otherwise the chain
/// can forget the subscription before the first paid charge ever lands.
/// Reduces to `2 * period_secs` when `trial_period_secs == 0`, preserving the
/// non-trial behavior the contract had before.
fn sub_ttl_window_secs(period_secs: u64, trial_period_secs: u64) -> u64 {
    period_secs.saturating_add(core::cmp::max(period_secs, trial_period_secs))
}

fn bump_persistent(env: &Env, key: &DataKey, window_secs: u64) {
    env.storage().persistent().extend_ttl(
        key,
        PERSISTENT_TTL_THRESHOLD,
        ttl_extend_for_window(window_secs),
    );
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

/// LIB-10: pick an allowance expiration ledger that sits one bucket below the
/// network's max_ttl, rounded UP to a bucket boundary. Rounding up keeps the
/// value strictly above `seq` for short windows; the bucket gap absorbs the
/// drift between simulate→execute so a re-approval can't land in a smaller
/// bucket than the previous one and shorten the live-until.
fn compute_approval_expiration(env: &Env) -> u32 {
    let max_ttl = env.storage().max_ttl();
    let seq = env.ledger().sequence();
    // 720 ledgers ≈ 1 hour — much larger than the simulate→execute gap (~seconds).
    const LEDGER_BUCKET: u32 = 720;
    if max_ttl <= LEDGER_BUCKET {
        // Pathological: network max_ttl smaller than one bucket. Skip
        // bucket rounding and use a direct value within SAC's limit.
        seq.saturating_add(max_ttl.saturating_sub(1))
    } else {
        let target = seq.saturating_add(max_ttl - LEDGER_BUCKET);
        target
            .saturating_add(LEDGER_BUCKET - 1)
            / LEDGER_BUCKET
            * LEDGER_BUCKET
    }
}

fn do_approve(
    env: &Env,
    subscriber: &Address,
    sub_id: u64,
    service: &Service,
    periods: u64,
) -> Result<(), ContractError> {
    let token = get_token(env);
    let token_client = TokenClient::new(env, &token);
    let contract_addr = env.current_contract_address();

    let this_sub_amount = service
        .price
        .checked_mul(periods as i128)
        .ok_or(ContractError::TimestampOverflow)?;

    // LIB-9: allowance is keyed by (owner, spender), not by service. Read
    // what's already approved and add this sub's budget on top — a plain
    // overwrite would shrink the budget reserved by any other active sub
    // of this user and silently break their renewals.
    let existing_allowance = token_client.allowance(subscriber, &contract_addr);
    let approve_amount = existing_allowance
        .checked_add(this_sub_amount)
        .ok_or(ContractError::TimestampOverflow)?;

    // Allowance expiration is set to nearly max_ttl (see
    // compute_approval_expiration) so a later approve from a shorter-lived
    // sub cannot truncate the budget's live-until under the longest
    // currently-active sub. The user's explicit cancel / toggle-off path
    // is what bounds the on-chain allowance lifetime, not this expiration.
    let expiration_ledger = compute_approval_expiration(env);

    token_client.approve(
        subscriber,
        &contract_addr,
        &approve_amount,
        &expiration_ledger,
    );

    // LIB-8 (full fix): mirror this sub's contribution into SubReservedAmount
    // so cancel / toggle-off can later reduce the on-chain allowance by
    // exactly this sub's share, even when other auto-renewing subs are alive.
    // Add (not replace) so extend_subscription's top-up matches the on-chain
    // add-on-top semantics above.
    let reserved_key = DataKey::SubReservedAmount(sub_id);
    let existing_reserved: i128 = env.storage().persistent().get(&reserved_key).unwrap_or(0);
    let new_reserved = existing_reserved
        .checked_add(this_sub_amount)
        .ok_or(ContractError::TimestampOverflow)?;
    env.storage().persistent().set(&reserved_key, &new_reserved);
    bump_persistent(
        env,
        &reserved_key,
        sub_ttl_window_secs(service.period_secs, service.trial_period_secs),
    );

    env.events().publish(
        (symbol_short!("approve"),),
        (
            subscriber.clone(),
            service.service_id,
            approve_amount,
            expiration_ledger,
            token,
        ),
    );

    Ok(())
}

/// LIB-8 (full fix): release the sub's tracked allowance reservation. Reads
/// SubReservedAmount for `sub_id`, reduces the on-chain allowance by exactly
/// that share (clamped), then removes the reservation key. Idempotent and
/// safe to call when no reservation exists (e.g. trial + !auto_renew subs).
fn release_sub_reservation(env: &Env, subscriber: &Address, sub_id: u64) {
    let reserved_key = DataKey::SubReservedAmount(sub_id);
    let reserved: i128 = env.storage().persistent().get(&reserved_key).unwrap_or(0);
    if reserved > 0 {
        do_reduce_approval(env, subscriber, reserved);
    }
    env.storage().persistent().remove(&reserved_key);
}

/// LIB-8 (full fix): reduce the contract's allowance from `subscriber` by
/// `amount`, clamped to what is currently on chain. If the resulting
/// allowance is zero the entry is fully revoked (`approve(0, 0)` clears it);
/// otherwise the entry is re-written with the same long expiration the
/// approve path uses. Inner-invoked from `cancel()` / `toggle_auto_renew(off)`
/// so the reduction rides inside the same atomic auth tree the caller signed.
fn do_reduce_approval(env: &Env, subscriber: &Address, amount: i128) {
    if amount <= 0 {
        return;
    }

    let token = get_token(env);
    let token_client = TokenClient::new(env, &token);
    let contract_addr = env.current_contract_address();

    let current = token_client.allowance(subscriber, &contract_addr);
    if current <= 0 {
        return;
    }

    let new_amount = current.saturating_sub(amount).max(0);

    if new_amount == 0 {
        token_client.approve(subscriber, &contract_addr, &0i128, &0u32);
        env.events()
            .publish((symbol_short!("revoke"),), (subscriber.clone(), token));
    } else {
        let expiration_ledger = compute_approval_expiration(env);
        token_client.approve(subscriber, &contract_addr, &new_amount, &expiration_ledger);
        // Distinct topic from "approve" (which signals an explicit user
        // top-up via subscribe / extend / toggle_on). "alw_red" is the
        // contract reducing the (subscriber, contract) allowance as part
        // of cancel / toggle_off, so an indexer keyed on "approve" doesn't
        // see a payload-shape mismatch.
        env.events().publish(
            (symbol_short!("alw_red"),),
            (
                subscriber.clone(),
                new_amount,
                expiration_ledger,
                token,
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Paginated index helpers
// ---------------------------------------------------------------------------
//
// An index is split across pages of at most PAGE_SIZE entries. Pages 0..tail-1
// are always full; only the tail page is partial. Removal uses swap-with-
// global-tail to keep this invariant, so the caller may need to update the
// reverse pointer of whichever element is moved into the removed slot.

/// Append `item` to a paginated index. Returns the page index it was placed on.
fn paginated_append<F>(
    env: &Env,
    page_key_fn: F,
    count_key: &DataKey,
    item: u64,
    window_secs: u64,
) -> u32
where
    F: Fn(u32) -> DataKey,
{
    let count: u64 = env.storage().persistent().get(count_key).unwrap_or(0);
    let page_idx = (count / PAGE_SIZE as u64) as u32;
    let page_key = page_key_fn(page_idx);
    let mut page: Vec<u64> = env
        .storage()
        .persistent()
        .get(&page_key)
        .unwrap_or_else(|| Vec::new(env));
    page.push_back(item);
    env.storage().persistent().set(&page_key, &page);
    bump_persistent(env, &page_key, window_secs);
    env.storage().persistent().set(count_key, &(count + 1));
    bump_persistent(env, count_key, window_secs);
    page_idx
}

/// Remove `target` from `target_page`, swapping in the global tail to keep
/// pages dense. Returns Some((moved_id, new_page_idx)) when a different
/// element was relocated into the removed slot — the caller must update its
/// reverse pointer. Returns None if no relocation was needed (target was the
/// global tail) or target wasn't found.
fn paginated_remove<F>(
    env: &Env,
    page_key_fn: F,
    count_key: &DataKey,
    target: u64,
    target_page: u32,
    window_secs: u64,
) -> Option<(u64, u32)>
where
    F: Fn(u32) -> DataKey,
{
    let target_page_key = page_key_fn(target_page);
    let mut target_page_vec: Vec<u64> = match env.storage().persistent().get(&target_page_key) {
        Some(v) => v,
        None => return None,
    };

    let mut target_idx_opt: Option<u32> = None;
    for i in 0..target_page_vec.len() {
        if target_page_vec.get(i).unwrap() == target {
            target_idx_opt = Some(i);
            break;
        }
    }
    let target_idx = match target_idx_opt {
        Some(i) => i,
        None => return None,
    };

    let count: u64 = env.storage().persistent().get(count_key).unwrap_or(0);
    if count == 0 {
        return None;
    }
    let last_page_idx = ((count - 1) / PAGE_SIZE as u64) as u32;

    let moved: Option<(u64, u32)> = if last_page_idx == target_page {
        // Target lives on the last page; swap-and-pop within this page only.
        let last_idx = target_page_vec.len() - 1;
        if target_idx != last_idx {
            let last_item = target_page_vec.get(last_idx).unwrap();
            target_page_vec.set(target_idx, last_item);
        }
        target_page_vec.pop_back();
        if target_page_vec.is_empty() {
            env.storage().persistent().remove(&target_page_key);
        } else {
            env.storage().persistent().set(&target_page_key, &target_page_vec);
            bump_persistent(env, &target_page_key, window_secs);
        }
        // Movement was within the same page, so reverse pointer is unchanged.
        None
    } else {
        // Target is on an earlier page; relocate the global tail into its slot.
        let last_page_key = page_key_fn(last_page_idx);
        let mut last_page_vec: Vec<u64> =
            env.storage().persistent().get(&last_page_key).unwrap();
        let last_item_idx = last_page_vec.len() - 1;
        let last_item = last_page_vec.get(last_item_idx).unwrap();

        target_page_vec.set(target_idx, last_item);
        env.storage().persistent().set(&target_page_key, &target_page_vec);
        bump_persistent(env, &target_page_key, window_secs);

        last_page_vec.pop_back();
        if last_page_vec.is_empty() {
            env.storage().persistent().remove(&last_page_key);
        } else {
            env.storage().persistent().set(&last_page_key, &last_page_vec);
            bump_persistent(env, &last_page_key, window_secs);
        }

        Some((last_item, target_page))
    };

    env.storage().persistent().set(count_key, &(count - 1));
    bump_persistent(env, count_key, window_secs);

    moved
}

/// Read a paginated index in full. Returns the concatenated contents of all
/// pages in append order. Each page is loaded as a separate persistent entry,
/// so total work is O(n) but no single read needs to fit the entire history
/// in one entry.
fn paginated_read_all<F>(env: &Env, page_key_fn: F, count_key: &DataKey) -> Vec<u64>
where
    F: Fn(u32) -> DataKey,
{
    let count: u64 = env.storage().persistent().get(count_key).unwrap_or(0);
    let mut out: Vec<u64> = Vec::new(env);
    if count == 0 {
        return out;
    }
    let last_page_idx = ((count - 1) / PAGE_SIZE as u64) as u32;
    for p in 0..=last_page_idx {
        if let Some(page) = env
            .storage()
            .persistent()
            .get::<_, Vec<u64>>(&page_key_fn(p))
        {
            for i in 0..page.len() {
                out.push_back(page.get(i).unwrap());
            }
        }
    }
    out
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
        approve_periods: u64,
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
        if approve_periods == 0 {
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
            approve_periods,
            is_active: true,
            created_at: now,
        };

        let svc_window = sub_ttl_window_secs(period_secs, trial_period_secs);
        let svc_key = DataKey::Service(service_id);
        env.storage().persistent().set(&svc_key, &service);
        bump_persistent(&env, &svc_key, svc_window);

        // Append to merchant's paginated service index
        let merchant_for_pages = merchant.clone();
        let count_key = DataKey::MerchantServicesCount(merchant);
        paginated_append(
            &env,
            |p| DataKey::MerchantServicesPage(merchant_for_pages.clone(), p),
            &count_key,
            service_id,
            svc_window,
        );

        bump_instance(&env);

        env.events()
            .publish((symbol_short!("srv_reg"),), service.clone());

        Ok(service)
    }

    /// Activate or deactivate a service. Only the service's own merchant
    /// may call this. While inactive, `subscribe()` rejects new sign-ups
    /// for the service (existing subscriptions are unaffected and continue
    /// billing through their `service_end_ts`). Used as the merchant's
    /// off-switch for retiring old plans or pausing sign-ups.
    pub fn set_service_active(
        env: Env,
        merchant: Address,
        service_id: u64,
        active: bool,
    ) -> Result<(), ContractError> {
        merchant.require_auth();

        let svc_key = DataKey::Service(service_id);
        let mut service: Service = env
            .storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)?;

        if service.merchant != merchant {
            return Err(ContractError::NotServiceOwner);
        }

        service.is_active = active;
        env.storage().persistent().set(&svc_key, &service);
        bump_persistent(
            &env,
            &svc_key,
            sub_ttl_window_secs(service.period_secs, service.trial_period_secs),
        );
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("svc_act"),), (merchant, service_id, active));

        Ok(())
    }

    // ---- Subscription lifecycle -------------------------------------------

    /// Subscribe to a service.
    ///
    /// `auto_renew` controls whether the subscription will auto-renew via
    /// merchant-initiated `process()` calls.
    ///
    /// A trial is granted only on the subscriber's first subscription to a
    /// service that has `trial_period_secs > 0`. Trial consumption is recorded
    /// independently of `auto_renew` and persists across `cancel()` /
    /// re-subscription, so the one-trial policy cannot be bypassed.
    ///
    /// **First subscription to a service with a trial:**
    /// - `auto_renew = true`  – approves the contract for `approve_periods`
    ///   future periods; no immediate payment. After the trial, `process()`
    ///   charges each period.
    /// - `auto_renew = false` – subscription covers the trial period only;
    ///   no approval, no payment.  Expires when the trial ends.
    ///
    /// **Re-subscription after trial, or service without a trial:**
    /// - `auto_renew = true`  – immediately transfers the first period's price
    ///   and approves the contract for `approve_periods` future periods.
    /// - `auto_renew = false` – immediately transfers the first period's price
    ///   and approves the contract for 1 period.  `process()` will skip this
    ///   subscription, so it expires after the paid period unless extended via
    ///   `extend_subscription`.
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        service_id: u64,
        auto_renew: bool,
    ) -> Result<Subscription, ContractError> {
        subscriber.require_auth();

        let svc_key = DataKey::Service(service_id);
        let service: Service = env
            .storage()
            .persistent()
            .get(&svc_key)
            .ok_or(ContractError::ServiceNotFound)?;
        let svc_window = sub_ttl_window_secs(service.period_secs, service.trial_period_secs);
        bump_persistent(&env, &svc_key, svc_window);

        if !service.is_active {
            return Err(ContractError::ServiceNotActive);
        }

        // ---- Trial-consumption flag (independent of auto_renew / cancel) ----
        let trial_used_key = DataKey::TrialUsed(subscriber.clone(), service_id);
        let mut had_trial = env.storage().persistent().has(&trial_used_key);
        if had_trial {
            bump_persistent(&env, &trial_used_key, svc_window);
        }

        // ---- Dedup check ----
        // If a prior subscription exists and is fully dead (auto_renew=false
        // AND past service_end_ts), we'll prune its index entries below
        // before adding the new sub_id, so the indexes don't grow on each
        // re-subscribe cycle.
        let pair_key = DataKey::SubServicePair(subscriber.clone(), service_id);
        let mut prior_sub_to_prune: Option<u64> = None;
        if let Some(existing_sub_id) = env.storage().persistent().get::<_, u64>(&pair_key) {
            bump_persistent(&env, &pair_key, svc_window);
            let sub_key = DataKey::Sub(existing_sub_id);
            if let Some(existing) = env.storage().persistent().get::<_, Subscription>(&sub_key) {
                bump_persistent(
                    &env,
                    &sub_key,
                    sub_ttl_window_secs(existing.period_secs, existing.trial_period_secs),
                );
                if existing.auto_renew || env.ledger().timestamp() < existing.service_end_ts {
                    return Err(ContractError::AlreadySubscribed);
                }
                // Legacy migration: pre-fix subscriptions never set TrialUsed,
                // so derive it from the prior subscription's trial_period_secs.
                if existing.trial_period_secs > 0 {
                    had_trial = true;
                }
                prior_sub_to_prune = Some(existing_sub_id);
            }
        }

        let now = env.ledger().timestamp();
        let sub_id = next_sub_id(&env);
        let token = get_token(&env);
        let token_client = TokenClient::new(&env, &token);

        let grant_trial = service.trial_period_secs > 0 && !had_trial;

        let sub = if grant_trial {
            let trial_end = checked_add_ts(now, service.trial_period_secs)?;

            // Mark trial as consumed before any external calls so the flag
            // sticks regardless of subsequent cancel() or allowance revocation.
            env.storage().persistent().set(&trial_used_key, &true);
            bump_persistent(&env, &trial_used_key, svc_window);

            if auto_renew {
                // Trial + auto_renew: approve for future periods. Allowance
                // expiration uses max_ttl so the trial window is covered too.
                do_approve(&env, &subscriber, sub_id, &service, service.approve_periods)?;

                let balance = token_client.balance(&subscriber);
                if balance < service.price {
                    env.events().publish(
                        (symbol_short!("low_bal"),),
                        (subscriber.clone(), service_id, balance, service.price),
                    );
                }
            }
            // Trial + !auto_renew: no approval, no payment – trial only

            Subscription {
                sub_id,
                subscriber: subscriber.clone(),
                service_id,
                price: service.price,
                period_secs: service.period_secs,
                trial_period_secs: service.trial_period_secs,
                trial_end_ts: trial_end,
                auto_renew,
                service_end_ts: trial_end,
                next_charge_ts: trial_end,
                created_at: now,
            }
        } else {
            let period_end = checked_add_ts(now, service.period_secs)?;

            // Approve before transfer so the contract is pre-authorized
            let periods = if auto_renew { service.approve_periods } else { 1 };
            do_approve(&env, &subscriber, sub_id, &service, periods)?;

            // No trial – immediate first payment
            token_client.transfer(&subscriber, &service.merchant, &service.price);

            if auto_renew {
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
                auto_renew,
                service_end_ts: period_end,
                next_charge_ts: period_end,
                created_at: now,
            }
        };

        // ---- Persist subscription ----
        // LIB-7: window must cover the trial leg too — the next bump on these
        // keys is the post-trial process() call, which may be `trial_period`
        // away on the new sub.
        let new_window = sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs);
        let sub_key = DataKey::Sub(sub_id);
        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(&env, &sub_key, new_window);

        env.storage().persistent().set(&pair_key, &sub_id);
        bump_persistent(&env, &pair_key, new_window);

        // Prune the dead prior sub_id from the indexes before appending the
        // new one. Without this, every re-subscribe cycle would leave a
        // permanently-stale entry behind and the indexes would grow without
        // bound. Reverse-pointer (SubIndex) for the prior sub tells us which
        // page to touch — O(1) per index, not a full scan.
        if let Some(old_sub_id) = prior_sub_to_prune {
            let idx_key = DataKey::SubIndex(old_sub_id);
            if let Some(old_idx) = env.storage().persistent().get::<_, SubIndex>(&idx_key) {
                let svc_id_for_pages = service_id;
                let moved_in_service = paginated_remove(
                    &env,
                    |p| DataKey::ServiceSubsPage(svc_id_for_pages, p),
                    &DataKey::ServiceSubsCount(svc_id_for_pages),
                    old_sub_id,
                    old_idx.service_page,
                    new_window,
                );
                if let Some((moved_id, new_page)) = moved_in_service {
                    let moved_key = DataKey::SubIndex(moved_id);
                    if let Some(mut moved_idx) =
                        env.storage().persistent().get::<_, SubIndex>(&moved_key)
                    {
                        moved_idx.service_page = new_page;
                        env.storage().persistent().set(&moved_key, &moved_idx);
                        bump_persistent(&env, &moved_key, new_window);
                    }
                }

                let subscriber_for_pages = subscriber.clone();
                let moved_in_subscriber = paginated_remove(
                    &env,
                    |p| DataKey::SubscriberSubsPage(subscriber_for_pages.clone(), p),
                    &DataKey::SubscriberSubsCount(subscriber.clone()),
                    old_sub_id,
                    old_idx.subscriber_page,
                    new_window,
                );
                if let Some((moved_id, new_page)) = moved_in_subscriber {
                    let moved_key = DataKey::SubIndex(moved_id);
                    if let Some(mut moved_idx) =
                        env.storage().persistent().get::<_, SubIndex>(&moved_key)
                    {
                        moved_idx.subscriber_page = new_page;
                        env.storage().persistent().set(&moved_key, &moved_idx);
                        bump_persistent(&env, &moved_key, new_window);
                    }
                }

                env.storage().persistent().remove(&idx_key);
            }
            // The prior Subscription record itself is now unreachable from
            // any index — drop it so storage doesn't accumulate dead subs.
            env.storage().persistent().remove(&DataKey::Sub(old_sub_id));
            // LIB-8 (full fix): release the prior sub's reservation against
            // the on-chain allowance. cancel() / toggle_off() normally drain
            // both already, but a sub deactivated via process() charge
            // failure keeps its reservation (process can't approve without
            // subscriber auth). Re-subscribe is the next subscriber-authed
            // touch point, so this is where we honor the invariant: the
            // dead sub's share gets removed from the (subscriber, contract)
            // allowance before the new sub stacks its own share on top.
            release_sub_reservation(&env, &subscriber, old_sub_id);
        }

        // Append to subscriber's paginated index
        let subscriber_for_sub_pages = subscriber.clone();
        let subscriber_page = paginated_append(
            &env,
            |p| DataKey::SubscriberSubsPage(subscriber_for_sub_pages.clone(), p),
            &DataKey::SubscriberSubsCount(subscriber.clone()),
            sub_id,
            new_window,
        );

        // Append to service's paginated subscriber index
        let service_page = paginated_append(
            &env,
            |p| DataKey::ServiceSubsPage(service_id, p),
            &DataKey::ServiceSubsCount(service_id),
            sub_id,
            new_window,
        );

        // Reverse pointer so cancel/re-subscribe can locate this entry
        let idx_key = DataKey::SubIndex(sub_id);
        env.storage().persistent().set(
            &idx_key,
            &SubIndex {
                service_page,
                subscriber_page,
            },
        );
        bump_persistent(&env, &idx_key, new_window);

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
        if !sub.auto_renew {
            return Err(ContractError::AlreadyCancelled);
        }

        sub.auto_renew = false;
        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(
            &env,
            &sub_key,
            sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs),
        );

        // LIB-8 (full fix): reduce the on-chain allowance by exactly this
        // sub's tracked reservation. The token allowance is keyed by
        // (subscriber, contract), not by sub, so we can't just revoke the
        // whole entry without also tearing down billing for the user's other
        // live subs. The per-sub SubReservedAmount lets us subtract just this
        // sub's contribution; if that drains the entry it gets fully revoked,
        // otherwise the remaining budget for the user's other subs is left
        // intact.
        release_sub_reservation(&env, &subscriber, sub_id);

        bump_instance(&env);

        let now = env.ledger().timestamp();
        let remaining_secs = sub.service_end_ts.saturating_sub(now);

        env.events().publish(
            (symbol_short!("cancel"),),
            (subscriber, sub_id, sub.service_id, remaining_secs),
        );

        Ok(())
    }

    /// Toggle auto-renew on or off.  Cannot re-enable on an expired
    /// subscription.
    pub fn toggle_auto_renew(
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
        if !sub.auto_renew && now >= sub.service_end_ts {
            return Err(ContractError::SubscriptionExpired);
        }

        sub.auto_renew = !sub.auto_renew;

        let sub_window = sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs);
        env.storage().persistent().set(&sub_key, &sub);
        bump_persistent(&env, &sub_key, sub_window);

        if sub.auto_renew {
            // Re-enabling: refresh the token approval so process() can charge.
            let svc_key = DataKey::Service(sub.service_id);
            let service: Service = env
                .storage()
                .persistent()
                .get(&svc_key)
                .ok_or(ContractError::ServiceNotFound)?;

            do_approve(&env, &subscriber, sub_id, &service, service.approve_periods)?;
            bump_persistent(
                &env,
                &svc_key,
                sub_ttl_window_secs(service.period_secs, service.trial_period_secs),
            );
        } else {
            // LIB-8 (full fix): reduce the on-chain allowance by this sub's
            // tracked reservation. Per-sub bookkeeping means we no longer
            // leak the cancelled sub's budget when other auto-renewing subs
            // still depend on the same (subscriber, contract) allowance.
            release_sub_reservation(&env, &subscriber, sub_id);
        }

        bump_instance(&env);

        env.events().publish(
            (symbol_short!("renew"),),
            (subscriber, sub_id, sub.service_id, sub.auto_renew),
        );

        Ok(sub.auto_renew)
    }

    /// Extend an active subscription by refreshing the token approval.
    ///
    /// Call this when your allowance is running low and you want the
    /// subscription to continue renewing.  Sets `auto_renew` to `true`
    /// and approves the contract for `approve_periods` future periods.
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

        do_approve(&env, &subscriber, sub_id, &service, service.approve_periods)?;

        sub.auto_renew = true;
        env.storage().persistent().set(&sub_key, &sub);
        let svc_window = sub_ttl_window_secs(service.period_secs, service.trial_period_secs);
        bump_persistent(
            &env,
            &sub_key,
            sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs),
        );
        bump_persistent(&env, &svc_key, svc_window);
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
        offset: u32,
        limit: u32,
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
        let svc_window = sub_ttl_window_secs(service.period_secs, service.trial_period_secs);
        bump_persistent(&env, &svc_key, svc_window);

        let token = get_token(&env);
        let token_client = TokenClient::new(&env, &token);
        let contract_addr = env.current_contract_address();
        let now = env.ledger().timestamp();

        // Total live subs is tracked in a small companion key — we no longer
        // load the entire subscriber list to know its size.
        let count_key = DataKey::ServiceSubsCount(service_id);
        let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count > 0 {
            bump_persistent(&env, &count_key, svc_window);
        }
        let total: u32 = if count > u32::MAX as u64 {
            u32::MAX
        } else {
            count as u32
        };

        let start = offset.min(total);
        let end = start.saturating_add(limit).min(total);

        let mut charged: u32 = 0;
        let mut failed: u32 = 0;
        let mut skipped: u32 = 0;

        if end > start {
            let start_page = start / PAGE_SIZE;
            let last_page = (end - 1) / PAGE_SIZE;

            'pages: for p in start_page..=last_page {
                let page_key = DataKey::ServiceSubsPage(service_id, p);
                let page: Vec<u64> = match env.storage().persistent().get(&page_key) {
                    Some(v) => {
                        bump_persistent(&env, &page_key, svc_window);
                        v
                    }
                    None => break 'pages,
                };

                let page_base = p * PAGE_SIZE;
                let in_page_start = if p == start_page { start - page_base } else { 0 };
                let in_page_end =
                    core::cmp::min(page.len(), end.saturating_sub(page_base));

                for i in in_page_start..in_page_end {
                    let sid = page.get(i).unwrap();
                    let sub_key = DataKey::Sub(sid);

                    let mut sub: Subscription = match env.storage().persistent().get(&sub_key) {
                        Some(s) => s,
                        None => {
                            skipped += 1;
                            continue;
                        }
                    };

                    if !sub.auto_renew {
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

                        let new_next = match sub.next_charge_ts.checked_add(sub.period_secs) {
                            Some(ts) => ts,
                            None => {
                                // Overflow: disable auto-renew rather than reverting the batch
                                sub.auto_renew = false;
                                env.storage().persistent().set(&sub_key, &sub);
                                bump_persistent(
                                    &env,
                                    &sub_key,
                                    sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs),
                                );
                                failed += 1;
                                env.events().publish(
                                    (symbol_short!("chg_fail"),),
                                    (sub.subscriber.clone(), service_id, sub.sub_id),
                                );
                                continue;
                            }
                        };
                        sub.next_charge_ts = new_next;
                        sub.service_end_ts = new_next;
                        env.storage().persistent().set(&sub_key, &sub);
                        bump_persistent(
                            &env,
                            &sub_key,
                            sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs),
                        );

                        // LIB-8 (full fix): mirror the on-chain allowance
                        // debit by decrementing this sub's tracked
                        // reservation. Keeps cancel/toggle-off's reduction
                        // accurate as charges drain the budget. Clamps at
                        // zero — if external state ever drifts below the
                        // tracked value we don't want to roll negative.
                        let reserved_key = DataKey::SubReservedAmount(sid);
                        let cur_reserved: i128 = env
                            .storage()
                            .persistent()
                            .get(&reserved_key)
                            .unwrap_or(0);
                        let new_reserved = (cur_reserved - sub.price).max(0);
                        if new_reserved == 0 {
                            env.storage().persistent().remove(&reserved_key);
                        } else {
                            env.storage().persistent().set(&reserved_key, &new_reserved);
                            bump_persistent(
                                &env,
                                &reserved_key,
                                sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs),
                            );
                        }

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
                                (
                                    sub.subscriber.clone(),
                                    service_id,
                                    remaining_allowance,
                                    sub.price,
                                ),
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
                        // LIB-8 (full fix) limitation: process() runs under
                        // merchant auth, not subscriber auth, so we cannot
                        // call token.approve here to release the dead sub's
                        // SubReservedAmount share against the on-chain
                        // allowance. The reservation key lingers until the
                        // subscriber's next authed touch (cancel / toggle /
                        // extend / re-subscribe), at which point the
                        // matching path drains it.
                        sub.auto_renew = false;
                        env.storage().persistent().set(&sub_key, &sub);
                        bump_persistent(
                            &env,
                            &sub_key,
                            sub_ttl_window_secs(sub.period_secs, sub.trial_period_secs),
                        );
                        failed += 1;

                        env.events().publish(
                            (symbol_short!("chg_fail"),),
                            (sub.subscriber.clone(), service_id, sub.sub_id),
                        );
                    }
                }
            }
        }

        bump_instance(&env);

        Ok(ProcessResult {
            charged,
            failed,
            skipped,
            total,
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

        let subscriber_for_pages = subscriber.clone();
        let sub_ids = paginated_read_all(
            &env,
            |p| DataKey::SubscriberSubsPage(subscriber_for_pages.clone(), p),
            &DataKey::SubscriberSubsCount(subscriber),
        );

        let mut result: Vec<Subscription> = Vec::new(&env);
        for i in 0..sub_ids.len() {
            let sid = sub_ids.get(i).unwrap();
            if let Some(sub) =
                env.storage().persistent().get::<_, Subscription>(&DataKey::Sub(sid))
            {
                result.push_back(sub);
            }
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

        let sub_ids = paginated_read_all(
            &env,
            |p| DataKey::ServiceSubsPage(service_id, p),
            &DataKey::ServiceSubsCount(service_id),
        );

        let mut result: Vec<Subscription> = Vec::new(&env);
        for i in 0..sub_ids.len() {
            let sid = sub_ids.get(i).unwrap();
            if let Some(sub) =
                env.storage().persistent().get::<_, Subscription>(&DataKey::Sub(sid))
            {
                result.push_back(sub);
            }
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
        let merchant_for_pages = merchant.clone();
        let svc_ids = paginated_read_all(
            &env,
            |p| DataKey::MerchantServicesPage(merchant_for_pages.clone(), p),
            &DataKey::MerchantServicesCount(merchant),
        );

        let mut result: Vec<Service> = Vec::new(&env);
        for i in 0..svc_ids.len() {
            let sid = svc_ids.get(i).unwrap();
            if let Some(svc) =
                env.storage().persistent().get::<_, Service>(&DataKey::Service(sid))
            {
                result.push_back(svc);
            }
        }
        result
    }

    pub fn is_subscription_active(env: Env, subscriber: Address, service_id: u64) -> bool {
        let pair_key = DataKey::SubServicePair(subscriber, service_id);
        let sub_id: u64 = match env.storage().persistent().get(&pair_key) {
            Some(id) => id,
            None => return false,
        };
        let sub: Subscription = match env
            .storage()
            .persistent()
            .get(&DataKey::Sub(sub_id))
        {
            Some(s) => s,
            None => return false,
        };
        env.ledger().timestamp() < sub.service_end_ts
    }

    // ---- Admin ------------------------------------------------------------

    /// Propose a new admin. Two-step rotation: the proposed address still
    /// has to call `accept_admin` to take over, so a fat-fingered or
    /// uncontrolled address cannot strand the admin role.
    pub fn propose_admin(env: Env, new_admin: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("adm_prop"),), (admin, new_admin));
    }

    /// Complete admin rotation. The pending admin proves control of their
    /// address by signing this call; only after that does `Admin` flip.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), ContractError> {
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(ContractError::NoAdminProposed)?;

        if new_admin != pending {
            return Err(ContractError::NotProposedAdmin);
        }
        new_admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("adm_acc"),), new_admin);

        Ok(())
    }

    /// LIB-13: stage a contract upgrade. Records the candidate WASM hash
    /// alongside an `apply_after_ts` exactly `UPGRADE_TIMELOCK_SECS` into the
    /// future and emits `up_prop` so subscribers (who have approved this
    /// contract as a token spender) get a known window to revoke their
    /// allowances before any new bytecode runs. Rejects if a proposal is
    /// already pending — `cancel_upgrade()` must run first to replace it,
    /// which prevents a hostile or careless re-propose from silently
    /// resetting the timelock.
    pub fn propose_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
    ) -> Result<PendingUpgrade, ContractError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        if env.storage().instance().has(&DataKey::PendingUpgrade) {
            return Err(ContractError::UpgradeAlreadyPending);
        }

        let apply_after_ts = env
            .ledger()
            .timestamp()
            .checked_add(UPGRADE_TIMELOCK_SECS)
            .ok_or(ContractError::TimestampOverflow)?;

        let pending = PendingUpgrade {
            wasm_hash: new_wasm_hash,
            apply_after_ts,
        };
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgrade, &pending);
        bump_instance(&env);

        env.events().publish(
            (symbol_short!("up_prop"),),
            (admin, pending.wasm_hash.clone(), apply_after_ts),
        );

        Ok(pending)
    }

    /// LIB-13: apply the staged upgrade once the timelock has elapsed.
    /// Reverts if no proposal exists or if the timelock window has not
    /// passed yet, so the audit's compromised-admin path can never bypass
    /// the public delay.
    pub fn apply_upgrade(env: Env) -> Result<(), ContractError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(ContractError::NoUpgradeProposed)?;

        if env.ledger().timestamp() < pending.apply_after_ts {
            return Err(ContractError::UpgradeTimelockNotElapsed);
        }

        env.deployer()
            .update_current_contract_wasm(pending.wasm_hash.clone());
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("up_apply"),), pending.wasm_hash);

        Ok(())
    }

    /// LIB-13: drop a pending upgrade proposal. Lets the admin abort a
    /// proposal mid-timelock if a problem with the candidate WASM is
    /// found, and lets a freshly-rotated admin clear an inherited
    /// proposal they did not author.
    pub fn cancel_upgrade(env: Env) -> Result<(), ContractError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let pending: PendingUpgrade = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgrade)
            .ok_or(ContractError::NoUpgradeProposed)?;

        env.storage().instance().remove(&DataKey::PendingUpgrade);
        bump_instance(&env);

        env.events()
            .publish((symbol_short!("up_cancl"),), (admin, pending.wasm_hash));

        Ok(())
    }

    /// Read the currently-pending upgrade, if any. No auth — proposers,
    /// subscribers and off-chain watchers all have a legitimate reason to
    /// inspect the staged hash and earliest-apply timestamp.
    pub fn get_pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        env.storage().instance().get(&DataKey::PendingUpgrade)
    }

    pub fn version(_env: Env) -> u32 {
        1
    }
}

mod test;
