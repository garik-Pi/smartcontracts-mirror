#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};
use stellar_tokens::non_fungible::{Base, NonFungibleToken};
use stellar_access::ownable::{self as ownable, Ownable};
use stellar_macros::{default_impl, only_owner};

#[contract]
pub struct NFTContract;

#[contracttype]
pub struct NFTSummary {
    pub name: String,
    pub symbol: String,
    pub token_uri: String
}

#[contracttype]
enum NFTKeys {
    TokensOfOwner(Address),
}

#[contractimpl]
impl NFTContract {
    pub fn __constructor(e: &Env, name: String, symbol: String, token_uri: String, owner: Address) {
        Base::set_metadata(e, token_uri, name, symbol);
        ownable::set_owner(e, &owner);
    }

    #[only_owner]
    pub fn mint(e: &Env, to: Address) -> u32 {
        let token_id = Base::sequential_mint(e, &to);
        let key = NFTKeys::TokensOfOwner(to);
        let mut tokens = e.storage().persistent().get::<_, Vec<u32>>(&key).unwrap_or_else(|| Vec::new(&e));
        tokens.push_back(token_id);
        e.storage().persistent().set(&key, &tokens);
        token_id
    }

    pub fn summary(e: &Env, token_id: u32) -> NFTSummary {
        NFTSummary {
            name: Base::name(e),
            symbol: Base::symbol(e),
            token_uri: Base::token_uri(e, token_id)
        }
    }

    pub fn tokens_of_owner(e: &Env, owner: Address) -> Vec<u32> {
        owner.require_auth();
        let key = NFTKeys::TokensOfOwner(owner);
        let tokens = e.storage().persistent().get::<_, Vec<u32>>(&key).unwrap_or_else(|| Vec::new(&e));
        tokens
    }
}

#[default_impl]
#[contractimpl]
impl NonFungibleToken for NFTContract {
    type ContractType = Base;
}

#[default_impl]
#[contractimpl]
impl Ownable for NFTContract {
}

mod test;