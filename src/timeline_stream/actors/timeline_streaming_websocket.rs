use actix::{Actor, StreamHandler, Addr};
use actix_web_actors::ws::{WebsocketContext, self};

use super::timeline_streaming::TimelineStreamSupervisorActor;

struct TimelineStreamWebsocket;

impl Actor for TimelineStreamWebsocket {
    type Context = WebsocketContext<Self>;
}

impl TimelineStreamWebsocket {
  fn new(stream_supervisor: Addr<TimelineStreamSupervisorActor>) {
    
  }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for TimelineStreamWebsocket {
  fn started(&mut self, ctx: &mut Self::Context) {
      
  }

    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match item {
          Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
          Ok(_) => (),
          Err(_) => todo!(),
        }
    }
}
