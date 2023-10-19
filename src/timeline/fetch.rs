use std::collections::HashMap;

use crate::{
    get_remote_software,
    misskey::MisskeyDirectFetching,
    types::{
        json::{self, Post},
        Host,
    },
    RemoteSoftware,
};
use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use futures_util::future::Remote;
use log::debug;
use once_cell::sync::Lazy;

pub struct FetchOption {
    limit: u32,
    until_id: Option<String>,
}

impl FetchOption {
    pub fn get_limit(&self) -> u32 {
        self.limit
    }

    pub fn get_until_id(&self) -> Option<&str> {
        self.until_id.as_ref().map(|s| s.as_str())
    }
}

impl TryFrom<json::FetchOption> for FetchOption {
    type Error = Error;

    fn try_from(value: json::FetchOption) -> std::result::Result<Self, Self::Error> {
        if value.limit > 50 {
            return Err(anyhow!("Limit cannot be larger than 50."));
        }
        Ok(Self {
            limit: value.limit,
            until_id: value.untilId,
        })
    }
}

#[async_trait]
pub trait FetchStrategy {
    fn get_strategy_id(&self) -> &'static str;
    async fn get(&mut self, option: &FetchOption) -> Result<Vec<Post>>;
}

static FETCH_STRATEGY: Lazy<HashMap<Host, RemoteSoftware>> = Lazy::new(|| {HashMap::new()});

// TODO: Cache
async fn get_timeline_fetcher(host: Host) -> Result<Box<dyn FetchStrategy>> {
    let strategy = if let Some(strategy) = FETCH_STRATEGY.get(&host) {
        strategy.clone()
    } else {
        get_remote_software(&host.base_url()).await?
    };

    match strategy {
        RemoteSoftware::MisskeyDirect => {
            debug!("Host {} seems to be running Misskey.", &host.base_url());
            Ok(Box::new(MisskeyDirectFetching::new(host.clone())))
        }
    }
}

pub async fn get_timeline(host: Host, option: FetchOption) -> Result<Vec<Post>> {
    let mut fetcher = get_timeline_fetcher(host).await?;

    fetcher.get(&option).await
}
