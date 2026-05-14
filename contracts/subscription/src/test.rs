#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{StellarAssetClient, TokenClient as TestTokenClient},
    Address, Env, String,
};

const DAY: u64 = 86_400;
const MONTH: u64 = 30 * DAY;
const WEEK: u64 = 7 * DAY;
const PRICE: i128 = 1_000;
const INITIAL_BALANCE: i128 = 100_000;

#[allow(dead_code)]
struct Setup<'a> {
    env: Env,
    client: SubscriptionContractClient<'a>,
    contract_addr: Address,
    admin: Address,
    subscriber: Address,
    subscriber2: Address,
    merchant: Address,
    merchant2: Address,
    token: TestTokenClient<'a>,
    token_admin: StellarAssetClient<'a>,
    token_addr: Address,
}

fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let token_admin_addr = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin_addr.clone());
    let token_addr = sac.address();
    let token = TestTokenClient::new(&env, &token_addr);
    let token_admin = StellarAssetClient::new(&env, &token_addr);

    let contract_id = env.register(SubscriptionContract, (&admin, &token_addr));
    let client = SubscriptionContractClient::new(&env, &contract_id);

    let subscriber = Address::generate(&env);
    let subscriber2 = Address::generate(&env);
    let merchant = Address::generate(&env);
    let merchant2 = Address::generate(&env);

    // Fund subscribers
    token_admin.mint(&subscriber, &INITIAL_BALANCE);
    token_admin.mint(&subscriber2, &INITIAL_BALANCE);

    Setup {
        env,
        client,
        contract_addr: contract_id,
        admin,
        subscriber,
        subscriber2,
        merchant,
        merchant2,
        token,
        token_admin,
        token_addr,
    }
}

fn advance_time(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp = timestamp;
    });
}

fn register_default_service(s: &Setup) -> Service {
    s.client.register_service(
        &s.merchant,
        &String::from_str(&s.env, "Premium Plan"),
        &PRICE,
        &MONTH,
        &0,
        &12,
    )
}

fn register_trial_service(s: &Setup) -> Service {
    s.client.register_service(
        &s.merchant,
        &String::from_str(&s.env, "Trial Plan"),
        &PRICE,
        &MONTH,
        &WEEK,
        &12,
    )
}

// ===========================================================================
// Service Registration
// ===========================================================================

#[test]
fn test_register_service() {
    let s = setup();
    let svc = register_default_service(&s);

    assert_eq!(svc.service_id, 0);
    assert_eq!(svc.merchant, s.merchant);
    assert_eq!(svc.name, String::from_str(&s.env, "Premium Plan"));
    assert_eq!(svc.price, PRICE);
    assert_eq!(svc.period_secs, MONTH);
    assert_eq!(svc.trial_period_secs, 0);
    assert_eq!(svc.is_active, true);
}

#[test]
fn test_register_service_invalid_price() {
    let s = setup();
    let result = s.client.try_register_service(
        &s.merchant,
        &String::from_str(&s.env, "Bad"),
        &0,
        &MONTH,
        &0,
        &12,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidPrice)));
}

#[test]
fn test_register_service_invalid_period() {
    let s = setup();
    let result = s.client.try_register_service(
        &s.merchant,
        &String::from_str(&s.env, "Bad"),
        &PRICE,
        &0,
        &0,
        &12,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidPeriod)));
}

#[test]
fn test_register_service_invalid_name() {
    let s = setup();
    let result = s.client.try_register_service(
        &s.merchant,
        &String::from_str(&s.env, ""),
        &PRICE,
        &MONTH,
        &0,
        &12,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidServiceName)));
}

#[test]
fn test_register_multiple_services() {
    let s = setup();
    let svc1 = register_default_service(&s);
    let svc2 = s.client.register_service(
        &s.merchant,
        &String::from_str(&s.env, "Basic Plan"),
        &500,
        &WEEK,
        &0,
        &12,
    );

    assert_eq!(svc1.service_id, 0);
    assert_eq!(svc2.service_id, 1);

    let services = s.client.get_merchant_services(&s.merchant);
    assert_eq!(services.len(), 2);
}

#[test]
fn test_get_service() {
    let s = setup();
    let svc = register_default_service(&s);
    let fetched = s.client.get_service(&svc.service_id);
    assert_eq!(fetched, svc);
}

#[test]
fn test_get_service_not_found() {
    let s = setup();
    let result = s.client.try_get_service(&99);
    assert_eq!(result, Err(Ok(ContractError::ServiceNotFound)));
}

// ===========================================================================
// Service activation (LIB-12)
// ===========================================================================

#[test]
fn test_set_service_active_blocks_new_subscriptions() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.set_service_active(&s.merchant, &svc.service_id, &false);

    let result = s
        .client
        .try_subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(result, Err(Ok(ContractError::ServiceNotActive)));
}

#[test]
fn test_set_service_active_reactivation_allows_subscriptions() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.set_service_active(&s.merchant, &svc.service_id, &false);
    s.client.set_service_active(&s.merchant, &svc.service_id, &true);

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(sub.auto_renew, true);
}

#[test]
fn test_set_service_active_only_service_owner() {
    let s = setup();
    let svc = register_default_service(&s);

    let result = s
        .client
        .try_set_service_active(&s.merchant2, &svc.service_id, &false);
    assert_eq!(result, Err(Ok(ContractError::NotServiceOwner)));
}

#[test]
fn test_set_service_active_nonexistent_service() {
    let s = setup();
    let result = s.client.try_set_service_active(&s.merchant, &99, &false);
    assert_eq!(result, Err(Ok(ContractError::ServiceNotFound)));
}

