#![cfg(test)]

use super::*;
use soroban_sdk::{Env, String, Address};
use soroban_sdk::testutils::Address as TestAddress;

#[test]
fn test_basic_functionality() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Test NFT"),
        String::from_str(&env, "TNFT"),
        String::from_str(&env, "https://example.com/token/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Test that the contract can be instantiated
    // The stellar-tokens library provides the ERC-721 functionality
    // We just need to test that our contract wrapper works
    
    // Test basic metadata functions
    let name = client.name();
    let symbol = client.symbol();
    
    // With the new constructor order: (name, symbol, token_uri)
    assert_eq!(name, String::from_str(&env, "Test NFT"));
    assert_eq!(symbol, String::from_str(&env, "TNFT"));
}

#[test]
fn test_contract_instantiation() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "My NFT"),
        String::from_str(&env, "MNFT"),
        String::from_str(&env, "https://example.com/metadata/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Test that the contract was instantiated correctly
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "My NFT"));
    assert_eq!(symbol, String::from_str(&env, "MNFT"));
}

#[test]
fn test_different_metadata_values() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "CryptoPunks"),
        String::from_str(&env, "PUNK"),
        String::from_str(&env, "https://cryptopunks.com/metadata/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "CryptoPunks"));
    assert_eq!(symbol, String::from_str(&env, "PUNK"));
}

#[test]
fn test_empty_metadata() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, ""),
        String::from_str(&env, ""),
        String::from_str(&env, ""),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, ""));
    assert_eq!(symbol, String::from_str(&env, ""));
}

#[test]
fn test_long_metadata_values() {
    let env = Env::default();
    let long_name = String::from_str(&env, "This is a very long NFT collection name that tests the contract's ability to handle long strings");
    let long_symbol = String::from_str(&env, "VERYLONGSYMBOL");
    let long_uri = String::from_str(&env, "https://example.com/very/long/metadata/uri/that/might/be/used/for/storing/detailed/information/about/the/nft/collection");
    
    let contract_id = env.register(NFTContract, (long_name.clone(), long_symbol.clone(), long_uri.clone()));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, long_name);
    assert_eq!(symbol, long_symbol);
}

#[test]
fn test_special_characters_in_metadata() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "NFT Collection 🚀"),
        String::from_str(&env, "NFT🚀"),
        String::from_str(&env, "https://example.com/nft/metadata/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "NFT Collection 🚀"));
    assert_eq!(symbol, String::from_str(&env, "NFT🚀"));
}

#[test]
fn test_multiple_contract_instances() {
    let env = Env::default();
    
    // Create first contract instance
    let contract_id_1 = env.register(NFTContract, (
        String::from_str(&env, "Collection 1"),
        String::from_str(&env, "COL1"),
        String::from_str(&env, "https://example.com/collection1/"),
    ));
    let client_1 = NFTContractClient::new(&env, &contract_id_1);
    
    // Create second contract instance
    let contract_id_2 = env.register(NFTContract, (
        String::from_str(&env, "Collection 2"),
        String::from_str(&env, "COL2"),
        String::from_str(&env, "https://example.com/collection2/"),
    ));
    let client_2 = NFTContractClient::new(&env, &contract_id_2);
    
    // Test that each instance has its own metadata
    assert_eq!(client_1.name(), String::from_str(&env, "Collection 1"));
    assert_eq!(client_1.symbol(), String::from_str(&env, "COL1"));
    
    assert_eq!(client_2.name(), String::from_str(&env, "Collection 2"));
    assert_eq!(client_2.symbol(), String::from_str(&env, "COL2"));
}

#[test]
fn test_edge_cases() {
    let env = Env::default();
    
    // Test with very short values
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "A"),
        String::from_str(&env, "B"),
        String::from_str(&env, "C"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    assert_eq!(client.name(), String::from_str(&env, "A"));
    assert_eq!(client.symbol(), String::from_str(&env, "B"));
}

#[test]
fn test_numeric_metadata() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "12345"),
        String::from_str(&env, "67890"),
        String::from_str(&env, "https://example.com/123/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    assert_eq!(client.name(), String::from_str(&env, "12345"));
    assert_eq!(client.symbol(), String::from_str(&env, "67890"));
}

// ========== ERC-721 Core Function Tests ==========

