use soroban_sdk::{Env, Address};
use crate::storage::DataKey;

pub fn record_volume(env: &Env, user: Address, amount: i128) {
    let current: i128 = env
        .storage()
        .instance()
        .get(&DataKey::Volume(user.clone()))
        .unwrap_or(0);

    env.storage()
        .instance()
        .set(&DataKey::Volume(user.clone()), &(current + amount));

    // assign commission
    if let Some(referrer) = env.storage().instance().get::<_, Address>(&DataKey::Referrer(user.clone())) {
        let rate: i128 = env.storage().instance().get(&DataKey::CommissionRate).unwrap();

        let commission = amount * rate / 100;

        let current_commission: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Commission(referrer.clone()))
            .unwrap_or(0);

        env.storage().instance().set(
            &DataKey::Commission(referrer),
            &(current_commission + commission),
        );
    }
}