#[test]
fn test_set_service_active_does_not_affect_existing_subs() {
    // Existing subscriptions keep their access through service_end_ts and
    // continue to be billable by process(). Deactivation is a sign-up gate,
    // not a kill-switch for outstanding obligations.
    let s = setup();
    let svc = register_default_service(&s);
    let _sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    s.client.set_service_active(&s.merchant, &svc.service_id, &false);

    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );

    advance_time(&s.env, MONTH + 1);
    let r = s.client.process(&s.merchant, &svc.service_id, &0, &10);
    assert_eq!(r.charged, 1);
    assert_eq!(r.failed, 0);
}

// ===========================================================================
// Subscribe
// ===========================================================================

#[test]
fn test_subscribe_no_trial_auto_renew() {
    let s = setup();
    let svc = register_default_service(&s);

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    assert_eq!(sub.auto_renew, true);
    assert_eq!(sub.price, PRICE);
    assert_eq!(sub.period_secs, MONTH);
    assert_eq!(sub.trial_period_secs, 0);
    assert_eq!(sub.trial_end_ts, 0);
    assert_eq!(sub.service_end_ts, MONTH);
    assert_eq!(sub.next_charge_ts, MONTH);

    // Immediate charge happened
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
    assert_eq!(s.token.balance(&s.merchant), PRICE);

    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );
}

#[test]
fn test_subscribe_no_trial_no_auto_renew() {
    let s = setup();
    let svc = register_default_service(&s);

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &false);

    assert_eq!(sub.auto_renew, false);
    assert_eq!(sub.price, PRICE);

    // Immediate charge still happens for first period
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);

    // Service is active during period
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );

    // Won't renew after period ends
    advance_time(&s.env, MONTH + 1);
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        false
    );
}

#[test]
fn test_subscribe_with_trial_auto_renew() {
    let s = setup();
    let svc = register_trial_service(&s);

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    assert_eq!(sub.auto_renew, true);
    assert_eq!(sub.trial_period_secs, WEEK);
    assert_eq!(sub.trial_end_ts, WEEK);
    assert_eq!(sub.service_end_ts, WEEK);
    assert_eq!(sub.next_charge_ts, WEEK);

    // No charge during trial
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE);
    assert_eq!(s.token.balance(&s.merchant), 0);

    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );
}

#[test]
fn test_subscribe_with_trial_no_auto_renew() {
    let s = setup();
    let svc = register_trial_service(&s);

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &false);

    assert_eq!(sub.auto_renew, false);
    assert_eq!(sub.trial_period_secs, WEEK);
    assert_eq!(sub.trial_end_ts, WEEK);
    assert_eq!(sub.service_end_ts, WEEK);

    // No charge, no approval – trial only
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE);

    // Active during trial
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );

    // Expires after trial
    advance_time(&s.env, WEEK + 1);
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        false
    );

    // Process won't charge since auto_renew=false
    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 0);
    assert_eq!(result.skipped, 1);
}

#[test]
fn test_subscribe_trial_abuse_blocked() {
    let s = setup();
    let svc = register_trial_service(&s);

    // First trial subscription (no auto_renew) — free trial, no charge
    s.client.subscribe(&s.subscriber, &svc.service_id, &false);
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE);

    // Wait for trial to expire
    advance_time(&s.env, WEEK + 1);
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        false
    );

    // Re-subscribing without auto_renew is allowed but no second trial:
    // contract takes the paid path and charges immediately.
    let sub2 = s.client.subscribe(&s.subscriber, &svc.service_id, &false);
    assert_eq!(sub2.trial_period_secs, 0);
    assert_eq!(sub2.trial_end_ts, 0);
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
}

#[test]
fn test_subscribe_trial_abuse_blocked_via_auto_renew_cycle() {
    // LIB-5 regression: subscribe(true) → cancel during trial → wait for
    // expiry → subscribe(true) used to grant a fresh trial because the
    // had_trial guard only fired when auto_renew was false. After the fix
    // the second subscription must take the paid path.
    let s = setup();
    let svc = register_trial_service(&s);

    let sub1 = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(sub1.trial_period_secs, WEEK);

    // Cancel during the trial — keeps trial active but disables auto-renew
    s.client.cancel(&s.subscriber, &sub1.sub_id);

    // Trial expires
    advance_time(&s.env, WEEK + 1);

    // Second subscribe with auto_renew=true must NOT grant another trial
    let sub2 = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(sub2.auto_renew, true);
    assert_eq!(sub2.trial_period_secs, 0);
    assert_eq!(sub2.trial_end_ts, 0);

    // Paid path: first period charged immediately
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
}

#[test]
fn test_subscribe_multiple_services() {
    let s = setup();
    let svc1 = register_default_service(&s);
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Other Plan"),
        &500,
        &WEEK,
        &0,
        &12,
    );

    let sub1 = s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    let sub2 = s.client.subscribe(&s.subscriber, &svc2.service_id, &true);

    assert_eq!(sub1.service_id, svc1.service_id);
    assert_eq!(sub2.service_id, svc2.service_id);
    assert_eq!(sub1.price, PRICE);
    assert_eq!(sub2.price, 500);

    let subs = s.client.get_subscriber_subs(&s.subscriber);
    assert_eq!(subs.len(), 2);
}

#[test]
fn test_subscribe_duplicate_rejected() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let result = s
        .client
        .try_subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(result, Err(Ok(ContractError::AlreadySubscribed)));
}

#[test]
fn test_subscribe_nonexistent_service() {
    let s = setup();
    let result = s.client.try_subscribe(&s.subscriber, &99, &true);
    assert_eq!(result, Err(Ok(ContractError::ServiceNotFound)));
}

