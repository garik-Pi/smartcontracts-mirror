#![cfg(test)]
use crate::{Deployer, DeployerClient};
use soroban_sdk::{
    BytesN, Env, IntoVal, String, Val, Vec, vec
};

mod test_contract {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/hello_world.wasm"
    );
}

#[test]
fn test() {
    let env = Env::default();
    let deployer_client = DeployerClient::new(&env, &env.register(Deployer, ()));

    // Upload the Wasm to be deployed from the deployer contract.
    // This can also be called from within a contract if needed.
    let wasm_hash = env.deployer().upload_contract_wasm(test_contract::WASM);

    // Deploy contract using deployer, and include an init function to call.
    let salt = BytesN::from_array(&env, &[0; 32]);
    let constructor_args: Vec<Val> = ().into_val(&env);
    env.mock_all_auths();
    let contract_id = deployer_client.deploy_nft(&wasm_hash, &salt, &constructor_args);

    // Invoke contract to check that it is initialized.
    let client = test_contract::Client::new(&env, &contract_id);
    let out = client.hello(&String::from_str(&env, "NFT"));
    assert_eq!(out, vec![&env, String::from_str(&env, "Hello"), String::from_str(&env, "NFT")]);
}