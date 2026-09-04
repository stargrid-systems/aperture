//! The poll worker: request scheduling, staleness transitions, and events.

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aperture_events::{EventBus, EventDefinition};
use aperture_runtime::{Stop, Worker};
use aperture_storage::ActorId;
use arc_swap::ArcSwap;
use cellguard_protocol::{
    BalancerStatus, DeviceId, Kind, RailSnapshot, SerialNumber, Snapshot, TempSnapshot,
};
use jiff::Timestamp;
use tokio::time::sleep;

use crate::config::CellguardConfig;
use crate::event::{DeviceConnected, DeviceDisconnected, SnapshotStale};
use crate::link::LinkFactory;
use crate::snapshot::{BoardIdentity, Cached, DeviceSnapshot, NodeState};
use crate::transport::{ExchangeError, Framed, Reply};

/// Bus node id of the cellcore, the node this driver polls.
const CELLCORE_ID: u8 = 1;

/// A polled kind: one request/response pair per round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    CellVoltages,
    BalanceCurrents,
    Rails,
    Temperatures,
    BalancerStatus,
}

/// The poll cycle, served in this order.
const POLL_SLOTS: [Slot; 5] = [
    Slot::CellVoltages,
    Slot::BalanceCurrents,
    Slot::Rails,
    Slot::Temperatures,
    Slot::BalancerStatus,
];

impl Slot {
    /// The request kind the driver sends.
    const fn request(self) -> Kind {
        match self {
            Self::CellVoltages => Kind::ReadCellVoltages,
            Self::BalanceCurrents => Kind::ReadBalanceCurrents,
            Self::Rails => Kind::ReadRails,
            Self::Temperatures => Kind::ReadTemperatures,
            Self::BalancerStatus => Kind::ReadBalancerStatus,
        }
    }

    /// The reply kind this slot waits for.
    const fn reply(self) -> Kind {
        match self {
            Self::CellVoltages => Kind::CellVoltages,
            Self::BalanceCurrents => Kind::BalanceCurrents,
            Self::Rails => Kind::Rails,
            Self::Temperatures => Kind::Temperatures,
            Self::BalancerStatus => Kind::BalancerStatus,
        }
    }

    /// The kind's name in events and logs.
    const fn name(self) -> &'static str {
        match self {
            Self::CellVoltages => "cell_voltages",
            Self::BalanceCurrents => "balance_currents",
            Self::Rails => "rails",
            Self::Temperatures => "temperatures",
            Self::BalancerStatus => "balancer_status",
        }
    }

    /// Maps a reply kind back to its slot, so a reply that arrives after its
    /// request timed out still lands where it belongs.
    const fn from_reply(kind: Kind) -> Option<Self> {
        match kind {
            Kind::CellVoltages => Some(Self::CellVoltages),
            Kind::BalanceCurrents => Some(Self::BalanceCurrents),
            Kind::Rails => Some(Self::Rails),
            Kind::Temperatures => Some(Self::Temperatures),
            Kind::BalancerStatus => Some(Self::BalancerStatus),
            _ => None,
        }
    }

    /// Decodes a reply payload for this slot.
    fn decode(self, payload: &[u8]) -> Option<SlotData> {
        match self {
            Self::CellVoltages => Snapshot::decode(payload).map(SlotData::CellVoltages),
            Self::BalanceCurrents => Snapshot::decode(payload).map(SlotData::BalanceCurrents),
            Self::Rails => RailSnapshot::decode(payload).map(SlotData::Rails),
            Self::Temperatures => TempSnapshot::decode(payload).map(SlotData::Temperatures),
            Self::BalancerStatus => BalancerStatus::decode(payload).map(SlotData::BalancerStatus),
        }
    }
}

/// A decoded reply payload, tagged with its slot.
#[derive(Debug, Clone, Copy)]
enum SlotData {
    CellVoltages(Snapshot),
    BalanceCurrents(Snapshot),
    Rails(RailSnapshot),
    Temperatures(TempSnapshot),
    BalancerStatus(BalancerStatus),
}

/// Cache and failure bookkeeping for one polled kind.
struct SlotState<T> {
    data: Option<(T, Timestamp)>,
    failures: u32,
    stale_emitted: bool,
}

impl<T> SlotState<T> {
    const fn empty() -> Self {
        Self {
            data: None,
            failures: 0,
            stale_emitted: false,
        }
    }

    /// Records a successful read: fresh data and a cleared failure count.
    fn note_success(&mut self, data: T, now: Timestamp) {
        self.data = Some((data, now));
        self.failures = 0;
        self.stale_emitted = false;
    }

    /// Consumes a staleness transition: `true` exactly once when the kind
    /// crossed the threshold while the device is connected.
    const fn take_stale(&mut self, stale_after: u32, connected: bool) -> bool {
        if connected && self.failures >= stale_after && !self.stale_emitted {
            self.stale_emitted = true;
            return true;
        }
        false
    }
}

impl<T: Copy> SlotState<T> {
    /// The published cache entry, or `None` when the kind never answered.
    fn cached(&self, stale_after: u32) -> Option<Cached<T>> {
        self.data.as_ref().map(|(data, updated_at)| Cached {
            data: *data,
            updated_at: *updated_at,
            stale: self.failures >= stale_after,
        })
    }
}

/// The worker's private view of the device, feeding the published snapshot.
struct WorkingState {
    identity: Option<BoardIdentity>,
    connected: bool,
    /// Identity queries run once per link, on first contact.
    identity_pending: bool,
    /// Consecutive poll intervals without any valid reply.
    dead_rounds: u32,
    cell_voltages: SlotState<Snapshot>,
    balance_currents: SlotState<Snapshot>,
    rails: SlotState<RailSnapshot>,
    temperatures: SlotState<TempSnapshot>,
    balancer_status: SlotState<BalancerStatus>,
}