#[test]
fn test_subscribe_resubscribe_after_expiry() {
    let s = setup();
    let svc = register_default_service(&s);

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    s.client.cancel(&s.subscriber, &sub.sub_id);

    // Advance past service_end_ts
    advance_time(&s.env, MONTH + 1);

    // Re-subscribe should work
    let sub2 = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(sub2.auto_renew, true);
    assert!(sub2.sub_id != sub.sub_id);
}

// ===========================================================================
// Cancel
// ===========================================================================

#[test]
fn test_cancel_subscription() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    advance_time(&s.env, 15 * DAY);
    s.client.cancel(&s.subscriber, &sub.sub_id);

    let updated = s.client.get_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(updated.auto_renew, false);
    assert_eq!(updated.service_end_ts, MONTH);

    // Service still active mid-period
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );

    // Service inactive after period ends
    advance_time(&s.env, MONTH + 1);
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        false
    );
}

#[test]
fn test_cancel_nonexistent() {
    let s = setup();
    let result = s.client.try_cancel(&s.subscriber, &999);
    assert_eq!(result, Err(Ok(ContractError::SubscriptionNotFound)));
}

#[test]
fn test_cancel_already_cancelled() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    s.client.cancel(&s.subscriber, &sub.sub_id);

    let result = s.client.try_cancel(&s.subscriber, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::AlreadyCancelled)));
}

#[test]
fn test_cancel_wrong_subscriber() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let result = s.client.try_cancel(&s.subscriber2, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_cancel_during_trial() {
    let s = setup();
    let svc = register_trial_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    advance_time(&s.env, 3 * DAY);
    s.client.cancel(&s.subscriber, &sub.sub_id);

    // Service still active during trial
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        true
    );

    // Service inactive after trial ends
    advance_time(&s.env, WEEK + 1);
    assert_eq!(
        s.client
            .is_subscription_active(&s.subscriber, &svc.service_id),
        false
    );

    // No charge ever happened
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE);
}

// ===========================================================================
// Toggle Pay-Upfront
// ===========================================================================

#[test]
fn test_toggle_auto_renew_off() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    advance_time(&s.env, 10 * DAY);

    let result = s.client.toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert_eq!(result, false);

    let updated = s.client.get_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(updated.auto_renew, false);
}

#[test]
fn test_toggle_auto_renew_back_on() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    advance_time(&s.env, 10 * DAY);

    // Toggle off
    let r1 = s.client.toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert_eq!(r1, false);

    // Toggle back on (still within service_end_ts)
    let r2 = s.client.toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert_eq!(r2, true);

    let updated = s.client.get_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(updated.auto_renew, true);
}

#[test]
fn test_toggle_auto_renew_expired_fails() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Cancel then let it expire
    s.client.cancel(&s.subscriber, &sub.sub_id);
    advance_time(&s.env, MONTH + 1);

    // Cannot re-enable on expired sub
    let result = s
        .client
        .try_toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::SubscriptionExpired)));
}

#[test]
fn test_toggle_auto_renew_wrong_subscriber() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let result = s
        .client
        .try_toggle_auto_renew(&s.subscriber2, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ===========================================================================
// Extend Subscription
// ===========================================================================

#[test]
fn test_extend_subscription() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    advance_time(&s.env, 10 * DAY);

    let extended = s
        .client
        .extend_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(extended.auto_renew, true);
    assert_eq!(extended.sub_id, sub.sub_id);
}

#[test]
fn test_extend_subscription_reactivates_auto_renew() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Cancel (sets auto_renew to false)
    s.client.cancel(&s.subscriber, &sub.sub_id);
    let cancelled = s.client.get_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(cancelled.auto_renew, false);

    // Extend while still within service period
    advance_time(&s.env, 10 * DAY);
    let extended = s
        .client
        .extend_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(extended.auto_renew, true);
}

#[test]
fn test_extend_subscription_from_no_auto_renew() {
    let s = setup();
    let svc = register_default_service(&s);

    // Subscribe without auto_renew (one-time)
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &false);
    assert_eq!(sub.auto_renew, false);

    // Decide to continue: extend mid-period
    advance_time(&s.env, 10 * DAY);
    let extended = s
        .client
        .extend_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(extended.auto_renew, true);

    // Now process() can charge after current period ends
    s.token
        .approve(&s.subscriber, &s.contract_addr, &INITIAL_BALANCE, &10000);
    advance_time(&s.env, MONTH + 1);
    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 1);
}

#[test]
fn test_extend_subscription_expired_fails() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    s.client.cancel(&s.subscriber, &sub.sub_id);
    advance_time(&s.env, MONTH + 1);

    let result = s
        .client
        .try_extend_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::SubscriptionExpired)));
}

#[test]
fn test_extend_subscription_wrong_subscriber() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let result = s
        .client
        .try_extend_subscription(&s.subscriber2, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ===========================================================================
// Process
// ===========================================================================

#[test]
fn test_process_single_charge() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Set explicit approval for process transfer_from
    s.token
        .approve(&s.subscriber, &s.contract_addr, &INITIAL_BALANCE, &10000);

    advance_time(&s.env, MONTH + 1);

    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 1);
    assert_eq!(result.failed, 0);
    assert_eq!(result.skipped, 0);

    // Two charges: subscribe + renewal
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - 2 * PRICE);
    assert_eq!(s.token.balance(&s.merchant), 2 * PRICE);
}

#[test]
fn test_process_batch_multiple_subscribers() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    s.client.subscribe(&s.subscriber2, &svc.service_id, &true);

    s.token
        .approve(&s.subscriber, &s.contract_addr, &INITIAL_BALANCE, &10000);
    s.token
        .approve(&s.subscriber2, &s.contract_addr, &INITIAL_BALANCE, &10000);

    advance_time(&s.env, MONTH + 1);

    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 2);
    assert_eq!(result.failed, 0);
    assert_eq!(result.skipped, 0);
}