#[test]
fn test_erc721_metadata_functions() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Test NFT"),
        String::from_str(&env, "TNFT"),
        String::from_str(&env, "https://example.com/token/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Test all metadata functions
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "Test NFT"));
    assert_eq!(symbol, String::from_str(&env, "TNFT"));
}

#[test]
fn test_erc721_contract_deployment() {
    let env = Env::default();
    
    // Test multiple contract deployments
    let contracts = [
        (String::from_str(&env, "NFT 1"), String::from_str(&env, "NFT1"), String::from_str(&env, "https://example.com/1/")),
        (String::from_str(&env, "NFT 2"), String::from_str(&env, "NFT2"), String::from_str(&env, "https://example.com/2/")),
        (String::from_str(&env, "NFT 3"), String::from_str(&env, "NFT3"), String::from_str(&env, "https://example.com/3/")),
    ];
    
    for (name, symbol, uri) in contracts.iter() {
        let contract_id = env.register(NFTContract, (name.clone(), symbol.clone(), uri.clone()));
        let client = NFTContractClient::new(&env, &contract_id);
        
        assert_eq!(client.name(), *name);
        assert_eq!(client.symbol(), *symbol);
    }
}

#[test]
fn test_erc721_constructor_parameters() {
    let env = Env::default();
    
    // Test various constructor parameter combinations
    let test_cases = [
        (String::from_str(&env, "Empty"), String::from_str(&env, ""), String::from_str(&env, "")),
        (String::from_str(&env, "Short"), String::from_str(&env, "A"), String::from_str(&env, "B")),
        (String::from_str(&env, "Long"), String::from_str(&env, "Very Long NFT Collection Name"), String::from_str(&env, "VERYLONGSYMBOL")),
        (String::from_str(&env, "Special"), String::from_str(&env, "NFT 🚀"), String::from_str(&env, "NFT🚀")),
        (String::from_str(&env, "Numbers"), String::from_str(&env, "12345"), String::from_str(&env, "67890")),
    ];
    
    for (name, symbol, uri) in test_cases.iter() {
        let contract_id = env.register(NFTContract, (name.clone(), symbol.clone(), uri.clone()));
        let client = NFTContractClient::new(&env, &contract_id);
        
        assert_eq!(client.name(), *name);
        assert_eq!(client.symbol(), *symbol);
    }
}

#[test]
fn test_erc721_metadata_persistence() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Persistent NFT"),
        String::from_str(&env, "PNFT"),
        String::from_str(&env, "https://example.com/persistent/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Test that metadata persists across multiple calls
    for _ in 0..5 {
        let name = client.name();
        let symbol = client.symbol();
        
        assert_eq!(name, String::from_str(&env, "Persistent NFT"));
        assert_eq!(symbol, String::from_str(&env, "PNFT"));
    }
}

#[test]
fn test_erc721_unicode_support() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "NFT with Unicode: 🚀🌟💎"),
        String::from_str(&env, "UNI🚀"),
        String::from_str(&env, "https://example.com/unicode/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "NFT with Unicode: 🚀🌟💎"));
    assert_eq!(symbol, String::from_str(&env, "UNI🚀"));
}

#[test]
fn test_erc721_url_handling() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "URL Test NFT"),
        String::from_str(&env, "UTNFT"),
        String::from_str(&env, "https://example.com/metadata/token/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "URL Test NFT"));
    assert_eq!(symbol, String::from_str(&env, "UTNFT"));
}

#[test]
fn test_erc721_case_sensitivity() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Case Sensitive NFT"),
        String::from_str(&env, "CSNFT"),
        String::from_str(&env, "https://example.com/case/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    // Test that case is preserved exactly
    assert_eq!(name, String::from_str(&env, "Case Sensitive NFT"));
    assert_eq!(symbol, String::from_str(&env, "CSNFT"));
}

#[test]
fn test_erc721_whitespace_handling() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "  NFT with Spaces  "),
        String::from_str(&env, "  SPACES  "),
        String::from_str(&env, "https://example.com/spaces/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    let name = client.name();
    let symbol = client.symbol();
    
    // Test that whitespace is preserved exactly
    assert_eq!(name, String::from_str(&env, "  NFT with Spaces  "));
    assert_eq!(symbol, String::from_str(&env, "  SPACES  "));
}

