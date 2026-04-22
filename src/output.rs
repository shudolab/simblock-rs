//! JSON event log for visualization tools.

use crate::types::NodeId;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;

#[derive(Debug, Default, Serialize)]
pub struct EventLog {
    pub events: Vec<Value>,
}

impl EventLog {
    pub fn push(&mut self, v: Value) {
        self.events.push(v);
    }

    pub fn write_array(&self, w: &mut impl Write) -> std::io::Result<()> {
        write!(w, "[")?;
        let mut first = true;
        for e in &self.events {
            if !first {
                write!(w, ",")?;
            }
            first = false;
            write!(w, "{}", e)?;
        }
        write!(w, "]")?;
        Ok(())
    }
}

pub fn ev_add_node(ts: u64, node: NodeId, region: usize) -> Value {
    json!({
      "kind": "add-node",
      "content": {
        "timestamp": ts,
        "node-id": node,
        "region-id": region
      }
    })
}

pub fn ev_add_link(ts: u64, begin: NodeId, end: NodeId) -> Value {
    json!({
      "kind": "add-link",
      "content": {
        "timestamp": ts,
        "begin-node-id": begin,
        "end-node-id": end
      }
    })
}

pub fn ev_add_block(ts: u64, node: NodeId, block_id: u64) -> Value {
    json!({
      "kind": "add-block",
      "content": {
        "timestamp": ts,
        "node-id": node,
        "block-id": block_id
      }
    })
}

pub fn ev_flow_block(
    ts_send: u64,
    ts_recv: u64,
    begin: NodeId,
    end: NodeId,
    block_id: u64,
) -> Value {
    json!({
      "kind": "flow-block",
      "content": {
        "transmission-timestamp": ts_send,
        "reception-timestamp": ts_recv,
        "begin-node-id": begin,
        "end-node-id": end,
        "block-id": block_id
      }
    })
}

pub fn ev_simulation_end(ts: u64) -> Value {
    json!({
      "kind": "simulation-end",
      "content": { "timestamp": ts }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_log_write_array_is_valid_json() {
        let mut log = EventLog::default();
        log.push(ev_add_node(0, 1, 0));
        log.push(ev_simulation_end(42));
        let mut buf = Vec::new();
        log.write_array(&mut buf).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 2);
    }
}
