use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use config::RateLimitConfig;
use governor::clock::{DefaultClock, Reference};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use mini_moka::sync::Cache;

pub mod layer;

pub use layer::RateLimitLayer;

type KeyedRateLimiter<K> = RateLimiter<K, DefaultKeyedStateStore<K>, DefaultClock>;

pub struct RateLimitManager {
    config: Arc<RateLimitConfig>,
    global: Option<Arc<KeyedRateLimiter<()>>>,
    per_ip: Option<Arc<KeyedRateLimiter<IpAddr>>>,
    per_user: Option<Arc<KeyedRateLimiter<String>>>,
    tool_limiters: Cache<String, Arc<KeyedRateLimiter<String>>>,
    downstream_limiters: Cache<String, Arc<KeyedRateLimiter<String>>>,
}

impl RateLimitManager {
    pub fn new(config: RateLimitConfig) -> Result<Self> {
        let config = Arc::new(config);
        
        let global = config.global.as_ref().map(|quota| {
            let (per_second, burst) = config.per_second_quota(quota);
            Arc::new(RateLimiter::keyed(
                Quota::per_second(per_second.try_into().unwrap())
                    .allow_burst(burst.try_into().unwrap()),
            ))
        });
        
        let per_ip = config.per_ip.as_ref().map(|quota| {
            let (per_second, burst) = config.per_second_quota(quota);
            Arc::new(RateLimiter::keyed(
                Quota::per_second(per_second.try_into().unwrap())
                    .allow_burst(burst.try_into().unwrap()),
            ))
        });
        
        let per_user = config.per_user.as_ref().map(|quota| {
            let (per_second, burst) = config.per_second_quota(quota);
            Arc::new(RateLimiter::keyed(
                Quota::per_second(per_second.try_into().unwrap())
                    .allow_burst(burst.try_into().unwrap()),
            ))
        });
        
        let tool_limiters = Cache::builder()
            .max_capacity(1000)
            .time_to_idle(Duration::from_secs(3600))
            .build();
            
        let downstream_limiters = Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(3600))
            .build();
        
        Ok(Self {
            config,
            global,
            per_ip,
            per_user,
            tool_limiters,
            downstream_limiters,
        })
    }
    
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
    
    pub async fn check_global(&self) -> Result<(), RateLimitError> {
        if let Some(limiter) = &self.global {
            limiter.check_key(&())
                .map_err(|_| RateLimitError::GlobalLimitExceeded)?;
        }
        Ok(())
    }
    
    pub async fn check_ip(&self, ip: IpAddr) -> Result<(), RateLimitError> {
        if let Some(limiter) = &self.per_ip {
            limiter.check_key(&ip)
                .map_err(|_| RateLimitError::IpLimitExceeded)?;
        }
        Ok(())
    }
    
    pub async fn check_user(&self, user_id: &str) -> Result<(), RateLimitError> {
        if let Some(limiter) = &self.per_user {
            limiter.check_key(&user_id.to_string())
                .map_err(|_| RateLimitError::UserLimitExceeded)?;
        }
        Ok(())
    }
    
    pub async fn check_tool(&self, tool_name: &str) -> Result<(), RateLimitError> {
        if let Some(quota) = self.config.find_tool_limit(tool_name) {
            let limiter = self.tool_limiters.get_with(tool_name.to_string(), || {
                let (per_second, burst) = self.config.per_second_quota(quota);
                Arc::new(RateLimiter::keyed(
                    Quota::per_second(per_second.try_into().unwrap())
                        .allow_burst(burst.try_into().unwrap()),
                ))
            });
            
            limiter.check_key(&tool_name.to_string())
                .map_err(|_| RateLimitError::ToolLimitExceeded)?;
        }
        Ok(())
    }
    
    pub async fn check_downstream(&self, server_name: &str) -> Result<(), RateLimitError> {
        if let Some(quota) = self.config.downstream_limit(server_name) {
            let limiter = self.downstream_limiters.get_with(server_name.to_string(), || {
                let (per_second, burst) = self.config.per_second_quota(&quota);
                Arc::new(RateLimiter::keyed(
                    Quota::per_second(per_second.try_into().unwrap())
                        .allow_burst(burst.try_into().unwrap()),
                ))
            });
            
            limiter.check_key(&server_name.to_string())
                .map_err(|_| RateLimitError::DownstreamLimitExceeded)?;
        }
        Ok(())
    }
    
    pub fn get_retry_after(&self, error: &RateLimitError) -> Option<Duration> {
        match error {
            RateLimitError::GlobalLimitExceeded => self.global.as_ref().map(|l| self.get_wait_time(l, &())),
            RateLimitError::IpLimitExceeded => None,
            RateLimitError::UserLimitExceeded => None,
            RateLimitError::ToolLimitExceeded => None,
            RateLimitError::DownstreamLimitExceeded => None,
        }
    }
    
    fn get_wait_time<K>(&self, limiter: &KeyedRateLimiter<K>, key: &K) -> Duration
    where
        K: governor::state::keyed::ShrinkableKeyedStateStore<K> + Clone,
    {
        match limiter.check_key(key) {
            Ok(_) => Duration::from_secs(1),
            Err(not_until) => not_until.wait_time_from(DefaultClock::default().now()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Global rate limit exceeded")]
    GlobalLimitExceeded,
    
    #[error("IP rate limit exceeded")]
    IpLimitExceeded,
    
    #[error("User rate limit exceeded")]
    UserLimitExceeded,
    
    #[error("Tool rate limit exceeded")]
    ToolLimitExceeded,
    
    #[error("Downstream server rate limit exceeded")]
    DownstreamLimitExceeded,
}