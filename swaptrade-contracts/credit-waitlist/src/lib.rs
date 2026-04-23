#![no_std]

mod storage;
mod queue;
mod onboarding;
mod admin;
mod events;

use soroban_sdk::{contract, contractimpl, Address, Env};
use storage::DataKey;

#[contract]
pub struct CreditWaitlist;

#[contractimpl]
impl CreditWaitlist {

    pub fn initialize(env: Env, admin: Address, batch_size: u32) {
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::BatchSize, &batch_size);
        env.storage().instance().set(&DataKey::QueueStart, &0u32);
        env.storage().instance().set(&DataKey::QueueEnd, &0u32);
    }

    pub fn join(env: Env, user: Address) {
        queue::join_queue(&env, user.clone());
        events::joined(&env, user);
    }

    pub fn release(env: Env, admin: Address) {
        admin::require_admin(&env, &admin);
        onboarding::release_batch(&env);
    }

    pub fn onboard(env: Env, user: Address) {
        onboarding::mark_onboarded(&env, user.clone());
        events::onboarded(&env, user);
    }

    pub fn get_status(env: Env, user: Address) -> storage::Status {
        env.storage()
            .instance()
            .get(&DataKey::Status(user))
            .unwrap()
    }
}