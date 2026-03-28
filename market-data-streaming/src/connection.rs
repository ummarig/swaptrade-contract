use crate::error::{MarketDataError, Result};
use crate::types::*;
use std::sync::Arc;
use parking_lot::RwLock;
use dashmap::DashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use std::collections::HashMap;
use tracing::{info, warn, error};

pub struct ConnectionManager {
    connections: Arc<DashMap<Uuid, Arc<RwLock<Connection>>>>,
    load_balancer: Arc<LoadBalancer>,
    connection_pool: Arc<ConnectionPool>,
    metrics: Arc<RwLock<ConnectionMetrics>>,
}

pub struct Connection {
    pub id: Uuid,
    pub remote_addr: String,
    pub connected_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub subscriptions: HashMap<String, Vec<MarketDataType>>,
    pub message_sender: mpsc::UnboundedSender<WebSocketMessage>,
    pub stats: ConnectionStats,
    pub state: ConnectionState,
}

#[derive(Debug, Clone)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
}

#[derive(Debug, Default, Clone)]
pub struct ConnectionStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub last_ping_sent: Option<DateTime<Utc>>,
    pub last_pong_received: Option<DateTime<Utc>>,
    pub errors_count: u64,
}

#[derive(Debug, Default)]
pub struct ConnectionMetrics {
    pub total_connections: u64,
    pub active_connections: u64,
    pub peak_connections: u64,
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub average_connection_duration: f64,
}

pub struct LoadBalancer {
    strategy: LoadBalancingStrategy,
    server_nodes: Arc<RwLock<Vec<ServerNode>>>,
    current_index: std::sync::atomic::AtomicUsize,
}

