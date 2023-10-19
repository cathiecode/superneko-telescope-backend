use anyhow::Result;
use async_stream::stream;
use futures_util::{SinkExt, Stream, StreamExt};
use log::{debug, error, log_enabled, warn};
use serde_json::json;

use crate::{get_remote_software, http_client, types::Host, Strategy, Timeline, TimelineItem};

pub trait StreamingStrategy {
    fn get_streategy_id(&self) -> &'static str;
    fn get_stream(self: Box<Self>) -> Box<dyn Stream<Item = Timeline> + Unpin>;
}

struct MisskeyDirectStreaming {
    stream: Box<dyn Stream<Item = Timeline> + Unpin>,
}

impl StreamingStrategy for MisskeyDirectStreaming {
    fn get_streategy_id(&self) -> &'static str {
        "misskey_direct_streaming"
    }

    fn get_stream(self: Box<Self>) -> Box<dyn Stream<Item = Timeline> + Unpin> {
        self.stream
    }
}

impl MisskeyDirectStreaming {
    async fn new(host: Host) -> Result<Self> {
        let client = misskey::http::HttpClient::builder(host.base_url().clone().join("/api")?).build();
        let mut host = host.base_url().clone();
        host.set_scheme("wss").unwrap();
        let wsurl = host.join("/streaming").unwrap();

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

                                    match serde_json::from_str::<TimelineItem>(&text) {
                                        Ok(item) => yield vec![item],
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

                  yield Vec::new();
                });

        Ok(Self {
            stream: Box::new(stream),
        })
    }
}

pub async fn get_timeline_stream(host: Host) -> Result<Box<dyn StreamingStrategy>> {
    match get_remote_software(&host.base_url()).await? {
        Strategy::MisskeyDirect => {
            debug!("Host {} seems to be running Misskey.", &host.base_url());
            Ok(Box::new(MisskeyDirectStreaming::new(host).await?))
        }
    }
}
