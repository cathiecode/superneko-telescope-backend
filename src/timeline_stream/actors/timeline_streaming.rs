use std::{
    collections::HashMap,
    time::{self, Duration},
};

use actix::{Actor, ActorContext, Addr, AsyncContext, Context, Handler, Message, StreamHandler};
use futures_util::Stream;

use crate::{Timeline, types::Host};

static HEARTBEAT_DURATION: Duration = Duration::from_secs(1);
static HEARTBEAT_DURATION_TRELANCE: Duration = Duration::from_secs(2);

// Actors
pub struct TimelineStreamSupervisorActor {
  streamers: HashMap<Host, Addr<TimelineStreamerActor>>
}

impl Actor for TimelineStreamSupervisorActor {
    type Context = Context<Self>;
}

impl Handler<TimelineStreamerStopMessage> for TimelineStreamSupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: TimelineStreamerStopMessage, ctx: &mut Self::Context) -> Self::Result {
        let mut host_to_remove = None;

        // TODO: 最後にStreamerを誰かに紹介したら削除させない

        for (host, addr) in &self.streamers {
          if addr == &msg.addr {
            host_to_remove = Some(host.clone());
          }
        }
        
        if let Some(host) = host_to_remove {
          self.streamers.remove(&host);
        }
    }
}

pub struct TimelineStreamerActor {
    supervisor: Addr<TimelineStreamSupervisorActor>,
    last_heartbeat: HashMap<Addr<TimelineStreamReceiverActor>, time::Instant>,
}

impl TimelineStreamerActor {
  pub fn new(timeline_stream: impl Stream<Item = Vec<Timeline>> + 'static, supervisor: Addr<TimelineStreamSupervisorActor>) -> Addr<Self> {
    Self::create(|ctx| {
      ctx.add_stream(timeline_stream);

      TimelineStreamerActor { last_heartbeat: HashMap::new(), supervisor }
    })
  }
}

impl Actor for TimelineStreamerActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(Duration::from_secs(60), |actor, ctx| {
            let mut remove_list = Vec::new();

            for (receiver, last_heartbeat) in &actor.last_heartbeat {
                if last_heartbeat.elapsed() > HEARTBEAT_DURATION_TRELANCE {
                    remove_list.push(receiver.clone());
                }
            }

            for to_remove in remove_list {
                actor.last_heartbeat.remove(&to_remove);
            }

            if actor.last_heartbeat.len() == 0 {
                ctx.stop();
            }
        });
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> actix::Running {
        // TODO: Check supervisor state before stop
        self.supervisor.do_send(TimelineStreamerStopMessage { addr: ctx.address() });
        actix::Running::Stop
    }
}

impl StreamHandler<Vec<Timeline>> for TimelineStreamerActor {
    fn handle(&mut self, item: Vec<Timeline>, ctx: &mut Self::Context) {
        todo!()
    }
}

impl Handler<HeartbeatMessage> for TimelineStreamerActor {
    type Result = ();

    fn handle(&mut self, msg: HeartbeatMessage, _: &mut Self::Context) -> Self::Result {
        self.last_heartbeat.insert(msg.addr, time::Instant::now());
    }
}

pub struct TimelineStreamReceiverActor {
    streamer: Addr<TimelineStreamerActor>,
}

impl Actor for TimelineStreamReceiverActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(HEARTBEAT_DURATION, |actor, ctx| {
            actor.streamer.do_send(HeartbeatMessage {
                addr: ctx.address().clone(),
            });
        });
    }
}

// Messages
struct HeartbeatMessage {
    addr: Addr<TimelineStreamReceiverActor>,
}

impl Message for HeartbeatMessage {
    type Result = ();
}

struct TimelineMessage {
    timeline: Vec<Timeline>,
}

impl Message for TimelineMessage {
    type Result = ();
}

struct TimelineStreamerStopMessage {
  addr: Addr<TimelineStreamerActor>
}

impl Message for TimelineStreamerStopMessage {
    type Result = ();
}