// ========== ERC-721 Integration Tests ==========

#[test]
fn test_erc721_comprehensive_metadata() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Comprehensive Metadata Test NFT"),
        String::from_str(&env, "CMTNFT"),
        String::from_str(&env, "https://example.com/comprehensivemetadata/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Test comprehensive metadata operations
    let name = client.name();
    let symbol = client.symbol();
    
    assert_eq!(name, String::from_str(&env, "Comprehensive Metadata Test NFT"));
    assert_eq!(symbol, String::from_str(&env, "CMTNFT"));
}

#[test]
fn test_erc721_stress_test() {
    let env = Env::default();
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Stress Test NFT"),
        String::from_str(&env, "STNFT"),
        String::from_str(&env, "https://example.com/stresstest/"),
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Stress test with many metadata operations
    for _ in 0..100 {
        let name = client.name();
        let symbol = client.symbol();
        
        assert_eq!(name, String::from_str(&env, "Stress Test NFT"));
        assert_eq!(symbol, String::from_str(&env, "STNFT"));
    }
}

#[test]
fn test_erc721_multiple_contracts_stress() {
    let env = Env::default();
    
    // Create multiple contracts and test them
    for _ in 0..10 {
        let contract_id = env.register(NFTContract, (
            String::from_str(&env, "NFT Collection"),
            String::from_str(&env, "NFT"),
            String::from_str(&env, "https://example.com/collection/"),
        ));
        let client = NFTContractClient::new(&env, &contract_id);
        
        let name = client.name();
        let symbol = client.symbol();
        
        assert_eq!(name, String::from_str(&env, "NFT Collection"));
        assert_eq!(symbol, String::from_str(&env, "NFT"));
    }
}

#[test]
fn test_erc721_metadata_edge_cases() {
    let env = Env::default();
    
    // Test various edge cases for metadata
    let edge_cases = [
        ("", "", ""),
        ("A", "B", "C"),
        ("Very Long Name That Tests The Contract's Ability To Handle Long Strings", "VERYLONGSYMBOL", "https://example.com/very/long/uri/"),
        ("NFT with Emoji 🚀🌟💎", "EMOJI🚀", "https://example.com/emoji/"),
        ("12345", "67890", "https://example.com/numbers/"),
        ("  Spaces  ", "  SPACES  ", "https://example.com/spaces/"),
    ];
    
    for (name, symbol, uri) in edge_cases.iter() {
        let contract_id = env.register(NFTContract, (
            String::from_str(&env, name),
            String::from_str(&env, symbol),
            String::from_str(&env, uri),
        ));
        let client = NFTContractClient::new(&env, &contract_id);
        
        let retrieved_name = client.name();
        let retrieved_symbol = client.symbol();
        
        assert_eq!(retrieved_name, String::from_str(&env, name));
        assert_eq!(retrieved_symbol, String::from_str(&env, symbol));
    }
}

// test the mint function
#[test]
fn test_mint_function() {
    let env = Env::default();
    let creator = Address::generate(&env);
    let user = Address::generate(&env);
    let contract_id = env.register(NFTContract, (
        String::from_str(&env, "Test NFT"),
        String::from_str(&env, "TNFT"),
        String::from_str(&env, "https://example.com/token/"),
        &creator,
    ));
    let client = NFTContractClient::new(&env, &contract_id);
    
    // Mock authentication for the creator address
    env.mock_all_auths();

    // Test minting functionality - the creator should be able to mint since they're the owner
    let token_id = client.mint(&user);
    assert_eq!(token_id, 0);

    // Verify the token was minted by checking balance
    let balance = client.balance(&user);
    assert_eq!(balance, 1);

    // Verify the token owner
    let token_owner = client.owner_of(&0);
    assert_eq!(token_owner, user);
    assert_eq!(client.tokens_of_owner(&user).len(), 1);

    // Test minting another token
    let token_id_2 = client.mint(&user);
    assert_eq!(token_id_2, 1);
    assert_eq!(client.tokens_of_owner(&user).len(), 2);
    
    let balance_after_second_mint = client.balance(&user);
    assert_eq!(balance_after_second_mint, 2);

    assert_eq!(client.tokens_of_owner(&creator).len(), 0);
}
