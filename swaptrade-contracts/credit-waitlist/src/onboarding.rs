use soroban_sdk::{Env, Address};
use crate::storage::{DataKey, Status};

pub fn release_batch(env: &Env) {
    let mut start: u32 = env.storage().instance().get(&DataKey::QueueStart).unwrap_or(0);
    let end: u32 = env.storage().instance().get(&DataKey::QueueEnd).unwrap_or(0);
    let batch: u32 = env.storage().instance().get(&DataKey::BatchSize).unwrap();

    let mut count = 0;

    while start < end && count < batch {
        let user: Address = env.storage().instance().get(&DataKey::Queue(start)).unwrap();

        env.storage()
            .instance()
            .set(&DataKey::Status(user), &Status::Invited);

        start += 1;
        count += 1;
    }

    env.storage().instance().set(&DataKey::QueueStart, &start);
}