#[derive(Debug, Clone)]
pub struct ServerNode {
    pub id: String,
    pub address: String,
    pub weight: u32,
    pub current_connections: u32,
    pub max_connections: u32,
    pub is_healthy: bool,
    pub last_health_check: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum LoadBalancingStrategy {
    RoundRobin,
    LeastConnections,
    WeightedRoundRobin,
    IPHash,
}

pub struct ConnectionPool {
    max_connections: usize,
    idle_timeout: std::time::Duration,
    cleanup_interval: std::time::Duration,
    idle_connections: Arc<RwLock<HashMap<Uuid, DateTime<Utc>>>>,
}

impl ConnectionManager {
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            load_balancer: Arc::new(LoadBalancer::new()),
            connection_pool: Arc::new(ConnectionPool::new(max_connections)),
            metrics: Arc::new(RwLock::new(ConnectionMetrics::default())),
        }
    }

    pub async fn add_connection(&self, connection: Connection) -> Result<()> {
        let connection_id = connection.id;
        
        // Check connection limits
        if !self.connection_pool.can_add_connection() {
            return Err(MarketDataError::Validation("Connection limit reached".to_string()));
        }

        // Add to connections map
        self.connections.insert(connection_id, Arc::new(RwLock::new(connection)));

        // Update metrics
        {
            let mut metrics = self.metrics.write();
            metrics.total_connections += 1;
            metrics.active_connections += 1;
            if metrics.active_connections > metrics.peak_connections {
                metrics.peak_connections = metrics.active_connections;
            }
        }

        info!("Added connection: {}", connection_id);
        Ok(())
    }

    pub async fn remove_connection(&self, connection_id: Uuid) -> Result<()> {
        if let Some((_, conn)) = self.connections.remove(&connection_id) {
            let connection = conn.read();
            let duration = Utc::now().signed_duration_since(connection.connected_at);
            
            // Update metrics
            {
                let mut metrics = self.metrics.write();
                metrics.active_connections -= 1;
                metrics.total_messages_sent += connection.stats.messages_sent;
                metrics.total_messages_received += connection.stats.messages_received;
                metrics.total_bytes_sent += connection.stats.bytes_sent;
                metrics.total_bytes_received += connection.stats.bytes_received;
                
                // Update average connection duration
                let total_duration = metrics.average_connection_duration * (metrics.total_connections - 1) as f64;
                metrics.average_connection_duration = (total_duration + duration.num_milliseconds() as f64) / metrics.total_connections as f64;
            }

            // Close message channel
            drop(connection.stats.messages_sent); // Force close
        }

        info!("Removed connection: {}", connection_id);
        Ok(())
    }

    pub fn get_connection(&self, connection_id: Uuid) -> Option<Arc<RwLock<Connection>>> {
        self.connections.get(&connection_id).map(|entry| entry.clone())
    }

    pub async fn send_message(&self, connection_id: Uuid, message: WebSocketMessage) -> Result<()> {
        if let Some(conn) = self.connections.get(&connection_id) {
            let connection = conn.read();
            
            if let Err(_) = connection.message_sender.send(message.clone()) {
                warn!("Failed to send message to connection {}: channel closed", connection_id);
                return Err(MarketDataError::ConnectionNotFound(connection_id.to_string()));
            }

            // Update stats
            drop(connection);
            let mut conn_mut = conn.write();
            conn_mut.stats.messages_sent += 1;
            conn_mut.stats.bytes_sent += message.payload.len() as u64;
            conn_mut.last_activity = Utc::now();

            Ok(())
        } else {
            Err(MarketDataError::ConnectionNotFound(connection_id.to_string()))
        }
    }

    pub async fn broadcast_message(&self, message: WebSocketMessage, filter: Option<&dyn Fn(&Connection) -> bool>) -> Result<usize> {
        let mut sent_count = 0;
        
        for entry in self.connections.iter() {
            let connection = entry.value().read();
            
            if let Some(filter_fn) = filter {
                if !filter_fn(&*connection) {
                    continue;
                }
            }

            if let Err(_) = connection.message_sender.send(message.clone()) {
                warn!("Failed to broadcast to connection {}: channel closed", connection.id);
                continue;
            }

            sent_count += 1;
        }

        info!("Broadcasted message to {} connections", sent_count);
        Ok(sent_count)
    }

    pub async fn cleanup_idle_connections(&self, idle_timeout: std::time::Duration) -> Result<usize> {
        let now = Utc::now();
        let mut removed_count = 0;
        let mut connections_to_remove = Vec::new();

        for entry in self.connections.iter() {
            let connection = entry.value().read();
            if now.signed_duration_since(connection.last_activity).to_std().unwrap_or(std::time::Duration::MAX) > idle_timeout {
                connections_to_remove.push(connection.id);
            }
        }

        for connection_id in connections_to_remove {
            if let Err(e) = self.remove_connection(connection_id).await {
                error!("Error removing idle connection {}: {}", connection_id, e);
            } else {
                removed_count += 1;
            }
        }

        if removed_count > 0 {
            info!("Cleaned up {} idle connections", removed_count);
        }

        Ok(removed_count)
    }

    pub fn get_metrics(&self) -> ConnectionMetrics {
        self.metrics.read().clone()
    }

    pub fn get_connection_count(&self) -> usize {
        self.connections.len()
    }

    pub fn get_active_connections(&self) -> Vec<Arc<RwLock<Connection>>> {
        self.connections.iter().map(|entry| entry.clone()).collect()
    }

    pub async fn update_connection_activity(&self, connection_id: Uuid) -> Result<()> {
        if let Some(conn) = self.connections.get(&connection_id) {
            conn.write().last_activity = Utc::now();
            Ok(())
        } else {
            Err(MarketDataError::ConnectionNotFound(connection_id.to_string()))
        }
    }
}

