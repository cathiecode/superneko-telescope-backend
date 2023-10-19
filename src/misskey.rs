use anyhow::{anyhow, Result};
use async_stream::stream;
use async_trait::async_trait;
use futures_util::{SinkExt, Stream, StreamExt};
use log::{debug, error, log_enabled, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    http_client,
    timeline::{fetch::FetchStrategy, stream::StreamingStrategy},
    types::{json::Post, Host},
};

#[derive(Serialize, Deserialize)]
struct MisskeyChannelMessage {
    body: Post,
}

#[derive(Serialize, Deserialize)]
struct MisskeyWsMessage {
    body: MisskeyChannelMessage,
}

pub struct MisskeyDirectStreaming {
    stream: Box<dyn Stream<Item = Post> + Unpin>,
}

impl StreamingStrategy for MisskeyDirectStreaming {
    fn get_streategy_id(&self) -> &'static str {
        "misskey_direct_streaming"
    }

    fn get_stream(self: Box<Self>) -> Box<dyn Stream<Item = Post> + Unpin> {
        self.stream
    }
}

impl MisskeyDirectStreaming {
    pub async fn new(host: Host) -> Result<Self> {
        let mut host = host.base_url().clone();
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
                                Ok(item) => yield item.body.body,
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

    async fn get(&mut self, option: &crate::timeline::fetch::FetchOption) -> Result<Vec<Post>> {
        let url = self.host.base_url().join("api/")?.join("notes/")?.join("local-timeline")?;
        debug!("Fetching from {}", url);
        let request = reqwest::Client::new().post(url).body(format!(
            "{{\"limit\": {limit}, \"untilId\": \"{until_id}\"}}",
            limit = option.get_limit(),
            until_id = option
                .get_until_id()
                .unwrap_or("undefined")
                .replace("\"", "")
        )).header("Content-Type", "application/json");

        let response = http_client::request(request).await?;


        let body = response.text().await?;
        let result = body.as_str();

        debug!("{}", result);

        Ok(serde_json::from_str::<Vec<Post>>(result)?)
    }
}
