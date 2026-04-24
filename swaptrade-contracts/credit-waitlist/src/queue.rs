use soroban_sdk::{Env, Address};
use crate::storage::{DataKey, Status};

pub fn join_queue(env: &Env, user: Address) {
    user.require_auth();

    let status_key = DataKey::Status(user.clone());

    if env.storage().instance().has(&status_key) {
        panic!("Already in queue");
    }

    let mut end: u32 = env.storage().instance().get(&DataKey::QueueEnd).unwrap_or(0);

    env.storage().instance().set(&DataKey::Queue(end), &user);
    env.storage().instance().set(&DataKey::QueueEnd, &(end + 1));

    env.storage().instance().set(&status_key, &Status::Waiting);
}