impl WorkingState {
    const fn empty() -> Self {
        Self {
            identity: None,
            connected: false,
            identity_pending: false,
            dead_rounds: 0,
            cell_voltages: SlotState::empty(),
            balance_currents: SlotState::empty(),
            rails: SlotState::empty(),
            temperatures: SlotState::empty(),
            balancer_status: SlotState::empty(),
        }
    }

    /// Records one failed attempt for the slot.
    const fn note_failure(&mut self, slot: Slot) {
        match slot {
            Slot::CellVoltages => self.cell_voltages.failures += 1,
            Slot::BalanceCurrents => self.balance_currents.failures += 1,
            Slot::Rails => self.rails.failures += 1,
            Slot::Temperatures => self.temperatures.failures += 1,
            Slot::BalancerStatus => self.balancer_status.failures += 1,
        }
    }

    /// Stores a decoded reply where it belongs.
    fn store(&mut self, slot: Slot, data: SlotData, now: Timestamp) {
        match (slot, data) {
            (Slot::CellVoltages, SlotData::CellVoltages(data)) => {
                self.cell_voltages.note_success(data, now);
            }
            (Slot::BalanceCurrents, SlotData::BalanceCurrents(data)) => {
                self.balance_currents.note_success(data, now);
            }
            (Slot::Rails, SlotData::Rails(data)) => self.rails.note_success(data, now),
            (Slot::Temperatures, SlotData::Temperatures(data)) => {
                self.temperatures.note_success(data, now);
            }
            (Slot::BalancerStatus, SlotData::BalancerStatus(data)) => {
                self.balancer_status.note_success(data, now);
            }
            // `Slot::decode` keys the payload to the slot, so a mismatched
            // pair cannot be constructed.
            _ => {}
        }
    }

    /// Consumes a pending staleness transition for the slot.
    const fn take_stale_event(&mut self, slot: Slot, stale_after: u32, connected: bool) -> bool {
        match slot {
            Slot::CellVoltages => self.cell_voltages.take_stale(stale_after, connected),
            Slot::BalanceCurrents => self.balance_currents.take_stale(stale_after, connected),
            Slot::Rails => self.rails.take_stale(stale_after, connected),
            Slot::Temperatures => self.temperatures.take_stale(stale_after, connected),
            Slot::BalancerStatus => self.balancer_status.take_stale(stale_after, connected),
        }
    }
}

/// Outcome of one slot's exchange within a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotOutcome {
    /// A decoded reply was stored.
    Data,
    /// The device rejected the request. It answered, so it is alive.
    Rejected,
    /// The reply never arrived, arrived corrupt, or had a bad shape.
    Failed,
    /// The link broke. The worker drops it and reopens.
    LinkDead,
    /// Shutdown was requested.
    Stopped,
}

/// What happened over the course of one poll round.
#[derive(Debug, Default)]
struct RoundState {
    /// The device produced at least one valid reply packet.
    answered: bool,
    /// The link broke mid-round and the round was cut short.
    link_dead: bool,
}

/// Result of one request/response exchange.
enum ExpectResult<T> {
    Reply(T),
    Rejected,
    Failed,
    LinkDead,
    Stopped,
}

/// Result of the identity queries at first contact.
enum IdentityOutcome {
    Identified(BoardIdentity),
    Unavailable,
    Aborted,
}

/// Shared state of the [`Cellguard`](crate::Cellguard) handle and its
/// worker.
pub struct Inner {
    pub(crate) config: CellguardConfig,
    pub(crate) event_bus: EventBus,
    pub(crate) snapshots: ArcSwap<DeviceSnapshot>,
}

/// The background poll worker. Spawn it on a
/// [`Supervisor`](aperture_runtime::Supervisor) as `"cellguard"`.
pub struct CellguardWorker<F> {
    inner: Arc<Inner>,
    factory: F,
    state: WorkingState,
}

impl<F> CellguardWorker<F> {
    pub(crate) const fn new(inner: Arc<Inner>, factory: F) -> Self {
        Self {
            inner,
            factory,
            state: WorkingState::empty(),
        }
    }
}

impl<F: LinkFactory> Worker for CellguardWorker<F> {
    /// Opens the port with backoff until the device appears, then polls
    /// round after round. Exits promptly when `stop` resolves, mid-round
    /// included.
    async fn run(mut self, stop: Stop) {
        let mut framed: Option<Framed<F::Link>> = None;
        let mut open_backoff = self.inner.config.open_retry_delay;
        loop {
            if framed.is_none() {
                let opened = tokio::select! {
                    biased;
                    () = stop.cancelled() => return,
                    opened = self.factory.open() => opened,
                };
                match opened {
                    Ok(link) => {
                        open_backoff = self.inner.config.open_retry_delay;
                        self.state.identity_pending = true;
                        framed = Some(Framed::new(link));
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = &err as &dyn StdError,
                            "failed to open the cellguard serial port, retrying"
                        );
                        self.transitions(false).await;
                        self.publish();
                        if !self.sleep_or_stop(open_backoff, &stop).await {
                            return;
                        }
                        open_backoff =
                            (open_backoff * 2).min(self.inner.config.open_retry_max_delay);
                        continue;
                    }
                }
            }

            let mut link = framed.take().expect("the loop just established the link");
            let Some(mut round) = self.poll_round(&mut link, &stop).await else {
                return;
            };
            self.finish_round(&mut round, &mut link, &stop).await;
            if round.link_dead {
                drop(link);
                self.state.identity_pending = true;
            } else {
                framed = Some(link);
            }
            if !self
                .sleep_or_stop(self.inner.config.poll_interval, &stop)
                .await
            {
                return;
            }
        }
    }
}

