use url::Url;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Host {
    base_url: Url,
}

impl Host {
    pub fn new(url: Url) -> Self {
        // FIXME: check url

        Self { base_url: url }
    }
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }
}

#[allow(non_snake_case)]
pub mod json {
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
        pub name: String,
        pub username: String,
        pub host: Option<String>,
        pub instance: Option<Instance>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Post {
        //created_at: Date,
        pub user: User,
        pub text: Option<String>,
        pub localOnly: bool,
        pub uri: Option<String>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct FetchOption {
        pub limit: u32,
        pub untilId: Option<String>,
    }
}

#[cfg(test)]
mod test {
    use super::json::Post;

    #[test]
    fn test_fetch() {
        let test_ltl = include_str!("test_ltl.json");

        serde_json::from_str::<Vec<Post>>(test_ltl).unwrap();
    }
}
