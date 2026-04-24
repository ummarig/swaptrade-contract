use crate::portfolio::{Asset, Portfolio};
use soroban_sdk::{contracttype, symbol_short, Address, Env, Map, Vec};

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub enum TimeWindow {
    Day1,
    Day7,
    Day30,
    YTD,
    All,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct PerformanceMetrics {
    pub sharpe_ratio: u128,  // Fixed-point: 7 decimals (10^-7 precision)
    pub sortino_ratio: u128, // Fixed-point: 7 decimals
    pub max_drawdown: u128,  // Fixed-point: 7 decimals (percentage)
    pub volatility: u128,    // Fixed-point: 7 decimals (annualized)
    pub total_return: i128,  // Raw return amount
    pub win_rate: u128,      // Fixed-point: 7 decimals (percentage)
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct AssetAllocation {
    pub assets: Vec<(Asset, u128)>, // Asset and percentage allocation (fixed-point: 7 decimals)
    pub correlations: Map<(Asset, Asset), i128>, // Correlation matrix (fixed-point: 7 decimals)
    pub diversification_score: u128, // Fixed-point: 7 decimals
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct BenchmarkComparison {
    pub alpha: i128,             // Excess return over benchmark
    pub beta: u128,              // Market sensitivity (fixed-point: 7 decimals)
    pub tracking_error: u128,    // Standard deviation of difference (fixed-point: 7 decimals)
    pub information_ratio: i128, // Risk-adjusted excess return (fixed-point: 7 decimals)
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct PeriodReturns {
    pub time_weighted_return: i128, // Time-weighted return
    pub arithmetic_return: i128,    // Simple arithmetic return
    pub geometric_return: i128,     // Compound return
    pub start_value: i128,
    pub end_value: i128,
    pub period_days: u32,
}

pub struct PortfolioAnalytics;

impl PortfolioAnalytics {
    // Fixed-point arithmetic constants
    const FIXED_POINT_PRECISION: u128 = 10_000_000; // 10^7 for 7 decimal places
    const FIXED_POINT_ONE: u128 = 10_000_000; // 1.0 in fixed-point

    /// Calculate performance metrics for a user over a time window
    pub fn get_performance_metrics(
        env: &Env,
        portfolio: &Portfolio,
        user: Address,
        time_window: TimeWindow,
    ) -> PerformanceMetrics {
        let daily_values =
            Self::get_daily_portfolio_values(env, portfolio, user.clone(), time_window);
        if daily_values.is_empty() {
            return PerformanceMetrics {
                sharpe_ratio: 0,
                sortino_ratio: 0,
                max_drawdown: 0,
                volatility: 0,
                total_return: 0,
                win_rate: 0,
            };
        }

        let returns = Self::calculate_daily_returns(&daily_values);
        let total_return = Self::calculate_total_return(&daily_values);
        let volatility = Self::calculate_volatility(&returns);
        let downside_volatility = Self::calculate_downside_volatility(&returns);
        let max_drawdown = Self::calculate_max_drawdown(&daily_values);
        let win_rate = Self::calculate_win_rate(&returns);

        // Assume risk-free rate of 2% annualized (0.02 in fixed-point)
        let risk_free_rate = 2_000_000; // 0.02 * FIXED_POINT_PRECISION

        let sharpe_ratio = if volatility > 0 {
            ((total_return as u128 * Self::FIXED_POINT_PRECISION / daily_values.len() as u128)
                .saturating_sub(risk_free_rate))
            .saturating_mul(Self::FIXED_POINT_PRECISION)
                / volatility
        } else {
            0
        };

        let sortino_ratio = if downside_volatility > 0 {
            ((total_return as u128 * Self::FIXED_POINT_PRECISION / daily_values.len() as u128)
                .saturating_sub(risk_free_rate))
            .saturating_mul(Self::FIXED_POINT_PRECISION)
                / downside_volatility
        } else {
            0
        };

        let metrics = PerformanceMetrics {
            sharpe_ratio,
            sortino_ratio,
            max_drawdown,
            volatility,
            total_return,
            win_rate,
        };

        // Emit event for analytics calculation
        crate::events::Events::performance_metrics_calculated(
            env,
            user,
            time_window,
            sharpe_ratio,
            max_drawdown,
            env.ledger().timestamp() as i64,
        );

        metrics
    }

    /// Get asset allocation breakdown with correlation analysis
    pub fn get_asset_allocation(
        env: &Env,
        portfolio: &Portfolio,
        user: Address,
    ) -> AssetAllocation {
        let mut assets = Vec::new(env);
        let mut total_value: i128 = 0;

        // Get all user balances
        // Note: In a real implementation, we'd need to get current prices for each asset
        // For now, we'll use simplified logic assuming XLM = 1 USD, USDC = 1 USD

        let xlm_balance = portfolio.balance_of(env, Asset::XLM, user.clone());
        let usdc_balance =
            portfolio.balance_of(env, Asset::Custom(symbol_short!("USDCSIM")), user.clone());

        total_value = xlm_balance + usdc_balance;

        if total_value > 0 {
            let xlm_percentage =
                (xlm_balance as u128 * Self::FIXED_POINT_PRECISION) / total_value as u128;
            let usdc_percentage =
                (usdc_balance as u128 * Self::FIXED_POINT_PRECISION) / total_value as u128;

            assets.push_back((Asset::XLM, xlm_percentage));
            assets.push_back((Asset::Custom(symbol_short!("USDCSIM")), usdc_percentage));
        }

        // Calculate correlations (simplified - would need historical price data)
        let correlations = Map::new(env);
        let diversification_score = Self::calculate_diversification_score(&assets);

        let allocation = AssetAllocation {
            assets,
            correlations,
            diversification_score,
        };

        // Emit event for asset allocation analysis
        crate::events::Events::asset_allocation_analyzed(
            env,
            user,
            allocation.assets.len() as u32,
            diversification_score,
            env.ledger().timestamp() as i64,
        );

        allocation
    }

    /// Compare portfolio performance against a benchmark
    pub fn get_benchmark_comparison(
        env: &Env,
        portfolio: &Portfolio,
        user: Address,
        benchmark_id: Symbol,
        time_window: TimeWindow,
    ) -> BenchmarkComparison {
        let portfolio_returns =
            Self::get_daily_portfolio_values(env, portfolio, user.clone(), time_window);
        // In a real implementation, we'd fetch benchmark data
        // For now, return placeholder values
        let benchmark_returns = Vec::new(env); // Placeholder

        if portfolio_returns.is_empty() {
            let comparison = BenchmarkComparison {
                alpha: 0,
                beta: Self::FIXED_POINT_ONE,
                tracking_error: 0,
                information_ratio: 0,
            };

            // Emit event even for empty data
            crate::events::Events::benchmark_comparison_calculated(
                env,
                user,
                benchmark_id,
                0,
                Self::FIXED_POINT_ONE,
                env.ledger().timestamp() as i64,
            );

            return comparison;
        }

        // Simplified calculations - would need proper benchmark data
        let alpha = 0; // Placeholder
        let beta = Self::FIXED_POINT_ONE; // Assume beta = 1.0
        let tracking_error = 0; // Placeholder
        let information_ratio = 0; // Placeholder

        let comparison = BenchmarkComparison {
            alpha,
            beta,
            tracking_error,
            information_ratio,
        };

        // Emit event for benchmark comparison
        crate::events::Events::benchmark_comparison_calculated(
            env,
            user,
            benchmark_id,
            alpha,
            beta,
            env.ledger().timestamp() as i64,
        );

        comparison
    }

    /// Calculate period returns between timestamps
    pub fn get_period_returns(
        env: &Env,
        portfolio: &Portfolio,
        user: Address,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> PeriodReturns {
        let daily_values = Self::get_portfolio_values_in_range(
            env,
            portfolio,
            user.clone(),
            start_timestamp,
            end_timestamp,
        );

        if daily_values.is_empty() {
            let returns = PeriodReturns {
                time_weighted_return: 0,
                arithmetic_return: 0,
                geometric_return: 0,
                start_value: 0,
                end_value: 0,
                period_days: 0,
            };

            // Emit event even for empty data
            crate::events::Events::period_returns_calculated(
                env,
                user,
                start_timestamp,
                end_timestamp,
                0,
                env.ledger().timestamp() as i64,
            );

            return returns;
        }

        let start_value = daily_values.get(0).unwrap_or(0);
        let end_value = daily_values.get(daily_values.len() - 1).unwrap_or(0);
        let period_days = daily_values.len() as u32;

        let arithmetic_return = end_value - start_value;
        let time_weighted_return = Self::calculate_time_weighted_return(&daily_values);
        let geometric_return = Self::calculate_geometric_return(&daily_values);

        let returns = PeriodReturns {
            time_weighted_return,
            arithmetic_return,
            geometric_return,
            start_value,
            end_value,
            period_days,
        };

        // Emit event for period returns calculation
        crate::events::Events::period_returns_calculated(
            env,
            user,
            start_timestamp,
            end_timestamp,
            time_weighted_return,
            env.ledger().timestamp() as i64,
        );

        returns
    }

    // Helper methods for calculations

    fn get_daily_portfolio_values(
        env: &Env,
        portfolio: &Portfolio,
        user: Address,
        time_window: TimeWindow,
    ) -> Vec<i128> {
        let current_timestamp = env.ledger().timestamp();
        let current_date = current_timestamp / 86400;

        let (start_date, end_date) = match time_window {
            TimeWindow::Day1 => (current_date.saturating_sub(1), current_date),
            TimeWindow::Day7 => (current_date.saturating_sub(7), current_date),
            TimeWindow::Day30 => (current_date.saturating_sub(30), current_date),
            TimeWindow::YTD => {
                // Simplified YTD - would need proper calendar logic
                let year_start = current_date - (current_date % 365);
                (year_start, current_date)
            }
            TimeWindow::All => {
                // Get all available historical data
                // For now, return last 90 days as a reasonable "all" period
                (current_date.saturating_sub(90), current_date)
            }
        };

        portfolio.get_portfolio_values_in_range(env, user, start_date, end_date)
    }

    fn get_portfolio_values_in_range(
        env: &Env,
        portfolio: &Portfolio,
        user: Address,
        start_timestamp: u64,
        end_timestamp: u64,
    ) -> Vec<i128> {
        let start_date = start_timestamp / 86400;
        let end_date = end_timestamp / 86400;
        portfolio.get_portfolio_values_in_range(env, user, start_date, end_date)
    }

    pub fn calculate_daily_returns(values: &Vec<i128>) -> Vec<i128> {
        let mut returns = Vec::new(values.env());
        for i in 1..values.len() {
            let prev = values.get(i - 1).unwrap_or(0);
            let curr = values.get(i).unwrap_or(0);
            if prev != 0 {
                let ret = ((curr - prev) as i128 * Self::FIXED_POINT_PRECISION as i128) / prev;
                returns.push_back(ret);
            }
        }
        returns
    }

    pub fn calculate_volatility(returns: &Vec<i128>) -> u128 {
        if values.is_empty() {
            return 0;
        }
        let start = values.get(0).unwrap_or(0);
        let end = values.get(values.len() - 1).unwrap_or(0);
        end - start
    }

    fn calculate_volatility(returns: &Vec<i128>) -> u128 {
        if returns.is_empty() {
            return 0;
        }

        // Calculate mean
        let mut sum: i128 = 0;
        for i in 0..returns.len() {
            sum += returns.get(i).unwrap_or(0);
        }
        let mean = sum / returns.len() as i128;

        // Calculate variance
        let mut variance: u128 = 0;
        for i in 0..returns.len() {
            let diff = returns.get(i).unwrap_or(0) - mean;
            variance += (diff * diff) as u128;
        }
        variance /= returns.len() as u128;

        // Return standard deviation (volatility)
        Self::sqrt_fixed_point(variance)
    }

    fn calculate_downside_volatility(returns: &Vec<i128>) -> u128 {
        if returns.is_empty() {
            return 0;
        }

        // Only consider negative returns
        let mut negative_returns = Vec::new(returns.env());
        for i in 0..returns.len() {
            let ret = returns.get(i).unwrap_or(0);
            if ret < 0 {
                negative_returns.push_back(ret);
            }
        }

        Self::calculate_volatility(&negative_returns)
    }

    pub fn calculate_max_drawdown(values: &Vec<i128>) -> u128 {
        if values.is_empty() {
            return 0;
        }

        let mut max_drawdown: u128 = 0;
        let mut peak = values.get(0).unwrap_or(0);

        for i in 1..values.len() {
            let current = values.get(i).unwrap_or(0);
            if current > peak {
                peak = current;
            } else {
                let drawdown =
                    ((peak - current) as u128 * Self::FIXED_POINT_PRECISION) / peak as u128;
                if drawdown > max_drawdown {
                    max_drawdown = drawdown;
                }
            }
        }

        max_drawdown
    }

    pub fn calculate_win_rate(returns: &Vec<i128>) -> u128 {
        if returns.is_empty() {
            return 0;
        }

        let mut wins = 0;
        for i in 0..returns.len() {
            if returns.get(i).unwrap_or(0) > 0 {
                wins += 1;
            }
        }

        (wins as u128 * Self::FIXED_POINT_PRECISION) / returns.len() as u128
    }

    pub fn calculate_diversification_score(assets: &Vec<(Asset, u128)>) -> u128 {
        if assets.is_empty() {
            return 0;
        }

        // Simplified diversification score based on number of assets and allocation evenness
        let num_assets = assets.len() as u128;
        let mut herfindahl = 0u128;

        for i in 0..assets.len() {
            let (_, percentage) = assets.get(i).unwrap_or((Asset::XLM, 0));
            herfindahl += percentage * percentage;
        }

        // Herfindahl-Hirschman Index (lower is more diversified)
        // Convert to diversification score (higher is more diversified)
        if herfindahl > 0 {
            Self::FIXED_POINT_PRECISION.saturating_sub(herfindahl / Self::FIXED_POINT_PRECISION)
        } else {
            Self::FIXED_POINT_PRECISION
        }
    }

    fn calculate_time_weighted_return(values: &Vec<i128>) -> i128 {
        if values.len() < 2 {
            return 0;
        }

        let mut twr = Self::FIXED_POINT_ONE as i128;
        for i in 1..values.len() {
            let prev = values.get(i - 1).unwrap_or(0);
            let curr = values.get(i).unwrap_or(0);
            if prev > 0 {
                let period_return = (curr as i128 * Self::FIXED_POINT_PRECISION as i128) / prev;
                twr = (twr * period_return) / Self::FIXED_POINT_PRECISION as i128;
            }
        }

        twr - Self::FIXED_POINT_ONE as i128
    }

    fn calculate_geometric_return(values: &Vec<i128>) -> i128 {
        if values.len() < 2 {
            return 0;
        }

        let twr = Self::calculate_time_weighted_return(values);
        // For geometric return, we need to annualize if we had time periods
        // For now, return the TWR as approximation
        twr
    }

    // Fixed-point square root approximation
    fn sqrt_fixed_point(value: u128) -> u128 {
        if value == 0 {
            return 0;
        }

        let mut x = value;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + value / x) / 2;
        }
        x
    }
}
