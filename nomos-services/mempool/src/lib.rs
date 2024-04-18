pub mod backend;
pub mod da;
pub mod network;
pub mod tx;

use backend::Status;
use overwatch_rs::services::relay::RelayMessage;
use std::fmt::{Debug, Error, Formatter};
use tokio::sync::oneshot::Sender;

pub use da::service::{DaMempoolService, DaMempoolSettings};
pub use tx::service::{TxMempoolService, TxMempoolSettings};

pub enum MempoolMsg<BlockId, Item, Key> {
    Add {
        item: Item,
        key: Key,
        reply_channel: Sender<Result<(), ()>>,
    },
    View {
        ancestor_hint: BlockId,
        reply_channel: Sender<Box<dyn Iterator<Item = Item> + Send>>,
    },
    Prune {
        ids: Vec<Key>,
    },
    #[cfg(test)]
    BlockItems {
        block: BlockId,
        reply_channel: Sender<Option<Box<dyn Iterator<Item = Item> + Send>>>,
    },
    MarkInBlock {
        ids: Vec<Key>,
        block: BlockId,
    },
    Metrics {
        reply_channel: Sender<MempoolMetrics>,
    },
    Status {
        items: Vec<Key>,
        reply_channel: Sender<Vec<Status<BlockId>>>,
    },
}

impl<BlockId, Item, Key> Debug for MempoolMsg<BlockId, Item, Key>
where
    BlockId: Debug,
    Item: Debug,
    Key: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            Self::View { ancestor_hint, .. } => {
                write!(f, "MempoolMsg::View {{ ancestor_hint: {ancestor_hint:?}}}")
            }
            Self::Add { item, .. } => write!(f, "MempoolMsg::Add{{item: {item:?}}}"),
            Self::Prune { ids } => write!(f, "MempoolMsg::Prune{{ids: {ids:?}}}"),
            Self::MarkInBlock { ids, block } => {
                write!(
                    f,
                    "MempoolMsg::MarkInBlock{{ids: {ids:?}, block: {block:?}}}"
                )
            }
            #[cfg(test)]
            Self::BlockItems { block, .. } => {
                write!(f, "MempoolMsg::BlockItem{{block: {block:?}}}")
            }
            Self::Metrics { .. } => write!(f, "MempoolMsg::Metrics"),
            Self::Status { items, .. } => write!(f, "MempoolMsg::Status{{items: {items:?}}}"),
        }
    }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MempoolMetrics {
    pub pending_items: usize,
    pub last_item_timestamp: u64,
}

impl<BlockId: 'static, Item: 'static, Key: 'static> RelayMessage
    for MempoolMsg<BlockId, Item, Key>
{
}
