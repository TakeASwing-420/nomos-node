use super::types::*;

mod flat_overlay;
mod random_beacon;
pub use flat_overlay::*;
pub use random_beacon::*;

use std::marker::Send;

pub trait Overlay: Clone {
    type Settings: Clone + Send + Sync + 'static;
    type LeaderSelection: LeaderSelection + Clone + Send + Sync + 'static;

    fn new(settings: Self::Settings) -> Self;
    fn root_committee(&self) -> Committee;
    fn rebuild(&mut self, timeout_qc: TimeoutQc);
    fn is_member_of_child_committee(&self, parent: NodeId, child: NodeId) -> bool;
    fn is_member_of_root_committee(&self, id: NodeId) -> bool;
    fn is_member_of_leaf_committee(&self, id: NodeId) -> bool;
    fn is_child_of_root_committee(&self, id: NodeId) -> bool;
    fn parent_committee(&self, id: NodeId) -> Committee;
    fn child_committees(&self, id: NodeId) -> Vec<Committee>;
    fn leaf_committees(&self, id: NodeId) -> Vec<Committee>;
    fn node_committee(&self, id: NodeId) -> Committee;
    fn next_leader(&self) -> NodeId;
    fn super_majority_threshold(&self, id: NodeId) -> usize;
    fn leader_super_majority_threshold(&self, id: NodeId) -> usize;
    fn update_leader_selection<F, E>(&self, f: F) -> Result<Self, E>
    where
        F: FnOnce(Self::LeaderSelection) -> Result<Self::LeaderSelection, E>;
}

pub trait LeaderSelection: Clone {
    fn next_leader(&self, nodes: &[NodeId]) -> NodeId;
}