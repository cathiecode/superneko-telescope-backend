use anyhow::{anyhow, Result};
use url::Url;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Host {
    base_url: Url,
}

impl Host {
    pub fn new(url: Url) -> Result<Self> {
        // FIXME: check url
        if url.host().is_none() {
            return Err(anyhow!("host should be provided"));
        }

        Ok(Self { base_url: url })
    }
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }
    pub fn host(&self) -> &str {
        &self.base_url.host_str().unwrap()
    }
}

#[allow(non_snake_case)]
pub mod json {
    use std::any::Any;

    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Instance {
        pub name: String,
        pub softwareName: String,
        pub iconUrl: String,
        pub faviconUrl: String,
        pub themeColor: String,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct User {
        pub name: Option<String>,
        pub username: String,
        pub host: String,
        //pub instance: Instance,
        pub avatarUrl: Option<String>,
        pub avatarBlurhash: Option<String>
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Post {
        pub visibility: String, // FIXME: enumへ
        pub remoteId: String,
        pub user: User,
        pub text: Option<String>,
        pub localOnly: bool,
        pub uri: Option<String>,
        pub createdAt: String,
        pub files: Vec<serde_json::Value>, // FIXME
        pub fileIds: Vec<String>,
        pub renote: Option<Box<Post>>,
        pub renoteId: Option<String>
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct FetchOption {
        pub limit: u32,
        pub untilId: Option<String>,
        pub server: String
    }
}