#[test]
fn test_process_insufficient_funds() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Drain subscriber balance so renewal fails
    let remaining = s.token.balance(&s.subscriber);
    s.token.transfer(&s.subscriber, &s.admin, &remaining);
    assert_eq!(s.token.balance(&s.subscriber), 0);

    advance_time(&s.env, MONTH + 1);

    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 0);
    assert_eq!(result.failed, 1);
    assert_eq!(result.skipped, 0);

    let sub = s.client.get_subscription(&s.subscriber, &0);
    assert_eq!(sub.auto_renew, false);
}

#[test]
fn test_process_before_due() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Don't advance time
    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.skipped, 1);

    // Only the initial subscribe charge
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
}

#[test]
fn test_process_no_drift() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    s.token
        .approve(&s.subscriber, &s.contract_addr, &INITIAL_BALANCE, &10000);

    // Advance 5 days past due
    advance_time(&s.env, MONTH + 5 * DAY);

    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 1);

    // next_charge_ts = old_next_charge_ts + period, not now + period
    let sub = s.client.get_subscription(&s.subscriber, &0);
    assert_eq!(sub.next_charge_ts, MONTH + MONTH);
    assert_eq!(sub.service_end_ts, MONTH + MONTH);
}

#[test]
fn test_process_wrong_merchant() {
    let s = setup();
    let svc = register_default_service(&s);
    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let result = s.client.try_process(&s.merchant2, &svc.service_id, &0, &100);
    assert_eq!(result, Err(Ok(ContractError::NotServiceOwner)));
}

#[test]
fn test_process_trial_expiry_and_first_charge() {
    let s = setup();
    let svc = register_trial_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // No charge during trial
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE);

    s.token
        .approve(&s.subscriber, &s.contract_addr, &INITIAL_BALANCE, &10000);

    // Advance past trial
    advance_time(&s.env, WEEK + 1);

    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 1);

    let sub = s.client.get_subscription(&s.subscriber, &0);
    assert_eq!(sub.next_charge_ts, WEEK + MONTH);
    assert_eq!(sub.service_end_ts, WEEK + MONTH);
    assert_eq!(sub.auto_renew, true);

    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
    assert_eq!(s.token.balance(&s.merchant), PRICE);
}

#[test]
fn test_sub_ttl_window_secs_covers_trial_window() {
    // LIB-7 unit check: the window helper must reduce to `2 * period_secs`
    // for non-trial subs (preserving prior behavior) and grow to
    // `trial_period_secs + period_secs` once a trial is configured, so the
    // first post-trial process() call always finds the storage alive.

    // Non-trial: identical to the pre-fix behavior of `period_secs * 2`.
    assert_eq!(sub_ttl_window_secs(DAY, 0), 2 * DAY);
    assert_eq!(sub_ttl_window_secs(MONTH, 0), 2 * MONTH);

    // Trial shorter than period: still bounded by `2 * period_secs`.
    assert_eq!(sub_ttl_window_secs(MONTH, WEEK), 2 * MONTH);

    // Trial == period: degenerates to `2 * period_secs`.
    assert_eq!(sub_ttl_window_secs(WEEK, WEEK), 2 * WEEK);

    // Trial much longer than period: window covers trial + one billing slack.
    let long_trial = 90 * DAY;
    assert_eq!(sub_ttl_window_secs(DAY, long_trial), long_trial + DAY);
    assert!(sub_ttl_window_secs(DAY, long_trial) > 2 * DAY);

    // Saturating add: extreme values must not panic.
    assert_eq!(sub_ttl_window_secs(u64::MAX, u64::MAX), u64::MAX);
}

#[test]
fn test_long_trial_state_survives_until_first_charge() {
    // LIB-7 regression: bump_persistent used to derive TTL from period_secs
    // alone, so a long trial (e.g. 90 days) with a short billing period (1
    // day) could see Sub / SubServicePair / Service / ServiceSubs expire
    // before the first post-trial process() call ever landed — silently
    // killing the subscription before its first paid charge. After the fix,
    // sub_ttl_window_secs covers the trial window plus one billing period of
    // slack, so all sub-scoped keys must survive the entire trial.
    let s = setup();

    let period = DAY;
    let trial = 90 * DAY;

    let svc = s.client.register_service(
        &s.merchant,
        &String::from_str(&s.env, "Long Trial"),
        &PRICE,
        &period,
        &trial,
        &12,
    );

    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Advance the ledger past the OLD `2 * period_secs` TTL window (~2 days
    // in ledgers) but stay well within the trial. Without the fix every
    // sub-scoped key would have expired here; with it they all survive.
    const TEST_SECS_PER_LEDGER: u64 = 5;
    let old_ttl_ledgers = (2 * period / TEST_SECS_PER_LEDGER) as u32;
    let advance_ledgers = old_ttl_ledgers.saturating_add(50_000);
    s.env.ledger().with_mut(|li| {
        li.sequence_number = li.sequence_number.saturating_add(advance_ledgers);
        li.timestamp += trial;
    });

    let fetched = s.client.get_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(fetched.sub_id, sub.sub_id);
    let fetched_svc = s.client.get_service(&svc.service_id);
    assert_eq!(fetched_svc.service_id, svc.service_id);

    // First post-trial charge succeeds — proves ServiceSubs page survived too.
    let result = s.client.process(&s.merchant, &svc.service_id, &0, &10);
    assert_eq!(result.charged, 1);
    assert_eq!(result.failed, 0);
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
}

#[test]
fn test_process_skips_no_auto_renew() {
    let s = setup();
    let svc = register_default_service(&s);

    // Subscribe without auto_renew
    s.client.subscribe(&s.subscriber, &svc.service_id, &false);

    advance_time(&s.env, MONTH + 1);

    let result = s.client.process(&s.merchant, &svc.service_id, &0, &100);
    assert_eq!(result.charged, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.skipped, 1);

    // Only the initial charge, no renewal
    assert_eq!(s.token.balance(&s.subscriber), INITIAL_BALANCE - PRICE);
}

// ===========================================================================
// Access Control
// ===========================================================================

#[test]
fn test_get_subscription_by_subscriber() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let fetched = s.client.get_subscription(&s.subscriber, &sub.sub_id);
    assert_eq!(fetched, sub);
}

