use std::io;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::cmp;
use std::time::Duration;

use manager::builder::PeerManagerBuilder;
use manager::peer_info::PeerInfo;
use manager::error::{PeerManagerError, PeerManagerErrorKind};

use futures::{StartSend, Poll, AsyncSink, Async};
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{self, Sender, Receiver};
use futures::task::{self as futures_task, Task};
use tokio_core::reactor::Handle;
use tokio_timer::{self, Timer};

pub mod builder;
pub mod peer_info;
pub mod error;

mod future;
mod task;

// We configure our tick duration based on this, could let users configure this in the future...
const DEFAULT_TIMER_SLOTS: usize = 2048;

/// Manages a set of peers with heartbeating heartbeating.
pub struct PeerManager<P> where P: Sink + Stream {
    handle:   Handle,
    timer:    Timer,
    build:    PeerManagerBuilder,
    send:     Sender<OPeerManagerMessage<P::Item>>,
    peers:    HashMap<PeerInfo, Sender<IPeerManagerMessage<P>>>,
    recv:     Receiver<OPeerManagerMessage<P::Item>>,
    opt_task: Option<Task>
}

impl<P> PeerManager<P>
    where P: Sink<SinkError=io::Error> +
             Stream<Error=io::Error>,
          P::SinkItem: ManagedMessage,
          P::Item:     ManagedMessage {
    /// Create a new `PeerManager` from the given `PeerManagerBuilder`.
    pub fn from_builder(builder: PeerManagerBuilder, handle: Handle) -> PeerManager<P> {
        // We use one timer for manager heartbeat intervals, and one for peer heartbeat timeouts
        let maximum_timers = builder.peer_capacity() * 2;
        let pow_maximum_timers = if maximum_timers & (maximum_timers - 1) == 0 {
            maximum_timers
        } else {
            maximum_timers.next_power_of_two()
        };

        // Figure out the right tick duration to get num slots of 2048.
        // TODO: We could probably let users change this in the future...
        let max_duration = cmp::max(builder.heartbeat_interval(), builder.heartbeat_timeout());
        let tick_duration = Duration::from_millis(max_duration.as_secs() * 1000 / DEFAULT_TIMER_SLOTS + 1);
        let timer = tokio_timer::wheel()
            .tick_duration(tick_duration)
            .max_capacity(pow_maximum_timers + 1)
            .channel_capacity(pow_maximum_timers)
            .num_slots(DEFAULT_TIMER_SLOTS)
            .build();
        
        let (res_send, res_recv) = mpsc::channel(builder.stream_buffer_capacity());

        PeerManager{ handle: handle, timer: timer, build: builder, send: res_send, peers: HashMap::new(), recv: res_recv, opt_task: None }
    }
}

impl<P> Sink for PeerManager<P>
    where P: Sink<SinkError=io::Error> +
             Stream<Error=io::Error> +
             'static,
          P::SinkItem: ManagedMessage,
          P::Item:     ManagedMessage {
    type SinkItem = IPeerManagerMessage<P>;
    type SinkError = PeerManagerError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match item {
            IPeerManagerMessage::AddPeer(info, peer) => {
                if self.peers.len() >= self.build.peer_capacity() {
                    self.opt_task = Some(futures_task::current());

                    return Ok(AsyncSink::NotReady(IPeerManagerMessage::AddPeer(info, peer)))
                }

                match self.peers.entry(info) {
                    Entry::Occupied(_) => Err(PeerManagerError::from_kind(PeerManagerErrorKind::PeerNotFound{ info: info })),
                    Entry::Vacant(vac) => {
                        vac.insert(task::run_peer(peer, info, self.send.clone(), self.timer.clone(), &self.build, &self.handle));

                        Ok(AsyncSink::Ready)
                    }
                }
            },
            IPeerManagerMessage::RemovePeer(info) => {
                self.peers.get_mut(&info)
                    .ok_or_else(|| PeerManagerError::from_kind(PeerManagerErrorKind::PeerNotFound{ info: info }))
                    .and_then(|send| send.start_send(IPeerManagerMessage::RemovePeer(info))
                                         .map_err(|_| panic!("bip_peer: PeerManager Failed To Send RemovePeer"))
                    )
            },
            IPeerManagerMessage::SendMessage(info, mid, peer_message) => {
                self.peers.get_mut(&info)
                    .ok_or_else(|| PeerManagerError::from_kind(PeerManagerErrorKind::PeerNotFound{ info: info }))
                    .and_then(|send| send.start_send(IPeerManagerMessage::SendMessage(info, mid, peer_message))
                                         .map_err(|_| panic!("bip_peer: PeerManager Failed to Send SendMessage"))
                    )
            }
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        for peer_mut in self.peers.values_mut() {
            // Needs type hint in case poll fails (so that error type matches)
            let result: Poll<(), Self::SinkError> = peer_mut
                .poll_complete()
                .map_err(|_| panic!("bip_peer: PeerManaged Failed To Poll Peer"));

            try!(result);
        }

        Ok(Async::Ready(()))
    }
}

