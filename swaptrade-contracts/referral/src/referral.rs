use soroban_sdk::{Env, Address};
use crate::storage::DataKey;

pub fn register_referral(env: &Env, user: Address, referrer: Address) {
    user.require_auth();

    if user == referrer {
        panic!("Cannot refer yourself");
    }

    let key = DataKey::Referrer(user.clone());

    if env.storage().instance().has(&key) {
        panic!("Referrer already set");
    }

    env.storage().instance().set(&key, &referrer);
}