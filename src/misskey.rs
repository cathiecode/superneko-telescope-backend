use anyhow::Result;
use async_stream::stream;
use async_trait::async_trait;
use futures_util::{SinkExt, Stream, StreamExt};
use log::{debug, error, log_enabled, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    http_client,
    timeline::{fetch::FetchStrategy, stream::StreamingStrategy},
    types::{json, Host},
};

#[allow(non_snake_case)]
mod types {
    use anyhow::Result;
    use serde::{Deserialize, Serialize};

    use crate::types::{json, Host};

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Instance {
        pub name: String,
        pub softwareName: String,
        pub iconUrl: String,
        pub faviconUrl: String,
        pub themeColor: String,
    }

    impl Into<json::Instance> for Instance {
        fn into(self) -> json::Instance {
            json::Instance {
                name: self.name,
                softwareName: self.softwareName,
                iconUrl: self.iconUrl,
                faviconUrl: self.faviconUrl,
                themeColor: self.themeColor,
            }
        }
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct User {
        pub name: Option<String>,
        pub username: String,
        pub host: Option<String>,
        //pub instance: Option<Instance>,
        pub avatarUrl: Option<String>,
        pub avatarBlurhash: Option<String>,
    }

    impl User {
        pub fn try_into_json_user(self, host: &Host) -> Result<json::User> {
            Ok(json::User {
                name: self.name,
                username: self.username,
                host: host.host().to_string(),
                //instance: self.instance.map(|instance| instance.into()),
                avatarUrl: self.avatarUrl,
                avatarBlurhash: self.avatarBlurhash,
            })
        }
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Post {
        pub visibility: String,
        pub id: String,
        pub user: User,
        pub text: Option<String>,
        pub localOnly: bool,
        //pub uri: Option<String>,
        pub createdAt: String,
        pub files: Vec<serde_json::Value>, // FIXME
        pub fileIds: Vec<String>,
        pub renote: Option<Box<Post>>,
        pub renoteId: Option<String>,
    }

    impl Post {
        pub fn try_into_json_post(self, host: &Host) -> Result<json::Post> {
            let may_renote_or_error = self
                .renote
                .map(|renote| (*renote).try_into_json_post(host));

            let renote = match may_renote_or_error {
                Some(Ok(renote)) => Some(Box::new(renote)),
                _ => None
            };

            Ok(json::Post {
                visibility: self.visibility,
                uri: Some(host.base_url().join("notes/")?.join(&self.id)?.to_string()),
                remoteId: self.id,
                user: self.user.try_into_json_user(host)?,
                text: self.text,
                localOnly: self.localOnly,
                files: self.files,
                fileIds: self.fileIds,
                createdAt: self.createdAt,
                renoteId: if renote.is_some() {self.renoteId} else {None},
                renote,
            })
        }
    }

    #[cfg(test)]
    mod test {
        use super::Post;

        #[test]
        fn test_should_be_parsed() {
            let test_ltl = include_str!("test_ltl.json");

            serde_json::from_str::<Vec<Post>>(test_ltl).unwrap();
        }
    }
}

#[derive(Serialize, Deserialize)]
struct MisskeyChannelMessage {
    body: types::Post,
}

#[derive(Serialize, Deserialize)]
struct MisskeyWsMessage {
    body: MisskeyChannelMessage,
}

pub struct MisskeyDirectStreaming {
    stream: Box<dyn Stream<Item = json::Post> + Unpin>,
}

impl StreamingStrategy for MisskeyDirectStreaming {
    fn get_streategy_id(&self) -> &'static str {
        "misskey_direct_streaming"
    }

    fn get_stream(self: Box<Self>) -> Box<dyn Stream<Item = json::Post> + Unpin> {
        self.stream
    }
}

impl MisskeyDirectStreaming {
    pub async fn new(host: Host) -> Result<Self> {
        let orig_host = host;
        let mut host = orig_host.base_url().clone();
        host.set_scheme("wss").unwrap(); // NOTE: できなかったらプログラムミス
        let wsurl = host.join("/streaming").unwrap(); // NOTE: できなかったらプログラムミス

        debug!("Connecting to {:?}", host.to_string());
        let (mut write, mut read) = http_client::ws_connect_async(wsurl.clone())
            .await?
            .0
            .split();

        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({
                    "type": "connect",
                    "body": {
                        "channel": "globalTimeline", // TODO: localTimelineにする
                        "id": "ltl"
                    }
                })
                .to_string(),
            ))
            .await?;

        let stream = Box::pin(stream! {
          debug!("Receive thread start!");

          while let Some(item) = read.next().await {
              match item {
                  Ok(message) => match message {
                      tokio_tungstenite::tungstenite::Message::Text(text) => {
                            if log_enabled!(log::Level::Debug) {
                                let json = serde_json::from_str::<serde_json::value::Value>(&text);

                                match json {
                                    Ok(json) => debug!("Received json {}", &json.to_string()),
                                    Err(_) => debug!("Received non-json {:?}", &text),
                                };
                            }

                            match serde_json::from_str::<MisskeyWsMessage>(&text) {
                                Ok(item) => {
                                    if let Ok(item) = item.body.body.try_into_json_post(&orig_host) {
                                        yield item;
                                    }
                                },
                                Err(e) => warn!("Received imcompatible message {} from {} with error {}", &text, &host, e),
                            }
                      }
                      _ => debug!("Received non-text {:?}", message),
                  },
                  Err(error) => {
                      // TODO: error
                      error!("{:?}", error);
                  }
              };
          }

          debug!("Disconnected from {}", &wsurl);
        });

        Ok(Self {
            stream: Box::new(stream),
        })
    }
}

pub struct MisskeyDirectFetching {
    host: Host,
}

impl MisskeyDirectFetching {
    pub fn new(host: Host) -> Self {
        Self { host }
    }
}

#[async_trait]
impl FetchStrategy for MisskeyDirectFetching {
    fn get_strategy_id(&self) -> &'static str {
        "misskey-direct"
    }

    async fn get(
        &mut self,
        option: &crate::timeline::fetch::FetchOption,
    ) -> Result<Vec<crate::types::json::Post>> {
        let url = self
            .host
            .base_url()
            .join("api/")?
            .join("notes/")?
            .join("local-timeline")?;
        debug!("Fetching from {}", url);
        let request = reqwest::Client::new()
            .post(url)
            .body(format!(
                "{{\"limit\": {limit}, \"untilId\": \"{until_id}\"}}",
                limit = option.get_limit(),
                until_id = option
                    .get_until_id()
                    .unwrap_or("undefined")
                    .replace("\"", "")
            ))
            .header("Content-Type", "application/json");

        let response = http_client::request(request).await?;

        let body = response.text().await?;
        let result = body.as_str();

        debug!("{}", result);

        Ok(serde_json::from_str::<Vec<types::Post>>(result)?
            .into_iter()
            .map(|item| item.try_into_json_post(&self.host))
            .filter_map(|item| item.ok())
            .collect())
    }
}