impl<P> Stream for PeerManager<P>
    where P: Sink +
             Stream {
    type Item = OPeerManagerMessage<P::Item>;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Intercept and propogate any messages indicating the peer shutdown so we can remove them from our peer map
        self.recv.poll()
            .map(|result| {
                match result {
                    Async::Ready(Some(OPeerManagerMessage::PeerRemoved(info))) => {
                        self.peers.remove(&info).unwrap_or_else(|| panic!("bip_peer: Received PeerRemoved Message With No Matching Peer In Map"));
                        self.opt_task.take().map(|task| task.notify());

                        Async::Ready(Some(OPeerManagerMessage::PeerRemoved(info)))
                    },
                    Async::Ready(Some(OPeerManagerMessage::PeerDisconnect(info))) => {
                        self.peers.remove(&info).unwrap_or_else(|| panic!("bip_peer: Received PeerDisconnect Message With No Matching Peer In Map"));
                        self.opt_task.take().map(|task| task.notify());

                        Async::Ready(Some(OPeerManagerMessage::PeerDisconnect(info)))
                    },
                    Async::Ready(Some(OPeerManagerMessage::PeerError(info, error))) => {
                        self.peers.remove(&info).unwrap_or_else(|| panic!("bip_peer: Received PeerError Message With No Matching Peer In Map"));
                        self.opt_task.take().map(|task| task.notify());

                        Async::Ready(Some(OPeerManagerMessage::PeerError(info, error)))
                    },
                    other => other
                }
            })
    }
}

//----------------------------------------------------------------------------//

/// Trait for giving `PeerManager` message information it needs.
///
/// For any `PeerProtocol` (or plain `Codec`), that wants to be managed
/// by `PeerManager`, it must ensure that it's message type implements
/// this trait so that we have the hooks necessary to manage the peer.
pub trait ManagedMessage {
    /// Retrieve a keep alive message variant.
    fn keep_alive() -> Self;

    /// Whether or not this message is a keep alive message.
    fn is_keep_alive(&self) -> bool;
}

//----------------------------------------------------------------------------//

/// Identifier for matching sent messages with received messages.
pub type MessageId = u64;

/// Message that can be sent to the `PeerManager`.
pub enum IPeerManagerMessage<P>
    where P: Sink {
    /// Add a peer to the peer manager.
    AddPeer(PeerInfo, P),
    /// Remove a peer from the peer manager.
    RemovePeer(PeerInfo),
    /// Send a message to a peer.
    SendMessage(PeerInfo, MessageId, P::SinkItem)
    // TODO: Support querying for statistics
}

/// Message that can be received from the `PeerManager`.
pub enum OPeerManagerMessage<M> {
    /// Message indicating a peer has been added to the peer manager.
    PeerAdded(PeerInfo),
    /// Message indicating a peer has been removed from the peer manager.
    PeerRemoved(PeerInfo),
    /// Message indicating a message has been sent to the given peer.
    SentMessage(PeerInfo, MessageId),
    /// Message indicating we have received a message from a peer.
    ReceivedMessage(PeerInfo, M),
    /// Message indicating a peer has disconnected from us.
    ///
    /// Same semantics as `PeerRemoved`, but the peer is not returned.
    PeerDisconnect(PeerInfo),
    /// Message indicating a peer errored out.
    ///
    /// Same semantics as `PeerRemoved`, but the peer is not returned.
    PeerError(PeerInfo, io::Error)
}