#[test]
fn test_get_subscription_by_merchant() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let fetched = s.client.get_subscription(&s.merchant, &sub.sub_id);
    assert_eq!(fetched, sub);
}

#[test]
fn test_get_subscription_unauthorized() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let result = s
        .client
        .try_get_subscription(&s.subscriber2, &sub.sub_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_get_subscriber_subs() {
    let s = setup();
    let svc1 = register_default_service(&s);
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Other"),
        &500,
        &WEEK,
        &0,
        &12,
    );

    s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    s.client.subscribe(&s.subscriber, &svc2.service_id, &true);

    let subs = s.client.get_subscriber_subs(&s.subscriber);
    assert_eq!(subs.len(), 2);
}

#[test]
fn test_get_merchant_subs() {
    let s = setup();
    let svc = register_default_service(&s);

    s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    s.client.subscribe(&s.subscriber2, &svc.service_id, &true);

    let subs = s.client.get_merchant_subs(&s.merchant, &svc.service_id);
    assert_eq!(subs.len(), 2);
}

#[test]
fn test_get_merchant_subs_wrong_merchant() {
    let s = setup();
    let svc = register_default_service(&s);

    let result = s
        .client
        .try_get_merchant_subs(&s.merchant2, &svc.service_id);
    assert_eq!(result, Err(Ok(ContractError::NotServiceOwner)));
}

// ===========================================================================
// is_subscription_active
// ===========================================================================

#[test]
fn test_is_subscription_active_no_subscription() {
    let s = setup();
    let random = Address::generate(&s.env);
    assert_eq!(s.client.is_subscription_active(&random, &0), false);
}

// ===========================================================================
// Admin
// ===========================================================================

mod upgrade_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/subscription.wasm");
}

#[test]
fn test_upgrade() {
    let s = setup();
    let wasm_hash = s.env.deployer().upload_contract_wasm(upgrade_wasm::WASM);
    s.client.upgrade(&wasm_hash);
    assert_eq!(s.client.version(), 1);
}

#[test]
fn test_version() {
    let s = setup();
    assert_eq!(s.client.version(), 1);
}

// ===========================================================================
// Admin rotation (LIB-11: two-step admin transfer)
// ===========================================================================

#[test]
fn test_admin_rotation_two_step_completes() {
    let s = setup();
    let new_admin = Address::generate(&s.env);

    // Stage 1: current admin proposes a successor — Admin field unchanged.
    s.client.propose_admin(&new_admin);

    // Stage 2: proposed address signs accept_admin to take over. After this,
    // upgrade authority sits with new_admin (verified below by upgrading).
    s.client.accept_admin(&new_admin);

    // Confirm new_admin actually controls upgrade now: the call would
    // panic if Admin still pointed at the old address (mock_all_auths
    // doesn't relax the storage-equality the contract reads).
    let wasm_hash = s.env.deployer().upload_contract_wasm(upgrade_wasm::WASM);
    s.client.upgrade(&wasm_hash);
}

#[test]
fn test_accept_admin_without_proposal_fails() {
    let s = setup();
    let result = s.client.try_accept_admin(&s.admin);
    assert_eq!(result, Err(Ok(ContractError::NoAdminProposed)));
}

#[test]
fn test_accept_admin_wrong_address_fails() {
    let s = setup();
    let proposed = Address::generate(&s.env);
    let imposter = Address::generate(&s.env);

    s.client.propose_admin(&proposed);

    let result = s.client.try_accept_admin(&imposter);
    assert_eq!(result, Err(Ok(ContractError::NotProposedAdmin)));
}

#[test]
fn test_repropose_overrides_pending() {
    // Calling propose_admin a second time replaces the pending address —
    // the original proposed address can no longer accept.
    let s = setup();
    let first = Address::generate(&s.env);
    let second = Address::generate(&s.env);

    s.client.propose_admin(&first);
    s.client.propose_admin(&second);

    let stale = s.client.try_accept_admin(&first);
    assert_eq!(stale, Err(Ok(ContractError::NotProposedAdmin)));

    s.client.accept_admin(&second);
}

#[test]
fn test_old_admin_loses_role_after_rotation() {
    // After rotation, calling propose_admin from the old admin's address
    // would still succeed in mock_all_auths (since it mocks all auths),
    // but the contract reads Admin from storage — so the *event* attributes
    // the call to whoever is now in storage. Verify by rotating once,
    // then doing a fresh rotation initiated by the new admin.
    let s = setup();
    let new_admin = Address::generate(&s.env);

    s.client.propose_admin(&new_admin);
    s.client.accept_admin(&new_admin);

    // new_admin now has rotation authority — proposing again works.
    let third = Address::generate(&s.env);
    s.client.propose_admin(&third);
    s.client.accept_admin(&third);

    // Sanity: third can upgrade.
    let wasm_hash = s.env.deployer().upload_contract_wasm(upgrade_wasm::WASM);
    s.client.upgrade(&wasm_hash);
}

