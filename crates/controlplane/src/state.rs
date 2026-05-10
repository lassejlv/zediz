use std::sync::Arc;

use sea_orm::DatabaseConnection;

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
}
