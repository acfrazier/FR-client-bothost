//! One game reactor, fixed initial-handshake lane, bounded byte custody, and
//! generation-scoped slot waits. The game transport is owned by this module;
//! DNS has its own single resolver worker.

pub mod budget;
pub mod cancel;
pub mod control;
pub mod health;
pub mod ledger;
pub mod park;
pub mod records;
pub mod ring;
pub mod reactor;
pub mod shared;
pub mod stream_handle;
pub mod ws_frames;
pub mod tcp;
pub mod tcp_endpoint;

pub mod handoff;
pub mod resolver;