// ===========================================================================
// Timestamp overflow (H-3 fix)
// ===========================================================================

#[test]
fn test_timestamp_overflow() {
    let s = setup();
    let svc = register_default_service(&s);

    // Advance to near u64::MAX
    advance_time(&s.env, u64::MAX - 100);

    // Subscribe should fail with overflow since now + period_secs overflows
    let result = s
        .client
        .try_subscribe(&s.subscriber, &svc.service_id, &true);
    assert_eq!(result, Err(Ok(ContractError::TimestampOverflow)));
}

// ===========================================================================
// Pagination (LIB-6: paginated indexes + stale-entry cleanup)
// ===========================================================================
//
// PAGE_SIZE in the contract is 50; tests use 55 entries to span exactly two
// pages, which is enough to exercise the page-boundary code paths without
// blowing up test setup time.

const PAGINATION_TEST_N: u32 = 55;

fn fund_and_subscribe(s: &Setup, service_id: u64) -> Address {
    let a = Address::generate(&s.env);
    s.token_admin.mint(&a, &INITIAL_BALANCE);
    s.client.subscribe(&a, &service_id, &true);
    a
}

#[test]
fn test_pagination_get_merchant_subs_across_pages() {
    let s = setup();
    let svc = register_default_service(&s);

    for _ in 0..PAGINATION_TEST_N {
        fund_and_subscribe(&s, svc.service_id);
    }

    let result = s.client.get_merchant_subs(&s.merchant, &svc.service_id);
    assert_eq!(result.len(), PAGINATION_TEST_N);
}

#[test]
fn test_pagination_get_subscriber_subs_across_pages() {
    let s = setup();

    // Single subscriber subscribed to many services — exercises
    // SubscriberSubs spanning multiple pages.
    let mut svc_ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&s.env);
    for _ in 0..PAGINATION_TEST_N {
        let svc = s.client.register_service(
            &s.merchant,
            &String::from_str(&s.env, "Plan"),
            &PRICE,
            &MONTH,
            &0,
            &12,
        );
        svc_ids.push_back(svc.service_id);
    }
    for i in 0..svc_ids.len() {
        s.client.subscribe(&s.subscriber, &svc_ids.get(i).unwrap(), &true);
    }

    let result = s.client.get_subscriber_subs(&s.subscriber);
    assert_eq!(result.len(), PAGINATION_TEST_N);
}

#[test]
fn test_pagination_get_merchant_services_across_pages() {
    let s = setup();

    for _ in 0..PAGINATION_TEST_N {
        s.client.register_service(
            &s.merchant,
            &String::from_str(&s.env, "Plan"),
            &PRICE,
            &MONTH,
            &0,
            &12,
        );
    }

    let result = s.client.get_merchant_services(&s.merchant);
    assert_eq!(result.len(), PAGINATION_TEST_N);
}

#[test]
fn test_pagination_process_iterates_across_pages() {
    let s = setup();
    let svc = register_default_service(&s);

    for _ in 0..PAGINATION_TEST_N {
        fund_and_subscribe(&s, svc.service_id);
    }

    advance_time(&s.env, MONTH + 1);

    let r = s.client.process(&s.merchant, &svc.service_id, &0, &PAGINATION_TEST_N);
    assert_eq!(r.charged, PAGINATION_TEST_N);
    assert_eq!(r.total, PAGINATION_TEST_N);
}

#[test]
fn test_pagination_process_offset_in_second_page() {
    let s = setup();
    let svc = register_default_service(&s);

    for _ in 0..PAGINATION_TEST_N {
        fund_and_subscribe(&s, svc.service_id);
    }

    advance_time(&s.env, MONTH + 1);

    // Process the first page only
    let r1 = s.client.process(&s.merchant, &svc.service_id, &0, &50);
    assert_eq!(r1.charged, 50);
    assert_eq!(r1.total, PAGINATION_TEST_N);

    // Process the tail (offset 50 lands on the second page).
    // Limit larger than remaining; should charge only what's left.
    let r2 = s.client.process(&s.merchant, &svc.service_id, &50, &50);
    assert_eq!(r2.charged, 5);
    assert_eq!(r2.total, PAGINATION_TEST_N);
    assert_eq!(r2.skipped, 0);
}

#[test]
fn test_resubscribe_prunes_stale_index_entries() {
    // LIB-6 regression: re-subscribe must remove the dead prior sub_id from
    // ServiceSubs/SubscriberSubs, otherwise the indexes grow unboundedly
    // across subscribe→cancel→expire→subscribe cycles.
    let s = setup();
    let svc = register_default_service(&s);

    let cycles: u64 = 5;
    for i in 0..cycles {
        let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
        s.client.cancel(&s.subscriber, &sub.sub_id);
        advance_time(&s.env, (i + 1) * (MONTH + 1));
    }

    // Despite 5 cycles, only the most recent (now-expired) sub remains
    // in either index — the prior 4 sub_ids were pruned on re-subscribe.
    let merchant_subs = s.client.get_merchant_subs(&s.merchant, &svc.service_id);
    assert_eq!(merchant_subs.len(), 1);

    let subscriber_subs = s.client.get_subscriber_subs(&s.subscriber);
    assert_eq!(subscriber_subs.len(), 1);
}

// ===========================================================================
// Allowance revocation (LIB-8: cancel/toggle clear the on-chain allowance)
// ===========================================================================

