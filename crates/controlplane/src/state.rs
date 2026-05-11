use std::sync::Arc;

use axum::extract::ws::WebSocket;
use dashmap::DashMap;
use sea_orm::DatabaseConnection;
use tokio::sync::oneshot;

use crate::config::Config;
use crate::crypto::MasterKey;
use crate::rate_limit::RateLimiter;
use crate::scheduler::SchedulerHandle;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Inner>,
}

pub struct Inner {
    pub pool: DatabaseConnection,
    pub config: Config,
    pub master_key: MasterKey,
    pub scheduler: SchedulerHandle,
    pub rate_limiter: RateLimiter,
    /// Console sessions waiting on the agent to dial back with the matching
    /// `session_id`. The browser-side handler inserts a oneshot sender keyed
    /// by `session_id`; the agent-side handler pops it and hands over its
    /// upgraded `WebSocket`. The browser-side task removes its own entry on
    /// timeout or early close via the `SessionGuard` RAII type.
    pub console_sessions: Arc<DashMap<String, oneshot::Sender<WebSocket>>>,
}

impl AppState {
    pub fn new(pool: DatabaseConnection, config: Config, master_key: MasterKey) -> Self {
        Self {
            inner: Arc::new(Inner {
                pool,
                config,
                master_key,
                scheduler: SchedulerHandle::default(),
                rate_limiter: RateLimiter::default(),
                console_sessions: Arc::new(DashMap::new()),
            }),
        }
    }

    pub fn pool(&self) -> &DatabaseConnection {
        &self.inner.pool
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn master_key(&self) -> &MasterKey {
        &self.inner.master_key
    }

    pub fn scheduler(&self) -> &SchedulerHandle {
        &self.inner.scheduler
    }

    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.inner.rate_limiter
    }

    pub fn console_sessions(&self) -> &Arc<DashMap<String, oneshot::Sender<WebSocket>>> {
        &self.inner.console_sessions
    }
}
