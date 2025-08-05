#![no_std]
use soroban_sdk::{contract, contractimpl, Env, String};
use stellar_tokens::non_fungible::{Base, NonFungibleToken};
use stellar_macros::default_impl;

#[contract]
pub struct NFTContract;

#[contractimpl]
impl NFTContract {
    pub fn __constructor(e: &Env, name: String, symbol: String, token_uri: String) {
        Base::set_metadata(e, token_uri, name, symbol);
    }
}

#[default_impl]
#[contractimpl]
impl NonFungibleToken for NFTContract {
    type ContractType = Base;
}

mod test;