#[test]
fn test_cancel_revokes_allowance_for_single_sub() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    // Subscribe with auto_renew=true approves the contract for future periods.
    let allowance_before = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert!(allowance_before > 0);

    s.client.cancel(&s.subscriber, &sub.sub_id);

    // Cancellation must zero out the allowance — this is the cancellation
    // boundary on chain.
    let allowance_after = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after, 0);
}

#[test]
fn test_cancel_reduces_allowance_by_only_cancelled_sub_share() {
    // LIB-8 (full fix) regression: allowance is keyed by (subscriber,
    // contract), not by service. The earlier partial fix preserved the FULL
    // aggregate allowance whenever any other auto-renewing sub remained,
    // leaving the cancelled sub's budget on chain and usable by future
    // logic / a bad upgrade. The full fix tracks each sub's contribution
    // (SubReservedAmount) and reduces the on-chain allowance by exactly
    // that share — so the cancelled sub's budget goes away while the other
    // sub's budget survives intact.
    let s = setup();
    let svc1 = register_default_service(&s); // PRICE=1000, 12 periods => 12_000
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Other Plan"),
        &500,
        &WEEK,
        &0,
        &12,
    ); // 500 * 12 => 6_000

    let sub1 = s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    let _sub2 = s.client.subscribe(&s.subscriber, &svc2.service_id, &true);

    let allowance_after_both = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after_both, 12 * PRICE + 12 * 500); // 18_000

    s.client.cancel(&s.subscriber, &sub1.sub_id);

    // Cancelled sub1's 12_000 budget must be removed; sub2's 6_000 must
    // remain so its future renewals continue working.
    let allowance_after_cancel = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after_cancel, 12 * 500);
}

#[test]
fn test_toggle_auto_renew_off_revokes_allowance() {
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    let new_state = s.client.toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert_eq!(new_state, false);

    let allowance = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance, 0);
}

#[test]
fn test_subscribe_aggregates_allowance_across_services() {
    // LIB-9: subscribing to a second service must add its budget on top of
    // the existing allowance, not overwrite it.
    let s = setup();
    let svc1 = register_default_service(&s); // PRICE=1000, periods=12
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Cheap"),
        &10,
        &MONTH,
        &0,
        &12,
    );

    s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    let after_sub1 = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(after_sub1, 12 * PRICE); // sub1's full budget

    s.client.subscribe(&s.subscriber, &svc2.service_id, &true);
    let after_sub2 = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(after_sub2, 12 * PRICE + 12 * 10); // sum of both budgets
}

#[test]
fn test_subscribe_to_cheaper_service_does_not_break_existing() {
    // LIB-9 regression: under the old overwrite semantics, subscribing to a
    // tiny-priced service after a large-priced one would shrink the
    // allowance to the small budget (12 * 10 = 120) and the next process()
    // for the large sub (needs 1000) would fail and disable it.
    let s = setup();
    let svc1 = register_default_service(&s);
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Cheap"),
        &10,
        &MONTH,
        &0,
        &12,
    );

    s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    s.client.subscribe(&s.subscriber, &svc2.service_id, &true);

    advance_time(&s.env, MONTH + 1);

    // sub1 must still be billable. With the bug, this would charged=0 / failed=1.
    let r1 = s.client.process(&s.merchant, &svc1.service_id, &0, &10);
    assert_eq!(r1.charged, 1);
    assert_eq!(r1.failed, 0);

    // sub2 also bills cleanly.
    let r2 = s.client.process(&s.merchant2, &svc2.service_id, &0, &10);
    assert_eq!(r2.charged, 1);
    assert_eq!(r2.failed, 0);
}

#[test]
fn test_extend_subscription_does_not_truncate_other_sub_budget() {
    // Refreshing one sub's allowance via extend_subscription must not shrink
    // the budget already reserved for the user's other active subs.
    let s = setup();
    let svc1 = register_default_service(&s); // PRICE=1000, periods=12
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Cheap"),
        &10,
        &MONTH,
        &0,
        &12,
    );

    s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    let sub2 = s.client.subscribe(&s.subscriber, &svc2.service_id, &false);
    // sub2 above used auto_renew=false → only 1 period approved (10)
    let allowance_after_subs = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after_subs, 12 * PRICE + 10);

    // Extend sub2 — refreshes its budget on top of existing allowance
    s.client.extend_subscription(&s.subscriber, &sub2.sub_id);
    let allowance_after_extend = s.token.allowance(&s.subscriber, &s.contract_addr);

    // sub1's 12000 budget must still be intact; extend just adds sub2's
    // 12-period budget on top.
    assert!(allowance_after_extend >= 12 * PRICE);
    assert_eq!(allowance_after_extend, 12 * PRICE + 10 + 12 * 10);
}

#[test]
fn test_toggle_auto_renew_back_on_restores_allowance() {
    // Toggle off -> revokes; toggle on -> the existing re-approve path
    // refreshes the allowance, so the user can resume billing.
    let s = setup();
    let svc = register_default_service(&s);
    let sub = s.client.subscribe(&s.subscriber, &svc.service_id, &true);

    s.client.toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert_eq!(s.token.allowance(&s.subscriber, &s.contract_addr), 0);

    s.client.toggle_auto_renew(&s.subscriber, &sub.sub_id);
    assert!(s.token.allowance(&s.subscriber, &s.contract_addr) > 0);
}

#[test]
fn test_toggle_off_reduces_allowance_by_only_toggled_sub_share() {
    // LIB-8 (full fix): the toggle-off path mirrors cancel — reduce the
    // (subscriber, contract) allowance by exactly the toggled sub's
    // reservation, leaving any other auto-renewing sub's budget intact.
    let s = setup();
    let svc1 = register_default_service(&s); // 12 * PRICE
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Other Plan"),
        &500,
        &WEEK,
        &0,
        &12,
    ); // 12 * 500

    let sub1 = s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    s.client.subscribe(&s.subscriber, &svc2.service_id, &true);

    s.client.toggle_auto_renew(&s.subscriber, &sub1.sub_id);

    let allowance = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance, 12 * 500); // sub2's budget only
}