impl LoadBalancer {
    pub fn new() -> Self {
        Self {
            strategy: LoadBalancingStrategy::LeastConnections,
            server_nodes: Arc::new(RwLock::new(Vec::new())),
            current_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn add_node(&self, node: ServerNode) {
        let mut nodes = self.server_nodes.write();
        nodes.push(node);
    }

    pub fn remove_node(&self, node_id: &str) {
        let mut nodes = self.server_nodes.write();
        nodes.retain(|node| node.id != node_id);
    }

    pub fn get_next_node(&self) -> Option<ServerNode> {
        let nodes = self.server_nodes.read();
        let healthy_nodes: Vec<&ServerNode> = nodes.iter()
            .filter(|node| node.is_healthy && node.current_connections < node.max_connections)
            .collect();

        if healthy_nodes.is_empty() {
            return None;
        }

        match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                let index = self.current_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % healthy_nodes.len();
                Some(healthy_nodes[index].clone())
            }
            LoadBalancingStrategy::LeastConnections => {
                healthy_nodes.iter()
                    .min_by_key(|node| node.current_connections)
                    .map(|node| (*node).clone())
            }
            LoadBalancingStrategy::WeightedRoundRobin => {
                // Simplified weighted round robin
                let total_weight: u32 = healthy_nodes.iter().map(|node| node.weight).sum();
                let mut weighted_nodes = Vec::new();
                for node in &healthy_nodes {
                    for _ in 0..node.weight {
                        weighted_nodes.push(*node);
                    }
                }
                let index = self.current_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % weighted_nodes.len();
                Some(weighted_nodes[index].clone())
            }
            LoadBalancingStrategy::IPHash => {
                // Simplified IP hash - would use actual client IP
                let index = (now_utc_timestamp() as usize) % healthy_nodes.len();
                Some(healthy_nodes[index].clone())
            }
        }
    }

    pub async fn health_check(&self) -> Result<()> {
        let mut nodes = self.server_nodes.write();
        let now = Utc::now();

        for node in nodes.iter_mut() {
            // Perform health check (simplified)
            let is_healthy = self.check_node_health(&node.address).await;
            node.is_healthy = is_healthy;
            node.last_health_check = now;
        }

        Ok(())
    }

    async fn check_node_health(&self, address: &str) -> bool {
        // Simplified health check - would implement actual HTTP/TCP check
        true
    }
}

impl ConnectionPool {
    pub fn new(max_connections: usize) -> Self {
        let pool = Self {
            max_connections,
            idle_timeout: std::time::Duration::from_secs(300), // 5 minutes
            cleanup_interval: std::time::Duration::from_secs(60), // 1 minute
            idle_connections: Arc::new(RwLock::new(HashMap::new())),
        };

        // Start cleanup task
        let idle_connections = Arc::clone(&pool.idle_connections);
        let idle_timeout = pool.idle_timeout;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(pool.cleanup_interval);
            loop {
                interval.tick().await;
                let now = Utc::now();
                let mut idle = idle_connections.write();
                idle.retain(|_, timestamp| {
                    now.signed_duration_since(*timestamp).to_std().unwrap_or(std::time::Duration::MAX) < idle_timeout
                });
            }
        });

        pool
    }

    pub fn can_add_connection(&self) -> bool {
        self.idle_connections.read().len() < self.max_connections
    }

    pub fn add_idle_connection(&self, connection_id: Uuid) {
        self.idle_connections.write().insert(connection_id, Utc::now());
    }

    pub fn remove_idle_connection(&self, connection_id: Uuid) {
        self.idle_connections.write().remove(&connection_id);
    }
}

fn now_utc_timestamp() -> i64 {
    Utc::now().timestamp_nanos_opt().unwrap_or(0)
}

impl Connection {
    pub fn new(
        id: Uuid,
        remote_addr: String,
        message_sender: mpsc::UnboundedSender<WebSocketMessage>,
    ) -> Self {
        Self {
            id,
            remote_addr,
            connected_at: Utc::now(),
            last_activity: Utc::now(),
            subscriptions: HashMap::new(),
            message_sender,
            stats: ConnectionStats::default(),
            state: ConnectionState::Connected,
        }
    }

    pub fn add_subscription(&mut self, symbol: String, data_types: Vec<MarketDataType>) {
        self.subscriptions
            .entry(symbol)
            .or_insert_with(Vec::new)
            .extend(data_types);
    }

    pub fn remove_subscription(&mut self, symbol: &str) {
        self.subscriptions.remove(symbol);
    }

    pub fn is_subscribed_to(&self, symbol: &str, data_type: &MarketDataType) -> bool {
        if let Some(types) = self.subscriptions.get(symbol) {
            types.contains(data_type)
        } else {
            false
        }
    }

    pub fn get_subscription_count(&self) -> usize {
        self.subscriptions.values().map(|types| types.len()).sum()
    }
}
