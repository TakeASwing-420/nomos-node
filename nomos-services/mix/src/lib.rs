pub mod backends;

use std::fmt::{self, Debug};

use async_trait::async_trait;
use backends::NetworkBackend;
use futures::StreamExt;
use overwatch_rs::services::{
    handle::ServiceStateHandle,
    life_cycle::LifecycleMessage,
    relay::RelayMessage,
    state::{NoOperator, ServiceState},
    ServiceCore, ServiceData, ServiceId,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot};

pub struct NetworkService<B: NetworkBackend + 'static> {
    backend: B,
    service_state: ServiceStateHandle<Self>,
}

impl<B: NetworkBackend + 'static> ServiceData for NetworkService<B> {
    const SERVICE_ID: ServiceId = "MixNetwork";
    type Settings = NetworkConfig<B>;
    type State = NetworkState<B>;
    type StateOperator = NoOperator<Self::State>;
    type Message = NetworkMsg<B>;
}

#[async_trait]
impl<B> ServiceCore for NetworkService<B>
where
    B: NetworkBackend + Send + 'static,
    B::State: Send + Sync,
{
    fn init(service_state: ServiceStateHandle<Self>) -> Result<Self, overwatch_rs::DynError> {
        Ok(Self {
            backend: <B as NetworkBackend>::new(
                service_state.settings_reader.get_updated_settings().backend,
                service_state.overwatch_handle.clone(),
            ),
            service_state,
        })
    }

    async fn run(mut self) -> Result<(), overwatch_rs::DynError> {
        let Self {
            service_state:
                ServiceStateHandle {
                    mut inbound_relay,
                    lifecycle_handle,
                    ..
                },
            mut backend,
        } = self;
        let mut lifecycle_stream = lifecycle_handle.message_stream();
        loop {
            tokio::select! {
                Some(msg) = inbound_relay.recv() => {
                    Self::handle_network_service_message(msg, &mut backend).await;
                }
                Some(msg) = lifecycle_stream.next() => {
                    if Self::should_stop_service(msg).await {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl<B> NetworkService<B>
where
    B: NetworkBackend + Send + 'static,
    B::State: Send + Sync,
{
    async fn handle_network_service_message(msg: NetworkMsg<B>, backend: &mut B) {
        match msg {
            NetworkMsg::Process(msg) => {
                // split sending in two steps to help the compiler understand we do not
                // need to hold an instance of &I (which is not send) across an await point
                let _send = backend.process(msg);
                _send.await
            }
            NetworkMsg::Subscribe { kind, sender } => sender
                .send(backend.subscribe(kind).await)
                .unwrap_or_else(|_| {
                    tracing::warn!(
                        "client hung up before a subscription handle could be established"
                    )
                }),
        }
    }

    async fn should_stop_service(msg: LifecycleMessage) -> bool {
        match msg {
            LifecycleMessage::Kill => true,
            LifecycleMessage::Shutdown(signal_sender) => {
                // TODO: Maybe add a call to backend to handle this. Maybe trying to save unprocessed messages?
                if signal_sender.send(()).is_err() {
                    tracing::error!(
                        "Error sending successful shutdown signal from service {}",
                        Self::SERVICE_ID
                    );
                }
                true
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NetworkConfig<B: NetworkBackend> {
    pub backend: B::Settings,
}

impl<B: NetworkBackend> Debug for NetworkConfig<B> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "NetworkConfig {{ backend: {:?}}}", self.backend)
    }
}

impl<B: NetworkBackend> Clone for NetworkConfig<B> {
    fn clone(&self) -> Self {
        NetworkConfig {
            backend: self.backend.clone(),
        }
    }
}

pub struct NetworkState<B: NetworkBackend> {
    _backend: B::State,
}

impl<B: NetworkBackend> Clone for NetworkState<B> {
    fn clone(&self) -> Self {
        NetworkState {
            _backend: self._backend.clone(),
        }
    }
}

impl<B: NetworkBackend> ServiceState for NetworkState<B> {
    type Settings = NetworkConfig<B>;
    type Error = <B::State as ServiceState>::Error;

    fn from_settings(settings: &Self::Settings) -> Result<Self, Self::Error> {
        B::State::from_settings(&settings.backend).map(|_backend| Self { _backend })
    }
}

pub enum NetworkMsg<B: NetworkBackend> {
    Process(B::Message),
    Subscribe {
        kind: B::EventKind,
        sender: oneshot::Sender<broadcast::Receiver<B::NetworkEvent>>,
    },
}

impl<B: NetworkBackend> Debug for NetworkMsg<B> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Process(msg) => write!(fmt, "NetworkMsg::Process({msg:?})"),
            Self::Subscribe { kind, sender } => write!(
                fmt,
                "NetworkMsg::Subscribe{{ kind: {kind:?}, sender: {sender:?}}}"
            ),
        }
    }
}

impl<T: NetworkBackend + 'static> RelayMessage for NetworkMsg<T> {}