impl<F: LinkFactory> CellguardWorker<F> {
    /// Polls every slot once. Returns `None` when shutdown was requested.
    async fn poll_round(&mut self, link: &mut Framed<F::Link>, stop: &Stop) -> Option<RoundState> {
        let mut round = RoundState::default();
        for slot in POLL_SLOTS {
            if round.link_dead {
                self.state.note_failure(slot);
                continue;
            }
            match self.poll_slot(link, stop, &mut round, slot).await {
                SlotOutcome::Stopped => return None,
                SlotOutcome::Data | SlotOutcome::Rejected => round.answered = true,
                SlotOutcome::Failed | SlotOutcome::LinkDead => {}
            }
        }
        Some(round)
    }

    /// Runs one request/response pair for the slot.
    async fn poll_slot(
        &mut self,
        link: &mut Framed<F::Link>,
        stop: &Stop,
        round: &mut RoundState,
        slot: Slot,
    ) -> SlotOutcome {
        let result = self
            .exchange(link, stop, round, slot.request(), slot.reply(), |payload| {
                slot.decode(payload)
            })
            .await;
        match result {
            ExpectResult::Stopped => SlotOutcome::Stopped,
            ExpectResult::LinkDead => {
                self.state.note_failure(slot);
                SlotOutcome::LinkDead
            }
            ExpectResult::Failed => {
                self.state.note_failure(slot);
                SlotOutcome::Failed
            }
            ExpectResult::Rejected => SlotOutcome::Rejected,
            ExpectResult::Reply(data) => {
                self.state.store(slot, data, Timestamp::now());
                SlotOutcome::Data
            }
        }
    }

