use std::any::Any;
use std::time::Duration;

use crate::TimeoutResult;
use bip_handshake::transports::TcpTransport;
use bip_handshake::{
    DiscoveryInfo, FilterDecision, HandshakeFilter, HandshakeFilters, HandshakerBuilder,
    InitiateMessage, Protocol,
};

use bip_util::bt::{self, InfoHash};
use futures::sink::Sink;
use futures::stream::Stream;
use futures::Future;
use tokio_core::reactor::{Core, Timeout};

#[derive(PartialEq, Eq)]
pub struct FilterBlockAllHash;

impl HandshakeFilter for FilterBlockAllHash {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_hash(&self, _opt_hash: Option<&InfoHash>) -> FilterDecision {
        FilterDecision::Block
    }
}

#[derive(PartialEq, Eq)]
pub struct FilterAllowHash {
    hash: InfoHash,
}

impl HandshakeFilter for FilterAllowHash {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_hash(&self, opt_hash: Option<&InfoHash>) -> FilterDecision {
        opt_hash
            .map(|hash| {
                if hash == &self.hash {
                    FilterDecision::Allow
                } else {
                    FilterDecision::Pass
                }
            })
            .unwrap_or(FilterDecision::NeedData)
    }
}

#[test]
fn test_filter_whitelist_diff_data() {
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let mut handshaker_one_addr = "127.0.0.1:0".parse().unwrap();
    let handshaker_one_pid = [4u8; bt::PEER_ID_LEN].into();

    let handshaker_one = HandshakerBuilder::new()
        .with_bind_addr(handshaker_one_addr)
        .with_peer_id(handshaker_one_pid)
        .build(TcpTransport, core.handle())
        .unwrap();

    handshaker_one_addr.set_port(handshaker_one.port());
    // Filter all incoming handshake requests hash's, then whitelist
    let allow_info_hash = [55u8; bt::INFO_HASH_LEN].into();

    handshaker_one.add_filter(FilterBlockAllHash);
    handshaker_one.add_filter(FilterAllowHash {
        hash: allow_info_hash,
    });

    let mut handshaker_two_addr = "127.0.0.1:0".parse().unwrap();
    let handshaker_two_pid = [5u8; bt::PEER_ID_LEN].into();

    let handshaker_two = HandshakerBuilder::new()
        .with_bind_addr(handshaker_two_addr)
        .with_peer_id(handshaker_two_pid)
        .build(TcpTransport, core.handle())
        .unwrap();

    handshaker_two_addr.set_port(handshaker_two.port());

    let (_, stream_one) = handshaker_one.into_parts();
    let (sink_two, stream_two) = handshaker_two.into_parts();

    let timeout_result = core
        .run(
            sink_two
                .send(InitiateMessage::new(
                    Protocol::BitTorrent,
                    [54u8; bt::INFO_HASH_LEN].into(),
                    handshaker_one_addr,
                ))
                .map_err(|_| ())
                .and_then(|_| {
                    let timeout = Timeout::new(Duration::from_millis(50), &handle)
                        .unwrap()
                        .map(|_| TimeoutResult::TimedOut)
                        .map_err(|_| ());

                    let result_one = stream_one
                        .into_future()
                        .map(|_| TimeoutResult::GotResult)
                        .map_err(|_| ());
                    let result_two = stream_two
                        .into_future()
                        .map(|_| TimeoutResult::GotResult)
                        .map_err(|_| ());

                    result_one
                        .select(result_two)
                        .map(|_| TimeoutResult::GotResult)
                        .map_err(|_| ())
                        .select(timeout)
                        .map(|(item, _)| item)
                        .map_err(|_| ())
                }),
        )
        .unwrap();

    assert_eq!(TimeoutResult::TimedOut, timeout_result);
}
