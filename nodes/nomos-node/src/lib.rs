use bytes::Bytes;
use color_eyre::eyre::Result;
use overwatch_derive::*;
use overwatch_rs::services::handle::ServiceHandle;
use serde::{de::DeserializeOwned, Serialize};

use api::AxumBackend;
pub use config::{Config, CryptarchiaArgs, HttpArgs, LogArgs, MetricsArgs, NetworkArgs};
use kzgrs_backend::common::attestation::Attestation;
use kzgrs_backend::common::blob::DaBlob;
use kzgrs_backend::dispersal::{Certificate, VidCertificate};
use nomos_api::ApiService;
use nomos_core::{da::certificate, header::HeaderId, tx::Transaction, wire};
pub use nomos_core::{
    da::certificate::select::FillSize as FillSizeWithBlobsCertificate,
    tx::select::FillSize as FillSizeWithTx,
};
use nomos_da_verifier::backend::kzgrs::KzgrsDaVerifier;
#[cfg(feature = "tracing")]
use nomos_log::Logger;
use nomos_mempool::da::service::DaMempoolService;
use nomos_mempool::da::verify::kzgrs::DaVerificationProvider as MempoolVerificationProvider;
use nomos_mempool::network::adapters::libp2p::Libp2pAdapter as MempoolNetworkAdapter;
use nomos_mempool::{backend::mockpool::MockPool, TxMempoolService};
#[cfg(feature = "metrics")]
use nomos_metrics::Metrics;
use nomos_network::backends::libp2p::Libp2p as NetworkBackend;
use nomos_network::NetworkService;
use nomos_storage::{
    backends::{rocksdb::RocksBackend, StorageSerde},
    StorageService,
};
use nomos_system_sig::SystemSig;
pub use tx::Tx;

pub mod api;
mod config;
mod tx;

pub type NomosApiService = ApiService<
    AxumBackend<
        Attestation,
        DaBlob,
        Certificate,
        VidCertificate,
        MempoolVerificationProvider,
        KzgrsDaVerifier,
        Tx,
        Wire,
        MB16,
    >,
>;

pub const CL_TOPIC: &str = "cl";
pub const DA_TOPIC: &str = "da";
const MB16: usize = 1024 * 1024 * 16;

pub type Cryptarchia = cryptarchia_consensus::CryptarchiaConsensus<
    cryptarchia_consensus::network::adapters::libp2p::LibP2pAdapter<Tx, VidCertificate>,
    MockPool<HeaderId, Tx, <Tx as Transaction>::Hash>,
    MempoolNetworkAdapter<Tx, <Tx as Transaction>::Hash>,
    MockPool<
        HeaderId,
        VidCertificate,
        <VidCertificate as certificate::vid::VidCertificate>::CertificateId,
    >,
    MempoolNetworkAdapter<Certificate, <Certificate as certificate::Certificate>::Id>,
    MempoolVerificationProvider,
    FillSizeWithTx<MB16, Tx>,
    FillSizeWithBlobsCertificate<MB16, VidCertificate>,
    RocksBackend<Wire>,
>;

pub type TxMempool = TxMempoolService<
    MempoolNetworkAdapter<Tx, <Tx as Transaction>::Hash>,
    MockPool<HeaderId, Tx, <Tx as Transaction>::Hash>,
>;

pub type DaMempool = DaMempoolService<
    MempoolNetworkAdapter<Certificate, <Certificate as certificate::Certificate>::Id>,
    MockPool<
        HeaderId,
        VidCertificate,
        <VidCertificate as certificate::vid::VidCertificate>::CertificateId,
    >,
    MempoolVerificationProvider,
>;

#[derive(Services)]
pub struct Nomos {
    #[cfg(feature = "tracing")]
    logging: ServiceHandle<Logger>,
    network: ServiceHandle<NetworkService<NetworkBackend>>,
    cl_mempool: ServiceHandle<TxMempool>,
    da_mempool: ServiceHandle<DaMempool>,
    cryptarchia: ServiceHandle<Cryptarchia>,
    http: ServiceHandle<NomosApiService>,
    storage: ServiceHandle<StorageService<RocksBackend<Wire>>>,
    #[cfg(feature = "metrics")]
    metrics: ServiceHandle<Metrics>,
    system_sig: ServiceHandle<SystemSig>,
}

pub struct Wire;

impl StorageSerde for Wire {
    type Error = wire::Error;

    fn serialize<T: Serialize>(value: T) -> Bytes {
        wire::serialize(&value).unwrap().into()
    }

    fn deserialize<T: DeserializeOwned>(buff: Bytes) -> Result<T, Self::Error> {
        wire::deserialize(&buff)
    }
}