#[test]
fn test_cancel_after_charges_reduces_only_remaining_reserved() {
    // LIB-8 (full fix): SubReservedAmount is decremented on every successful
    // process() charge, so cancel() removes only the *unused* portion of
    // this sub's budget — not its original full reservation. Without that
    // bookkeeping, cancel would over-remove and could drain the budget of
    // other live subs sharing the (subscriber, contract) allowance.
    let s = setup();
    let svc1 = register_default_service(&s); // 12 * PRICE = 12_000 reserved
    let svc2 = s.client.register_service(
        &s.merchant2,
        &String::from_str(&s.env, "Other"),
        &500,
        &MONTH,
        &0,
        &12,
    ); // 12 * 500 = 6_000 reserved

    let sub1 = s.client.subscribe(&s.subscriber, &svc1.service_id, &true);
    s.client.subscribe(&s.subscriber, &svc2.service_id, &true);

    // After subscribe (no process yet): full aggregate allowance.
    assert_eq!(
        s.token.allowance(&s.subscriber, &s.contract_addr),
        12 * PRICE + 12 * 500
    );

    // Charge sub1 three times: reserved drops by 3 * PRICE, allowance too.
    advance_time(&s.env, MONTH + 1);
    s.client.process(&s.merchant, &svc1.service_id, &0, &10);
    advance_time(&s.env, 2 * MONTH + 1);
    s.client.process(&s.merchant, &svc1.service_id, &0, &10);
    advance_time(&s.env, 3 * MONTH + 1);
    s.client.process(&s.merchant, &svc1.service_id, &0, &10);

    let allowance_after_3_charges = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after_3_charges, 12 * PRICE - 3 * PRICE + 12 * 500);

    // Cancel sub1 — only the remaining 9-period budget should drop off.
    s.client.cancel(&s.subscriber, &sub1.sub_id);

    let allowance_after_cancel = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after_cancel, 12 * 500);
}

#[test]
fn test_resubscribe_after_cancel_does_not_double_count_reservation() {
    // LIB-8 (full fix): subscribe pruning must drop the prior sub's
    // SubReservedAmount, otherwise a re-subscribe→cancel cycle would leave
    // a phantom reservation in storage and a future cancel could try to
    // remove budget that was never on chain. After the fix, the second
    // cancel cleanly drops the active reservation and only that.
    let s = setup();
    let svc = register_default_service(&s);

    let sub1 = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    s.client.cancel(&s.subscriber, &sub1.sub_id);
    assert_eq!(s.token.allowance(&s.subscriber, &s.contract_addr), 0);

    advance_time(&s.env, MONTH + 1);

    let sub2 = s.client.subscribe(&s.subscriber, &svc.service_id, &true);
    let allowance_after_resub = s.token.allowance(&s.subscriber, &s.contract_addr);
    assert_eq!(allowance_after_resub, 12 * PRICE);

    s.client.cancel(&s.subscriber, &sub2.sub_id);
    assert_eq!(s.token.allowance(&s.subscriber, &s.contract_addr), 0);
}

#[test]
fn test_resubscribe_cross_page_swap_updates_reverse_pointer() {
    // When pruning a sub from a non-tail page, the global tail is moved
    // into the freed slot — its reverse pointer (SubIndex.service_page)
    // MUST be rewritten, otherwise a future prune of the moved sub would
    // look in the wrong (old, now-empty) page, fail to decrement the
    // count, and leave the index in an inconsistent state.
    let s = setup();
    let svc = register_default_service(&s);

    // First subscriber lands at page 0, slot 0
    let first = Address::generate(&s.env);
    s.token_admin.mint(&first, &INITIAL_BALANCE);
    let first_sub = s.client.subscribe(&first, &svc.service_id, &true);

    // Fill the rest of page 0 (49 more entries)
    for _ in 0..49 {
        fund_and_subscribe(&s, svc.service_id);
    }

    // The 51st subscriber lands at page 1, slot 0 — global tail
    let tail = Address::generate(&s.env);
    s.token_admin.mint(&tail, &INITIAL_BALANCE);
    let tail_sub = s.client.subscribe(&tail, &svc.service_id, &true);

    // Cancel + expire + re-subscribe `first`. This triggers:
    //   1) prune first_sub from page 0 slot 0
    //   2) swap with global tail (tail_sub at page 1 slot 0)
    //   3) tail_sub.SubIndex.service_page must be rewritten 1 -> 0
    //   4) append new sub for `first` at the new global tail
    s.client.cancel(&first, &first_sub.sub_id);
    advance_time(&s.env, MONTH + 1);
    s.client.subscribe(&first, &svc.service_id, &true);

    // Now exercise tail's pruning path. If step 3 above failed to update
    // the reverse pointer, paginated_remove would search page 1 (where
    // tail_sub no longer lives), find nothing, and skip the count
    // decrement. The next append would then push the count to 52
    // instead of staying at 51.
    s.client.cancel(&tail, &tail_sub.sub_id);
    advance_time(&s.env, 3 * (MONTH + 1));
    s.client.subscribe(&tail, &svc.service_id, &true);

    // process() exposes the live count via ProcessResult.total — assert
    // it directly. With the bug present this would be 52.
    advance_time(&s.env, 6 * (MONTH + 1));
    let r = s.client.process(&s.merchant, &svc.service_id, &0, &200);
    assert_eq!(r.total, 51);
}
