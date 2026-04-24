use soroban_sdk::{contracttype, symbol_short, Env, Symbol};

const DEFAULT_PRICE_UPDATE_TOLERANCE_BPS: u32 = 10;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractError {
    InvalidPrice = 1,
    StalePrice = 2,
    SlippageExceeded = 3,
    PriceNotSet = 4,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceData {
    pub price: u128,
    pub timestamp: u64,
}

pub trait PriceFeed {
    fn get_price(env: &Env, token_pair: (Symbol, Symbol)) -> Result<u128, ContractError>;
    fn last_update_time(env: &Env, token_pair: (Symbol, Symbol)) -> u64;
    fn set_price(env: &Env, token_pair: (Symbol, Symbol), price: u128);
}

fn tolerance_key(pair: &(Symbol, Symbol)) -> (Symbol, Symbol, Symbol) {
    (symbol_short!("TOL"), pair.0.clone(), pair.1.clone())
}

pub fn get_price_update_tolerance_bps(env: &Env, pair: (Symbol, Symbol)) -> u32 {
    let key = tolerance_key(&pair);
    env.storage()
        .instance()
        .get(&key)
        .unwrap_or(DEFAULT_PRICE_UPDATE_TOLERANCE_BPS)
}

pub fn set_price_update_tolerance_bps(env: &Env, pair: (Symbol, Symbol), bps: u32) {
    let key = tolerance_key(&pair);
    env.storage().instance().set(&key, &bps);
}

pub fn get_stored_price(env: &Env, pair: (Symbol, Symbol)) -> Option<PriceData> {
    env.storage().instance().get(&pair)
}

fn price_delta_exceeds_tolerance(last_price: u128, new_price: u128, tolerance_bps: u32) -> bool {
    if last_price == 0 {
        return true;
    }
    let delta = if new_price >= last_price {
        new_price - last_price
    } else {
        last_price - new_price
    };
    let threshold = (last_price as u128).saturating_mul(tolerance_bps as u128) / 10_000;
    delta > threshold
}

pub fn set_stored_price(env: &Env, pair: (Symbol, Symbol), price: u128) {
    let existing = get_stored_price(env, pair.clone());
    let should_persist = match existing {
        None => true,
        Some(data) => price_delta_exceeds_tolerance(
            data.price,
            price,
            get_price_update_tolerance_bps(env, pair.clone()),
        ),
    };
    if should_persist {
        let timestamp = env.ledger().timestamp();
        let data = PriceData { price, timestamp };
        env.storage().instance().set(&pair, &data);
    }
}

pub fn get_price_safe(env: &Env, pair: (Symbol, Symbol)) -> Result<u128, ContractError> {
    match get_stored_price(env, pair) {
        Some(data) => Ok(data.price),
        None => Err(ContractError::PriceNotSet),
    }
}

pub struct DecentralizedOracle {
    feeds: Vec<FeedProvider>,
}

impl DecentralizedOracle {
    pub fn new() -> Self {
        Self { feeds: Vec::new() }
    }

    pub fn register_feed(&mut self, feed: FeedProvider) {
        self.feeds.push(feed);
    }

    pub fn submit_price(
        &self,
        feed_id: usize,
        token_pair: (Symbol, Symbol),
        price: u128,
        timestamp: u64,
    ) {
        if let Some(feed) = self.feeds.get(feed_id) {
            feed.submit_price(token_pair, price, timestamp);
        }
    }

    pub fn get_consensus_price(&self, token_pair: (Symbol, Symbol)) -> Option<u128> {
        let mut prices: Vec<u128> = self
            .feeds
            .iter()
            .filter_map(|feed| feed.get_price(token_pair))
            .collect();

        if prices.is_empty() {
            return None;
        }

        prices.sort_unstable();
        Some(prices[prices.len() / 2]) // Median
    }

    pub fn detect_anomalies(&self, token_pair: (Symbol, Symbol)) -> Vec<usize> {
        let prices: Vec<u128> = self
            .feeds
            .iter()
            .filter_map(|feed| feed.get_price(token_pair))
            .collect();

        let mean: u128 = prices.iter().sum::<u128>() / prices.len() as u128;
        let variance: u128 = prices
            .iter()
            .map(|&price| (price as i128 - mean as i128).pow(2) as u128)
            .sum::<u128>()
            / prices.len() as u128;
        let std_dev = (variance as f64).sqrt() as u128;

        prices
            .iter()
            .enumerate()
            .filter(|&(_, &price)| (price as i128 - mean as i128).abs() as u128 > 5 * std_dev)
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn get_price_history(
        &self,
        token_pair: (Symbol, Symbol),
        lookback_periods: usize,
    ) -> Vec<u128> {
        self.feeds
            .iter()
            .flat_map(|feed| feed.get_price_history(token_pair, lookback_periods))
            .collect()
    }
}
