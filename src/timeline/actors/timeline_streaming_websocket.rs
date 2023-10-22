use actix::{Actor, Addr, Handler, StreamHandler, AsyncContext, Message};
use actix_web_actors::ws::{self, WebsocketContext};
use async_trait::async_trait;
use log::error;

use crate::types::Host;

use super::timeline_streaming::{
    RequestTimelineStreamer, TimelineMessage, TimelineStreamSupervisorActor, TimelineStreamerActor, RequestTimelineStream, HEARTBEAT_DURATION, HeartbeatMessage,
};

pub struct TimelineStreamWebsocket {
    stream_supervisor: Addr<TimelineStreamSupervisorActor>,
    stream: Option<Addr<TimelineStreamerActor>>,
    host: Host,
}

impl Actor for TimelineStreamWebsocket {
    type Context = WebsocketContext<Self>;
}

impl TimelineStreamWebsocket {
    pub fn new(host: Host, stream_supervisor: Addr<TimelineStreamSupervisorActor>) -> Self {
        Self {
            stream_supervisor,
            host,
            stream: None,
        }
    }
}

#[async_trait]
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for TimelineStreamWebsocket {
    fn started(&mut self, ctx: &mut Self::Context) {
        let host = self.host.clone();
        let supervisor = self.stream_supervisor.clone();
        let self_addr = ctx.address();
        actix::spawn(async move {
            let stream_or_send_error = supervisor
                .send(RequestTimelineStreamer::new(host))
                .await;

            if let Ok(Ok(stream)) = stream_or_send_error {
                let result = stream.send(RequestTimelineStream::new(self_addr.clone().recipient())).await;

                if result.is_ok() {
                    self_addr.do_send(SetStreamerAddr { addr: stream });
                } else {
                    // TODO: Close
                    //ctx.close(Some(CloseCode::Abnormal.into()));
                }
            } else {
                // TODO: Close
                error!("{:?}", stream_or_send_error);
            }
        });

        ctx.run_interval(HEARTBEAT_DURATION, |actor, ctx| {
            if let Some(stream_addr) = &actor.stream {
                stream_addr.do_send(HeartbeatMessage::new(ctx.address().recipient()))
            }
        });
    }

    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match item {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(_) => (),
            Err(_) => todo!(),
        }
    }
}

impl Handler<TimelineMessage> for TimelineStreamWebsocket {
    type Result = ();

    fn handle(&mut self, msg: TimelineMessage, ctx: &mut Self::Context) -> Self::Result {
        ctx.text(serde_json::to_string(msg.get()).unwrap());  // NOTE: できなかったらプログラムミス
    }
}

struct SetStreamerAddr{addr:Addr<TimelineStreamerActor>}

impl Message for SetStreamerAddr {
    type Result = ();
}

impl Handler<SetStreamerAddr> for TimelineStreamWebsocket {
    type Result = ();

    fn handle(&mut self, msg: SetStreamerAddr, _ctx: &mut Self::Context) -> Self::Result {
        self.stream = Some(msg.addr);
    }
}

struct StreamReply {
    json: serde_json::Value
}

impl Message for StreamReply {
    type Result = ();
}

impl Handler<StreamReply> for TimelineStreamWebsocket {
    type Result = ();

    fn handle(&mut self, msg: StreamReply, ctx: &mut Self::Context) -> Self::Result {
        let str = serde_json::to_string(&msg.json).unwrap(); // NOTE: できなかったらプログラムミス
        ctx.text(str)
    }
}