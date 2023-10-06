use std::{str::FromStr, time::Duration};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use env_logger;
use futures_util::{stream::StreamExt, SinkExt};
use log::{debug, error, warn};
use serde::Deserialize;
use serde_json::json;
use url::Url;

struct GetTimelineOption {
    until: Option<i32>,
    since: Option<i32>,
}

type SingleChannelReceiver<T> = std::sync::mpsc::Receiver<T>;

type SingleChannelSender<T> = std::sync::mpsc::Sender<T>;

type Date = chrono::DateTime<chrono::Utc>;

#[derive(Clone, Debug)]
struct Instance {
    name: String,
    softwareName: String,
    iconUrl: String,
    faviconUrl: String,
    themeColor: String,
}

#[derive(Clone, Debug)]
struct User {
    name: String,
    username: String,
    host: String,
    instance: Instance,
}

#[derive(Clone, Debug)]
struct TimelineItem {
    createdAt: Date,
    user: User,
}

type Timeline = Vec<TimelineItem>;

struct TimelineStreamReceiver {}

#[async_trait]
trait StreamingStrategy {
    fn event_receiver<'a>(&'a mut self) -> &'a mut std::sync::mpsc::Receiver<Timeline>;
    fn get_strategy_id(&self) -> &'static str;
}

struct MisskeyDirectStreamingStrategy {
    receiver: SingleChannelReceiver<Timeline>,
}

#[derive(Debug, Deserialize)]
struct NodeInfoSoftware {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct NodeInfo {
    software: NodeInfoSoftware,
}

impl MisskeyDirectStreamingStrategy {
    async fn new(mut host: Url) -> Result<MisskeyDirectStreamingStrategy> {
        // NOTE: Constant literal changes does not returns error.
        host.set_scheme("wss").unwrap();
        let host = host.join("/streaming").unwrap();

        debug!("Connecting to {:?}", host.to_string());
        let (stream, _) = http_client::ws_connect_async(host).await?;
        let (mut write, mut read) = stream.split();
        let (event_sender, event_receiver) = std::sync::mpsc::channel();

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

        tokio::spawn(async move {
            // TODO: heartbeat
            debug!("Heartbeat");
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        tokio::spawn(async move {
            while let Some(item) = read.next().await {
                match item {
                    Ok(message) => match message {
                        tokio_tungstenite::tungstenite::Message::Text(text) => {
                            
                            if let Err(_) = event_sender.send(Vec::new()) {
                                return; // recvが終わってたら終了
                            }

                            let json = serde_json::from_str::<serde_json::value::Value>(&text);

                            match json {
                                Ok(json) => debug!("Received json {}", &json.to_string()),
                                Err(e) => debug!("Received non-json {:?}", &text),
                            }
                        }
                        _ => debug!("Received non-text {:?}", message),
                    },
                    Err(error) => {
                        // TODO: error
                        error!("{:?}", error);
                    }
                }
            }

            // TODO: disconnect
        });

        Ok(Self {
            receiver: event_receiver,
        })
    }
}

mod http_client {
    use std::{
        sync::Mutex,
        time::Duration,
    };

    use log::{debug, warn};
    use once_cell::sync::OnceCell;
    use reqwest;
    use tokio::net::TcpStream;
    use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::{Error, handshake::client::Response}};
    use url::Url;

    struct CircuitBreaker {
        read_count_per_sec: i32,
        overload_read_count_per_sec: i32,
        overload_trelance_sec: i32,
    }

    impl CircuitBreaker {
        fn global() -> &'static Mutex<CircuitBreaker> {
            static CIRCUIT_BREAKER_GLOBAL: OnceCell<Mutex<CircuitBreaker>> = OnceCell::new();

            if let Some(instance) = CIRCUIT_BREAKER_GLOBAL.get() {
                return instance;
            } else {
                debug!("Circuit breaker initialized.");
                let result = CIRCUIT_BREAKER_GLOBAL.set(Mutex::new(Self {
                    read_count_per_sec: 0,
                    overload_read_count_per_sec: 0,

                    overload_trelance_sec: 5,
                }));

                if let Err(_) = result {
                    CIRCUIT_BREAKER_GLOBAL.get().unwrap();
                }

                std::thread::spawn(|| {
                    // NOTE: Block for not to lock instance too long
                    loop {
                        {
                            let mut instance = CIRCUIT_BREAKER_GLOBAL.get().unwrap().lock().unwrap();

                            if instance.read_count_per_sec >= instance.overload_read_count_per_sec {
                                instance.overload_read_count_per_sec += 1;
                                instance.read_count_per_sec = 0;

                                warn!("Circuit breaker detects overload! Current overload seconds count is {}", instance.overload_read_count_per_sec);
                            }

                            if instance.overload_read_count_per_sec >= instance.overload_trelance_sec {
                                panic!("Too many requests! breaking circuit!");
                            }
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                });

                return CIRCUIT_BREAKER_GLOBAL.get().unwrap();
            }
        }

        fn readed() {
            let mut instance = Self::global().lock().unwrap();

            instance.read_count_per_sec += 1;
        }
    }

    pub async fn get(url: Url) -> reqwest::Result<reqwest::Response> {
        CircuitBreaker::readed();
        reqwest::get(url).await
    }

    pub async fn ws_connect_async(url: Url) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), Error> {
        tokio_tungstenite::connect_async(url).await
    }
}

enum Strategy {
    MisskeyDirect,
}

async fn get_remote_software(host: &Url) -> Result<Strategy> {
    let nodeinfo_text = http_client::get(host.join("nodeinfo/2.0")?)
        .await?
        .text()
        .await?;

    let nodeinfo_struct = serde_json::from_str::<NodeInfo>(&nodeinfo_text)?;

    match nodeinfo_struct.software.name.as_str() {
        "misskey" => return Ok(Strategy::MisskeyDirect),
        _ => {
            return Err(anyhow!(
                "The software".to_owned() + &nodeinfo_struct.software.name + " is not supported."
            ))
        }
    }
}

async fn get_stream(host: Url) -> Result<Box<dyn StreamingStrategy>> {
    match get_remote_software(&host).await? {
        Strategy::MisskeyDirect => {
            debug!("Host {} seems to be running Misskey.", &host);
            Ok(Box::new(MisskeyDirectStreamingStrategy::new(host).await?))
        }
    }
}

#[async_trait]
impl StreamingStrategy for MisskeyDirectStreamingStrategy {
    fn event_receiver<'a>(&'a mut self) -> &'a mut SingleChannelReceiver<Timeline> {
        &mut self.receiver
    }
    fn get_strategy_id(&self) -> &'static str {
        "misskey-direct-connect"
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut stream = get_stream(Url::from_str("https://superneko.net")?)
        .await
        .unwrap();
    while let Ok(message) = stream
        .event_receiver()
        .recv_timeout(Duration::from_secs(60))
    {
        println!("{:?}", message);
    }

    println!("Hello, world!");

    return Ok(());
}
