use soroban_sdk::{Env, Address};
use crate::storage::DataKey;

pub fn withdraw(env: &Env, affiliate: Address) -> i128 {
    affiliate.require_auth();

    let amount: i128 = env
        .storage()
        .instance()
        .get(&DataKey::Commission(affiliate.clone()))
        .unwrap_or(0);

    if amount <= 0 {
        panic!("No commission available");
    }

    // reset balance BEFORE transfer (security best practice)
    env.storage()
        .instance()
        .set(&DataKey::Commission(affiliate.clone()), &0);

    // ⚠️ In real use: integrate token transfer here
    amount
}