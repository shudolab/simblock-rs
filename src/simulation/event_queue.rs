//! Priority-queue entry used by [`super::Simulation`]. Earlier `at_ms` (and earlier sequence on
//! ties) pops first, via `BinaryHeap`-with-`Reverse`.
//!
//! Cancellation uses a tombstone set keyed on the monotonically-increasing `seq` assigned at
//! enqueue time. `cancel(seq)` marks the entry as stale in O(1); a stale entry is transparently
//! skipped when it would otherwise be popped.

use crate::event::SimEvent;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct TimedEvent {
    pub key: (Reverse<u64>, Reverse<u64>),
    pub inner: SimEvent,
}

impl TimedEvent {
    pub(super) fn at_ms(&self) -> u64 {
        self.key.0 .0
    }

    pub(super) fn seq(&self) -> u64 {
        self.key.1 .0
    }
}

impl Ord for TimedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl PartialOrd for TimedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Min-heap of [`TimedEvent`]s with an O(1) tombstone-based cancellation facility.
#[derive(Debug, Default)]
pub(super) struct EventQueue {
    heap: BinaryHeap<TimedEvent>,
    seq: u64,
    cancelled: HashSet<u64>,
}

impl EventQueue {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Schedule `ev` at `at_ms`, returning the `(at_ms, seq)` tuple the caller may later hand to
    /// [`Self::cancel`] to invalidate this particular entry.
    pub(super) fn push(&mut self, at_ms: u64, ev: SimEvent) -> (u64, u64) {
        self.seq += 1;
        let seq = self.seq;
        self.heap.push(TimedEvent {
            key: (Reverse(at_ms), Reverse(seq)),
            inner: ev,
        });
        (at_ms, seq)
    }

    /// Mark the event with this sequence as cancelled. Safe to call with a sequence that has
    /// already been popped/unknown; the tombstone is only consulted on pop.
    pub(super) fn cancel(&mut self, seq: u64) {
        self.cancelled.insert(seq);
    }

    /// Pop the earliest live event, transparently skipping cancelled entries.
    pub(super) fn pop_live(&mut self) -> Option<TimedEvent> {
        while let Some(te) = self.heap.pop() {
            if self.cancelled.remove(&te.seq()) {
                continue;
            }
            return Some(te);
        }
        None
    }

    /// Drain all events, dropping the cancellation bookkeeping.
    pub(super) fn clear(&mut self) {
        self.heap.clear();
        self.cancelled.clear();
    }

    /// Currently queued (including tombstoned) events — matches the public
    /// [`super::Simulation::pending_events`] contract.
    pub(super) fn len(&self) -> usize {
        self.heap.len()
    }
}
