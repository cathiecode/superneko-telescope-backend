use actix::{Actor, Addr, Context};
use actix_cors::Cors;
use actix_web::{body::MessageBody, get, post, web, Either, HttpRequest, HttpResponse, Responder};
use actix_web_actors::ws;
use anyhow::{anyhow, Error, Result};

use env_logger;

use log::error;
use once_cell::sync::Lazy;

use serde::{Deserialize, Serialize};
use serde_json::json;
use timeline::actors::timeline_streaming::TimelineStreamSupervisorActor;

use types::json::FetchOption;
use url::Url;

use crate::{timeline::actors::timeline_streaming_websocket::TimelineStreamWebsocket, types::Host};

mod http_client;
mod misskey;
mod timeline;
mod types;

#[derive(Debug, Deserialize)]
struct NodeInfoMetaData {
    nodeName: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NodeInfoSoftware {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct NodeInfo {
    software: NodeInfoSoftware,
    metadata: NodeInfoMetaData,
}

#[derive(Debug, Clone, Serialize)]
enum RemoteSoftware {
    Misskey,
}

#[derive(Serialize)]
struct RemoteInfo {
    name: Option<String>,
    software: RemoteSoftware,
}

async fn get_remote_info(host: &Url) -> Result<RemoteInfo> {
    let nodeinfo_text = http_client::get(host.join("nodeinfo/2.0")?)
        .await?
        .text()
        .await?;

    let nodeinfo_struct = serde_json::from_str::<NodeInfo>(&nodeinfo_text)?;

    let software = match nodeinfo_struct.software.name.as_str() {
        "misskey" => RemoteSoftware::Misskey,
        _ => {
            return Err(anyhow!(
                "The software".to_owned() + &nodeinfo_struct.software.name + " is not supported."
            ))
        }
    };

    Ok(RemoteInfo {
        name: nodeinfo_struct.metadata.nodeName,
        software,
    })
}

/*#[async_trait]
impl StreamingStrategy for MisskeyDirectStreaming {
    fn event_receiver(self: Box<Self>) -> SingleChannelReceiver<Timeline> {
        self.receiver
    }
    fn get_strategy_id(&self) -> &'static str {
        "misskey-direct-connect"
    }
}*/

async fn authorized(req: &HttpRequest) -> bool {
    // TODO: miauth
    let _authorized = req
        .headers()
        .get("Authorized")
        .map(|authorized| authorized.to_str().unwrap_or(""))
        .unwrap_or("");

    return true;
}

struct StreamingClient;
struct StreamingError;

impl Actor for StreamingClient {
    type Context = Context<Self>;
}

struct TimelineStreamingActor {
    dest: Vec<Addr<StreamingClient>>,
}
/*
impl TimelineStreamingActor {
    pub fn start(timeline: std::pin::Pin<Box<dyn Stream<Item = Post>>>) -> Addr<Self> {
        Self::create(|ctx| {
            ctx.add_stream(timeline);
            Self { dest: Vec::new() }
        })
    }
}

impl Actor for TimelineStreamingActor {
    type Context = actix::Context<Self>;

    fn stopped(&mut self, ctx: &mut Self::Context) {}
}

impl StreamHandler<Timeline> for TimelineStreamingActor {
    fn handle(&mut self, item: Timeline, ctx: &mut Self::Context) {
        todo!()
    }
}

impl Handler<StreamingError> for TimelineStreamingActor {
    type Result = ();

    fn handle(&mut self, msg: StreamingError, ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}

impl Message for StreamingError {
    type Result = ();
}
 */

static TIMELINE_STREAM_WEBSOCKET_SUPERVISOR: Lazy<Addr<TimelineStreamSupervisorActor>> =
    Lazy::new(|| TimelineStreamSupervisorActor::new());

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().message_body(json!({"version": "0.0.0"}).to_string())
}

#[get("/api/stream/{server}")]
async fn stream(
    req: HttpRequest,
    stream: web::Payload,
    path: web::Path<(String,)>,
) -> actix_web::Result<HttpResponse, actix_web::error::Error> {
    if !authorized(&req).await {
        return Err(actix_web::error::ErrorForbidden("Authentication Required"));
    }

    let server = Host::new(
        path.into_inner().0
            .parse::<Url>()
            .map_err(|e| actix_web::error::ErrorBadRequest(e))?,
    )
    .map_err(|e| actix_web::error::ErrorBadRequest(e))?;

    Ok(ws::start(
        TimelineStreamWebsocket::new(server, TIMELINE_STREAM_WEBSOCKET_SUPERVISOR.clone()),
        &req,
        stream,
    )
    .map_err(|e| actix_web::error::ErrorUpgradeRequired(e))?)
}

#[post("/api/timeline")]
async fn get_timeline(
    option: web::Json<FetchOption>,
) -> actix_web::Result<HttpResponse, actix_web::error::Error> {
    let server = Host::new(
        option
            .server
            .parse::<Url>()
            .map_err(|e| actix_web::error::ErrorBadRequest(e))?,
    )
    .map_err(|e| actix_web::error::ErrorBadRequest(e))?;

    let result = timeline::fetch::get_timeline(
        server,
        option.into_inner().try_into().map_err(|e| {
            error!("{:?}", e);
            actix_web::error::ErrorInternalServerError(e)
        })?,
    )
    .await
    .map_err(|e| {
        error!("{:?}", e);
        actix_web::error::ErrorInternalServerError(e)
    })?;

    HttpResponse::Ok().message_body(
        serde_json::to_string(&result)
            .map_err(|e| {
                error!("{:?}", e);
                actix_web::error::ErrorInternalServerError(e)
            })?
            .boxed(),
    )
}

#[derive(Deserialize)]
struct FetchNodeinfoOption {
    host: String,
}

#[derive(Serialize)]
struct FetchNodeInfoResult {
    name: String,
}

#[post("/api/nodeinfo")]
async fn get_node_info(option: web::Json<FetchNodeinfoOption>) -> impl Responder {
    let url = option.host.parse().map_err(|e| {
        error!("{:?}", e);
        actix_web::error::ErrorInternalServerError(e)
    })?;

    let result = get_remote_info(&url).await.map_err(|e| {
        error!("{:?}", e);
        actix_web::error::ErrorInternalServerError(e)
    })?;

    HttpResponse::Ok().message_body(
        serde_json::to_string(&result)
            .map_err(|e| {
                error!("{:?}", e);
                actix_web::error::ErrorInternalServerError(e)
            })?
            .boxed(),
    )
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    actix_web::HttpServer::new(|| {
        let cors = Cors::default()
            .allowed_origin("http://localhost:3000") // TODO: 外からAllowed originを差し込めるようにする
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![
                actix_web::http::header::AUTHORIZATION,
                actix_web::http::header::ACCEPT,
            ])
            .allowed_header(actix_web::http::header::CONTENT_TYPE)
            .max_age(3600);

        actix_web::App::new()
            .wrap(cors)
            .service(index)
            .service(stream)
            .service(get_timeline)
            .service(get_node_info)
    })
    .bind(("127.0.0.1", 8000))?
    .run()
    .await

    /*

    let mut stream = get_stream(Url::from_str("https://superneko.net")?)
        .await
        .unwrap();

    let mut receiver = stream.event_receiver();
    while let Some(message) = receiver.recv().await {
        println!("{:?}", message);
    }

    println!("Hello, world!");


    return Ok(());
    */
}