    /// Sends one request and waits for the expected reply kind.
    ///
    /// Replies for other slots are not lost when they arrive late: they are
    /// stored where they belong, and the wait for this slot's reply
    /// continues within the same timeout window.
    async fn exchange<T>(
        &mut self,
        link: &mut Framed<F::Link>,
        stop: &Stop,
        round: &mut RoundState,
        request: Kind,
        expected: Kind,
        decode: impl Fn(&[u8]) -> Option<T>,
    ) -> ExpectResult<T> {
        let sent = tokio::select! {
            biased;
            () = stop.cancelled() => return ExpectResult::Stopped,
            sent = link.send(CELLCORE_ID, request, &[]) => sent,
        };
        if let Err(err) = sent {
            tracing::warn!(
                error = &err as &dyn StdError,
                kind = ?request,
                "failed to send the poll request"
            );
            round.link_dead = true;
            return ExpectResult::LinkDead;
        }

        let deadline = Instant::now() + self.inner.config.reply_timeout;
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return ExpectResult::Failed;
            };
            let reply = tokio::select! {
                biased;
                () = stop.cancelled() => return ExpectResult::Stopped,
                reply = link.recv(remaining) => reply,
            };
            match reply {
                Err(ExchangeError::Io(err)) => {
                    tracing::warn!(error = &err as &dyn StdError, "bus link io failed");
                    round.link_dead = true;
                    return ExpectResult::LinkDead;
                }
                Err(ExchangeError::Timeout) => return ExpectResult::Failed,
                Err(err) => {
                    tracing::warn!(
                        error = &err as &dyn StdError,
                        "corrupt reply frame on the bus"
                    );
                    return ExpectResult::Failed;
                }
                Ok(reply) => {
                    round.answered = true;
                    if reply.kind == Kind::Nack {
                        // A node-local rejection carries an empty payload. A
                        // routed request that died on the way reports a
                        // one-byte reason code instead.
                        let reason = reply.payload.first().copied();
                        tracing::debug!(reason = ?reason, "device rejected the request");
                        return ExpectResult::Rejected;
                    }
                    if reply.kind == expected {
                        return if let Some(value) = decode(&reply.payload) {
                            ExpectResult::Reply(value)
                        } else {
                            tracing::warn!(
                                kind = ?expected,
                                "reply payload has an unexpected shape"
                            );
                            ExpectResult::Failed
                        };
                    }
                    match Slot::from_reply(reply.kind) {
                        Some(slot) => self.absorb_stray(slot, &reply),
                        None => {
                            tracing::debug!(kind = ?reply.kind, "ignoring unexpected reply kind");
                        }
                    }
                }
            }
        }
    }

    /// Stores a late reply under its own slot.
    fn absorb_stray(&mut self, slot: Slot, reply: &Reply) {
        let now = Timestamp::now();
        match slot.decode(&reply.payload) {
            Some(data) => self.state.store(slot, data, now),
            None => self.state.note_failure(slot),
        }
    }

    /// Applies the round's connect, disconnect, and staleness transitions,
    /// then publishes the snapshot. Shutdown during the identity queries
    /// skips the connect event. The loop exits on the next stop check.
    async fn finish_round(
        &mut self,
        round: &mut RoundState,
        link: &mut Framed<F::Link>,
        stop: &Stop,
    ) {
        if round.answered && !self.state.connected {
            if self.state.identity_pending {
                self.state.identity_pending = false;
                match self.query_identity(link, stop, round).await {
                    IdentityOutcome::Identified(identity) => {
                        self.state.identity = Some(identity);
                    }
                    IdentityOutcome::Unavailable => {}
                    IdentityOutcome::Aborted => {
                        self.publish();
                        return;
                    }
                }
            }
            self.state.connected = true;
            let identity = self.state.identity.clone();
            self.emit(DeviceConnected { identity }).await;
        }
        self.transitions(round.answered).await;
        self.publish();
    }

    /// Queries the cellcore identity, once per link at first contact.
    async fn query_identity(
        &mut self,
        link: &mut Framed<F::Link>,
        stop: &Stop,
        round: &mut RoundState,
    ) -> IdentityOutcome {
        let id = match self
            .exchange(
                link,
                stop,
                round,
                Kind::ReadDeviceId,
                Kind::DeviceId,
                DeviceId::decode,
            )
            .await
        {
            ExpectResult::Reply(id) => id,
            ExpectResult::Rejected | ExpectResult::Failed => return IdentityOutcome::Unavailable,
            ExpectResult::LinkDead | ExpectResult::Stopped => return IdentityOutcome::Aborted,
        };
        let serial = match self
            .exchange(
                link,
                stop,
                round,
                Kind::ReadSerialNumber,
                Kind::SerialNumber,
                SerialNumber::decode,
            )
            .await
        {
            ExpectResult::Reply(serial) => serial,
            ExpectResult::Rejected | ExpectResult::Failed => return IdentityOutcome::Unavailable,
            ExpectResult::LinkDead | ExpectResult::Stopped => return IdentityOutcome::Aborted,
        };
        IdentityOutcome::Identified(BoardIdentity::from_protocol(id, serial))
    }

    /// Connect, disconnect, and staleness transitions for one poll interval.
    async fn transitions(&mut self, answered: bool) {
        let stale_after = self.inner.config.stale_after;
        if answered {
            self.state.dead_rounds = 0;
        } else {
            self.state.dead_rounds += 1;
        }
        if !answered && self.state.connected && self.state.dead_rounds >= stale_after {
            self.state.connected = false;
            let identity = self.state.identity.clone();
            self.emit(DeviceDisconnected { identity }).await;
        }
        let connected = self.state.connected;
        for slot in POLL_SLOTS {
            if self.state.take_stale_event(slot, stale_after, connected) {
                let identity = self.state.identity.clone();
                self.emit(SnapshotStale {
                    kind: slot.name().to_owned(),
                    identity,
                })
                .await;
            }
        }
    }

    /// Publishes the current state to the handle.
    fn publish(&self) {
        let stale_after = self.inner.config.stale_after;
        let snapshot = DeviceSnapshot {
            updated_at: Timestamp::now(),
            connected: self.state.connected,
            cellcore: NodeState {
                identity: self.state.identity.clone(),
                cell_voltages: self.state.cell_voltages.cached(stale_after),
                balance_currents: self.state.balance_currents.cached(stale_after),
                rails: self.state.rails.cached(stale_after),
                temperatures: self.state.temperatures.cached(stale_after),
                balancer_status: self.state.balancer_status.cached(stale_after),
            },
        };
        self.inner.snapshots.store(Arc::new(snapshot));
    }

    /// Emits one domain event as the system actor.
    async fn emit<D: EventDefinition>(&self, payload: D) {
        if let Err(err) = self.inner.event_bus.emit(payload, ActorId::SYSTEM).await {
            tracing::warn!(
                error = &err as &dyn StdError,
                "failed to emit cellguard event"
            );
        }
    }

    /// Sleeps out `delay`, returning `false` when `stop` resolved first.
    async fn sleep_or_stop(&self, delay: Duration, stop: &Stop) -> bool {
        tokio::select! {
            biased;
            () = stop.cancelled() => false,
            () = sleep(delay) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::io;

    use aperture_events::{Delivery, EventBus, TypedEventStream};
    use aperture_runtime::{ShutdownOutcome, Supervisor};
    use cellguard_protocol::{
        Decoder, Packet, RAILS, SERIAL_LEN, TEMP_INVALID, TEMP_ORDER, encode_frame, max_encoded_len,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};
    use tokio::sync::Mutex;
    use tokio::time::timeout;

    use super::*;
    use crate::config::CellguardConfig;

    const BUFSZ: usize = 1024;

    fn test_config() -> CellguardConfig {
        let mut config = CellguardConfig::new("/dev/null".into(), 115_200);
        config.poll_interval = Duration::from_millis(10);
        config.reply_timeout = Duration::from_millis(50);
        config.stale_after = 2;
        config.open_retry_delay = Duration::from_millis(5);
        config.open_retry_max_delay = Duration::from_millis(20);
        config
    }

    fn test_inner(config: CellguardConfig, event_bus: &EventBus) -> Arc<Inner> {
        Arc::new(Inner {
            snapshots: ArcSwap::from_pointee(DeviceSnapshot::empty(Timestamp::now())),
            config,
            event_bus: event_bus.clone(),
        })
    }

    /// What the fake device answers to a request kind.
    #[derive(Debug, Clone)]
    enum Action {
        /// A normal reply, after an optional delay.
        Reply {
            /// The reply kind on the wire.
            kind: Kind,
            payload: Vec<u8>,
            delay: Duration,
        },
        /// A `Nack` with the given payload (empty or one reason byte).
        Nack(Vec<u8>),
        /// Raw wire bytes, ignoring the protocol.
        Garbage(Vec<u8>),
        /// Keep the link but never answer.
        Silence,
        /// Close the link.
        Die,
    }

    impl Action {
        fn reply(kind: Kind, payload: Vec<u8>) -> Self {
            Self::Reply {
                kind,
                payload,
                delay: Duration::ZERO,
            }
        }

        /// A reply that arrives after its own request timed out, so it
        /// lands during a later exchange.
        fn late_reply(kind: Kind, payload: Vec<u8>, delay: Duration) -> Self {
            Self::Reply {
                kind,
                payload,
                delay,
            }
        }
    }

    /// The fake device's state: what it received and how it answers.
    ///
    /// Scripted replies are per-request-kind queues. `Kind` is not `Hash`
    /// or `Ord`, so the table is a small `Vec`.
    #[derive(Default)]
    struct DeviceState {
        received: Vec<(u8, Kind)>,
        replies: Vec<(Kind, VecDeque<Action>)>,
    }

    impl DeviceState {
        fn enqueue(&mut self, request: Kind, action: Action) {
            if let Some((_, queue)) = self.replies.iter_mut().find(|(kind, _)| *kind == request) {
                queue.push_back(action);
            } else {
                self.replies.push((request, VecDeque::from([action])));
            }
        }

        /// Answers `request` with `action` from the first exchange on.
        fn always(&mut self, request: Kind, action: &Action) {
            for _ in 0..64 {
                self.enqueue(request, action.clone());
            }
        }

        fn next_action(&mut self, request: Kind) -> Option<Action> {
            self.replies
                .iter_mut()
                .find(|(kind, _)| *kind == request)
                .and_then(|(_, queue)| queue.pop_front())
        }

        fn count(&self, kind: Kind) -> usize {
            self.received.iter().filter(|(_, k)| *k == kind).count()
        }
    }

    type Device = Arc<Mutex<DeviceState>>;

    /// Runs the fake device until the link closes.
    async fn run_device(mut link: DuplexStream, device: Device) {
        let mut decoder = Decoder::new();
        let mut rx = [0; 256];
        loop {
            let mut byte = [0; 1];
            if link.read(&mut byte).await.unwrap_or(0) == 0 {
                return;
            }
            // Incomplete frames and decoder errors resynchronize at the
            // next delimiter, so both just keep the loop going.
            if let Ok(Some(len)) = decoder.feed(byte[0], &mut rx)
                && let Ok(packet) = Packet::parse(&rx[..len])
            {
                let action = {
                    let mut device = device.lock().await;
                    device.received.push((packet.id, packet.kind));
                    device.next_action(packet.kind)
                };
                match action {
                    Some(Action::Reply {
                        kind,
                        payload,
                        delay,
                    }) => {
                        sleep(delay).await;
                        send_reply(&mut link, packet.id, kind, &payload).await;
                    }
                    Some(Action::Nack(payload)) => {
                        send_reply(&mut link, packet.id, Kind::Nack, &payload).await;
                    }
                    Some(Action::Garbage(bytes)) => {
                        let _ = link.write_all(&bytes).await;
                    }
                    // Unscripted requests go unanswered, like a node that
                    // does not serve the kind.
                    Some(Action::Silence) | None => {}
                    Some(Action::Die) => return,
                }
            }
        }
    }

    async fn send_reply(link: &mut DuplexStream, id: u8, kind: Kind, payload: &[u8]) {
        let mut raw = [0; 256];
        let Ok(raw_len) = Packet::write(id, kind, payload, &mut raw) else {
            return;
        };
        let mut wire = [0; max_encoded_len(256)];
        let Some(wire_len) = encode_frame(&raw[..raw_len], &mut wire) else {
            return;
        };
        let _ = link.write_all(&wire[..wire_len]).await;
    }

    /// Yields scripted outcomes, one per open attempt: a fresh fake device
    /// or an open error.
    struct FakeFactory {
        next: Mutex<VecDeque<Result<Device, io::Error>>>,
    }

    impl FakeFactory {
        fn new(script: VecDeque<Result<Device, io::Error>>) -> Self {
            Self {
                next: Mutex::new(script),
            }
        }
    }

    impl LinkFactory for FakeFactory {
        type Link = DuplexStream;

        async fn open(&self) -> io::Result<DuplexStream> {
            let next = self.next.lock().await.pop_front();
            match next {
                Some(Ok(device)) => {
                    let (driver_side, device_side) = duplex(BUFSZ);
                    tokio::spawn(run_device(device_side, device));
                    Ok(driver_side)
                }
                Some(Err(err)) => Err(err),
                None => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }
    }

    fn absent() -> Result<Device, io::Error> {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }

    fn device_id_payload() -> Vec<u8> {
        let id = DeviceId {
            board_model: 0x1234,
            board_revision: 0x56,
            fw_version: 7,
        };
        let mut buf = [0; DeviceId::PAYLOAD_LEN];
        id.encode(&mut buf).expect("fits").to_vec()
    }

    fn serial_payload() -> Vec<u8> {
        let serial = SerialNumber {
            serial: [0xAB; SERIAL_LEN],
        };
        let mut buf = [0; SerialNumber::PAYLOAD_LEN];
        serial.encode(&mut buf).expect("fits").to_vec()
    }

    fn snapshot_payload(seq: u8, codes: [i32; 4]) -> Vec<u8> {
        let snapshot = Snapshot { seq, codes };
        let mut buf = [0; Snapshot::PAYLOAD_LEN];
        snapshot.encode(&mut buf).expect("fits").to_vec()
    }

    fn rails_payload(codes: [u16; RAILS]) -> Vec<u8> {
        let rails = RailSnapshot { codes };
        let mut buf = [0; RailSnapshot::PAYLOAD_LEN];
        rails.encode(&mut buf).expect("fits").to_vec()
    }

    fn temps_payload() -> Vec<u8> {
        assert_eq!(TEMP_ORDER.len(), 3);
        let temps = TempSnapshot {
            temps: [2150, 2410, TEMP_INVALID],
        };
        let mut buf = [0; TempSnapshot::PAYLOAD_LEN];
        temps.encode(&mut buf).expect("fits").to_vec()
    }

    fn status_payload() -> Vec<u8> {
        let status = BalancerStatus {
            en_3r6: 0x0F,
            en_36r5: 0,
            pwm_duty: 4096,
            gate_mask: 0x03,
            tiny_all_off: false,
            emergency_gate_off: false,
            active_balancer_on: true,
            en_all: true,
            cellagent_alive: true,
        };
        let mut buf = [0; BalancerStatus::PAYLOAD_LEN];
        status.encode(&mut buf).expect("fits").to_vec()
    }

    /// Enqueues the identity replies and one telemetry reply per kind.
    fn answering(device: &mut DeviceState) {
        device.enqueue(
            Kind::ReadDeviceId,
            Action::reply(Kind::DeviceId, device_id_payload()),
        );
        device.enqueue(
            Kind::ReadSerialNumber,
            Action::reply(Kind::SerialNumber, serial_payload()),
        );
        device.enqueue(
            Kind::ReadCellVoltages,
            Action::reply(
                Kind::CellVoltages,
                snapshot_payload(1, [100, 200, 300, 400]),
            ),
        );
        device.enqueue(
            Kind::ReadBalanceCurrents,
            Action::reply(Kind::BalanceCurrents, snapshot_payload(2, [10, 20, 30, 40])),
        );
        device.enqueue(
            Kind::ReadRails,
            Action::reply(Kind::Rails, rails_payload([1, 2, 3, 4, 5, 6, 7, 8])),
        );
        device.enqueue(
            Kind::ReadTemperatures,
            Action::reply(Kind::Temperatures, temps_payload()),
        );
        device.enqueue(
            Kind::ReadBalancerStatus,
            Action::reply(Kind::BalancerStatus, status_payload()),
        );
    }

    /// A device that answers the identity queries and every telemetry kind
    /// for the whole run.
    fn answering_device() -> Device {
        let mut device = DeviceState::default();
        device.always(
            Kind::ReadDeviceId,
            &Action::reply(Kind::DeviceId, device_id_payload()),
        );
        device.always(
            Kind::ReadSerialNumber,
            &Action::reply(Kind::SerialNumber, serial_payload()),
        );
        device.always(
            Kind::ReadCellVoltages,
            &Action::reply(
                Kind::CellVoltages,
                snapshot_payload(1, [100, 200, 300, 400]),
            ),
        );
        device.always(
            Kind::ReadBalanceCurrents,
            &Action::reply(Kind::BalanceCurrents, snapshot_payload(2, [10, 20, 30, 40])),
        );
        device.always(
            Kind::ReadRails,
            &Action::reply(Kind::Rails, rails_payload([1, 2, 3, 4, 5, 6, 7, 8])),
        );
        device.always(
            Kind::ReadTemperatures,
            &Action::reply(Kind::Temperatures, temps_payload()),
        );
        device.always(
            Kind::ReadBalancerStatus,
            &Action::reply(Kind::BalancerStatus, status_payload()),
        );
        Arc::new(Mutex::new(device))
    }

    fn collect_events<D: EventDefinition>(bus: &EventBus) -> Arc<Mutex<Vec<D>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let mut stream: TypedEventStream<D> = bus.subscribe_typed();
        tokio::spawn(async move {
            while let Some(Delivery::Event(event)) = stream.recv().await {
                sink.lock().await.push(event.payload);
            }
        });
        events
    }

    async fn wait_for<F, Fut>(condition: F) -> bool
    where
        F: Fn() -> Fut,
        Fut: Future<Output = bool>,
    {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if condition().await {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn polls_identity_and_telemetry_into_the_snapshot() {
        let event_bus = EventBus::new();
        let device = answering_device();
        let factory = FakeFactory::new(VecDeque::from([Ok(device.clone())]));
        let connected_events = collect_events::<DeviceConnected>(&event_bus);
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let snapshot = inner.snapshots.load_full();
                snapshot.connected && snapshot.cellcore.cell_voltages.is_some()
            })
            .await,
            "the worker must reach a connected, polling state"
        );

        assert_eq!(connected_events.lock().await.len(), 1);
        // The signal doubles as the stop trigger, exactly like the real
        // shutdown path.
        let outcome = supervisor
            .run_until_signal(sleep(Duration::from_millis(20)))
            .await;
        assert!(matches!(outcome, ShutdownOutcome::Signaled), "{outcome:?}");

        let received = device.lock().await;
        assert_eq!(received.count(Kind::ReadDeviceId), 1);
        assert_eq!(received.count(Kind::ReadSerialNumber), 1);
        assert!(
            received.received.iter().all(|(id, _)| *id == CELLCORE_ID),
            "every request must address the cellcore"
        );

        let snapshot = inner.snapshots.load_full();
        let identity = snapshot.cellcore.identity.as_ref().expect("identity");
        assert_eq!(identity.board_model, 0x1234);
        assert_eq!(identity.board_revision, 0x56);
        assert_eq!(identity.fw_version, 7);
        assert_eq!(identity.serial, hex::encode([0xAB; SERIAL_LEN]));

        let voltages = snapshot.cellcore.cell_voltages.as_ref().expect("voltages");
        assert_eq!(voltages.data.seq, 1);
        assert_eq!(voltages.data.codes, [100, 200, 300, 400]);
        assert!(!voltages.stale);

        let currents = snapshot
            .cellcore
            .balance_currents
            .as_ref()
            .expect("currents");
        assert_eq!(currents.data.seq, 2);

        let rails = snapshot.cellcore.rails.as_ref().expect("rails");
        assert_eq!(rails.data.codes, [1, 2, 3, 4, 5, 6, 7, 8]);

        let temps = snapshot.cellcore.temperatures.as_ref().expect("temps");
        assert_eq!(temps.data.temps[2], TEMP_INVALID);

        let status = snapshot
            .cellcore
            .balancer_status
            .as_ref()
            .expect("balancer status");
        assert!(status.data.active_balancer_on);
        assert_eq!(status.data.gate_mask, 0x03);
    }

    #[tokio::test]
    async fn seq_wrap_is_an_update_not_a_repeat() {
        let event_bus = EventBus::new();
        let mut device = DeviceState::default();
        answering(&mut device);
        // Round one sees seq 255, round two sees seq 0.
        let voltages = device
            .replies
            .iter_mut()
            .find(|(kind, _)| *kind == Kind::ReadCellVoltages)
            .map(|entry| &mut entry.1)
            .expect("scripted");
        *voltages = VecDeque::from([
            Action::reply(Kind::CellVoltages, snapshot_payload(255, [1, 1, 1, 1])),
            Action::reply(Kind::CellVoltages, snapshot_payload(0, [2, 2, 2, 2])),
        ]);
        let device = Arc::new(Mutex::new(device));
        let factory = FakeFactory::new(VecDeque::from([Ok(device)]));
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let snapshot = inner.snapshots.load_full();
                snapshot
                    .cellcore
                    .cell_voltages
                    .as_ref()
                    .is_some_and(|cached| cached.data.seq == 0 && cached.data.codes == [2, 2, 2, 2])
            })
            .await,
            "seq 255 -> 0 must be stored verbatim"
        );

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn silent_kind_goes_stale_and_emits_once() {
        let event_bus = EventBus::new();
        let mut device = DeviceState::default();
        answering(&mut device);
        // Round one answers rails, then silence: the cached data stays but
        // turns stale after `stale_after` failed intervals.
        device.always(Kind::ReadRails, &Action::Silence);
        device.always(
            Kind::ReadDeviceId,
            &Action::reply(Kind::DeviceId, device_id_payload()),
        );
        device.always(
            Kind::ReadSerialNumber,
            &Action::reply(Kind::SerialNumber, serial_payload()),
        );
        device.always(
            Kind::ReadCellVoltages,
            &Action::reply(
                Kind::CellVoltages,
                snapshot_payload(1, [100, 200, 300, 400]),
            ),
        );
        device.always(
            Kind::ReadBalanceCurrents,
            &Action::reply(Kind::BalanceCurrents, snapshot_payload(2, [10, 20, 30, 40])),
        );
        device.always(
            Kind::ReadTemperatures,
            &Action::reply(Kind::Temperatures, temps_payload()),
        );
        device.always(
            Kind::ReadBalancerStatus,
            &Action::reply(Kind::BalancerStatus, status_payload()),
        );
        let device = Arc::new(Mutex::new(device));
        let factory = FakeFactory::new(VecDeque::from([Ok(device)]));
        let stale_events = collect_events::<SnapshotStale>(&event_bus);
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let events = stale_events.lock().await;
                let snapshot = inner.snapshots.load_full();
                events.len() == 1
                    && snapshot.connected
                    && snapshot
                        .cellcore
                        .rails
                        .as_ref()
                        .is_some_and(|cached| cached.stale)
            })
            .await,
            "rails must go stale while the device stays connected"
        );

        let events = stale_events.lock().await;
        assert_eq!(events[0].kind, "rails");
        assert!(events[0].identity.is_some());
        drop(events);

        let snapshot = inner.snapshots.load_full();
        assert!(snapshot.connected, "the device itself still answers");
        assert!(
            !snapshot
                .cellcore
                .temperatures
                .as_ref()
                .expect("temps")
                .stale
        );
        assert!(
            !snapshot
                .cellcore
                .cell_voltages
                .as_ref()
                .expect("voltages")
                .stale
        );

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn nacks_keep_the_device_connected_without_data() {
        let event_bus = EventBus::new();
        // A node that rejects everything, like a cellagent asked for
        // identity kinds: the driver must treat a Nack as an answer, not
        // as silence.
        let mut device = DeviceState::default();
        for request in [
            Kind::ReadDeviceId,
            Kind::ReadSerialNumber,
            Kind::ReadCellVoltages,
            Kind::ReadBalanceCurrents,
            Kind::ReadRails,
            Kind::ReadTemperatures,
            Kind::ReadBalancerStatus,
        ] {
            device.always(request, &Action::Nack(Vec::new()));
        }
        let device = Arc::new(Mutex::new(device));
        let factory = FakeFactory::new(VecDeque::from([Ok(device)]));
        let connected_events = collect_events::<DeviceConnected>(&event_bus);
        let stale_events = collect_events::<SnapshotStale>(&event_bus);
        let disconnected_events = collect_events::<DeviceDisconnected>(&event_bus);
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let snapshot = inner.snapshots.load_full();
                snapshot.connected
                    && snapshot.cellcore.identity.is_none()
                    && snapshot.cellcore.cell_voltages.is_none()
            })
            .await,
            "nacks must count as contact but store no data"
        );

        let connected = connected_events.lock().await;
        assert_eq!(connected.len(), 1);
        assert!(connected[0].identity.is_none());
        drop(connected);
        assert!(stale_events.lock().await.is_empty());
        assert!(disconnected_events.lock().await.is_empty());

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn corrupt_reply_is_a_kind_failure() {
        let event_bus = EventBus::new();
        // A reply frame whose payload CRC was corrupted in transit.
        let mut wire = {
            let mut raw = [0; 256];
            let raw_len =
                Packet::write(CELLCORE_ID, Kind::CellVoltages, &[9; 17], &mut raw).unwrap();
            let mut wire = vec![0; max_encoded_len(256)];
            let wire_len = encode_frame(&raw[..raw_len], &mut wire).unwrap();
            wire.truncate(wire_len);
            wire
        };
        let last_payload_index = wire.len() - 4;
        wire[last_payload_index] ^= 0x55;

        let mut device = DeviceState::default();
        answering(&mut device);
        // The cell-voltage replies are corrupt from the start: replace the
        // scripted good reply with garbage.
        let voltages = device
            .replies
            .iter_mut()
            .find(|(kind, _)| *kind == Kind::ReadCellVoltages)
            .map(|entry| &mut entry.1)
            .expect("scripted");
        voltages.clear();
        voltages.push_back(Action::Garbage(wire));
        device.always(
            Kind::ReadDeviceId,
            &Action::reply(Kind::DeviceId, device_id_payload()),
        );
        device.always(
            Kind::ReadSerialNumber,
            &Action::reply(Kind::SerialNumber, serial_payload()),
        );
        device.always(
            Kind::ReadBalanceCurrents,
            &Action::reply(Kind::BalanceCurrents, snapshot_payload(2, [10, 20, 30, 40])),
        );
        device.always(
            Kind::ReadRails,
            &Action::reply(Kind::Rails, rails_payload([1, 2, 3, 4, 5, 6, 7, 8])),
        );
        device.always(
            Kind::ReadTemperatures,
            &Action::reply(Kind::Temperatures, temps_payload()),
        );
        device.always(
            Kind::ReadBalancerStatus,
            &Action::reply(Kind::BalancerStatus, status_payload()),
        );
        let device = Arc::new(Mutex::new(device));
        let factory = FakeFactory::new(VecDeque::from([Ok(device)]));
        let stale_events = collect_events::<SnapshotStale>(&event_bus);
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let events = stale_events.lock().await;
                let snapshot = inner.snapshots.load_full();
                events.len() == 1 && snapshot.connected && snapshot.cellcore.cell_voltages.is_none()
            })
            .await,
            "corrupt replies must push the kind into staleness"
        );

        let events = stale_events.lock().await;
        assert_eq!(events[0].kind, "cell_voltages");
        drop(events);

        let snapshot = inner.snapshots.load_full();
        assert!(snapshot.connected, "the other kinds still answer");
        let rails = snapshot.cellcore.rails.as_ref().expect("rails");
        assert!(!rails.stale);

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn dead_device_disconnects_and_reconnects() {
        let event_bus = EventBus::new();
        // First device: one healthy round, then the link dies.
        let mut device = DeviceState::default();
        answering(&mut device);
        device.enqueue(Kind::ReadRails, Action::Die);
        let first = Arc::new(Mutex::new(device));

        // The port is gone for one open attempt, then a fresh device
        // appears and answers everything again.
        let second = answering_device();
        let factory = FakeFactory::new(VecDeque::from([Ok(first), absent(), Ok(second.clone())]));

        let connected_events = collect_events::<DeviceConnected>(&event_bus);
        let disconnected_events = collect_events::<DeviceDisconnected>(&event_bus);
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let disconnected = disconnected_events.lock().await;
                let connected = connected_events.lock().await;
                disconnected.len() == 1 && connected.len() == 2
            })
            .await,
            "the device must disconnect once and reconnect"
        );

        let connected = connected_events.lock().await;
        assert!(connected[0].identity.is_some(), "first connect identifies");
        assert!(
            connected[1].identity.is_some(),
            "reconnect identifies again"
        );
        drop(connected);

        let second = second.lock().await;
        assert_eq!(
            second.count(Kind::ReadDeviceId),
            1,
            "the fresh device is identified once"
        );
        drop(second);

        let snapshot = inner.snapshots.load_full();
        assert!(snapshot.connected, "the fresh device is connected");

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn open_retries_until_the_device_appears() {
        let event_bus = EventBus::new();
        let device = answering_device();
        let factory = FakeFactory::new(VecDeque::from([absent(), absent(), Ok(device)]));
        let connected_events = collect_events::<DeviceConnected>(&event_bus);
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let snapshot = inner.snapshots.load_full();
                snapshot.connected
            })
            .await
        );
        assert_eq!(connected_events.lock().await.len(), 1);

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_mid_poll_exits_promptly() {
        let event_bus = EventBus::new();
        // A device that accepts requests but never answers: the worker
        // sits inside a reply wait when the stop signal fires.
        let device: Device = Arc::new(Mutex::new(DeviceState::default()));
        let factory = FakeFactory::new(VecDeque::from([Ok(device.clone())]));
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner, factory));
        assert!(
            wait_for(|| async { !device.lock().await.received.is_empty() }).await,
            "the worker must reach the poll loop"
        );

        let start = Instant::now();
        let outcome = timeout(
            Duration::from_secs(2),
            supervisor.run_until_signal(sleep(Duration::from_millis(20))),
        )
        .await
        .expect("shutdown must not hang on a pending reply");
        assert!(matches!(outcome, ShutdownOutcome::Signaled), "{outcome:?}");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "shutdown must interrupt the in-flight exchange"
        );
    }

    #[tokio::test]
    async fn late_reply_lands_in_its_own_slot() {
        let event_bus = EventBus::new();
        // The rails reply arrives after its own timeout, during the
        // temperatures wait. The driver must store it under rails, not
        // corrupt the temperatures slot.
        let mut device = DeviceState::default();
        answering(&mut device);
        device.enqueue(
            Kind::ReadRails,
            Action::late_reply(
                Kind::Rails,
                rails_payload([7, 7, 7, 7, 7, 7, 7, 7]),
                Duration::from_millis(70),
            ),
        );
        device.always(
            Kind::ReadRails,
            &Action::reply(Kind::Rails, rails_payload([7, 7, 7, 7, 7, 7, 7, 7])),
        );
        let device = Arc::new(Mutex::new(device));
        let factory = FakeFactory::new(VecDeque::from([Ok(device)]));
        let inner = test_inner(test_config(), &event_bus);

        let mut supervisor = Supervisor::new();
        supervisor.spawn("cellguard", CellguardWorker::new(inner.clone(), factory));
        assert!(
            wait_for(|| async {
                let snapshot = inner.snapshots.load_full();
                snapshot
                    .cellcore
                    .rails
                    .as_ref()
                    .is_some_and(|cached| cached.data.codes[0] == 7)
            })
            .await,
            "the late rails reply must reach the rails slot"
        );

        let snapshot = inner.snapshots.load_full();
        let temps = snapshot.cellcore.temperatures.as_ref().expect("temps");
        assert_eq!(
            temps.data.temps,
            [2150, 2410, TEMP_INVALID],
            "the temperatures slot must hold its own reply"
        );

        supervisor.trigger();
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn published_snapshot_starts_empty_and_disconnected() {
        let event_bus = EventBus::new();
        let inner = test_inner(test_config(), &event_bus);
        let snapshot = inner.snapshots.load_full();
        assert!(!snapshot.connected);
        assert!(snapshot.cellcore.identity.is_none());
        assert!(snapshot.cellcore.cell_voltages.is_none());

        // The handle path publishes the same initial state.
        let driver = crate::Cellguard::new(test_config(), event_bus);
        assert!(!driver.snapshot().connected);
    }
}
