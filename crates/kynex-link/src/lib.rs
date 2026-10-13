//! KYNEX in a Kreate shared session: Kreate's scene, kept in step as it is
//! edited.
//!
//! KYNEX joins the session the way a second Kreate would — same relay, same
//! patches — and takes in every edit the moment the relay passes it on. It
//! only listens: it sends no patches, so nothing KYNEX does can change the
//! scene a person is modelling.
//!
//! One rule differs from a Kreate joining. A participant that finds the
//! session empty seeds it with its own scene; KYNEX has none to offer, and an
//! empty seed would wipe the scene of the Kreate that arrives next. So when
//! the session is not yet seeded KYNEX steps back out and knocks again later.

use std::time::{Duration, Instant};

use kreate_collab::client::{Connection, Incoming};
use kreate_collab::wire::{Msg, Peer};
use kreate_collab::DEFAULT_PORT;
use kreate_scene::{Changes, Document};

/// The room a Kreate session uses: Kreate offers no other.
const ROOM: &str = "main";

/// Where the session is and how to get in.
#[derive(Clone, Debug)]
pub struct LinkConfig {
    /// `host:port`, or just a host for Kreate's default port.
    pub address: String,
    /// The session key Kreate's host set; empty when it set none.
    pub key: String,
    /// The name Kreate shows for this participant.
    pub name: String,
    /// How long to wait before knocking again after the session was closed,
    /// empty, or not there.
    pub retry: Duration,
}

impl LinkConfig {
    pub fn new(address: impl Into<String>) -> Self {
        let address = address.into();
        let address = if address.contains(':') {
            address
        } else {
            format!("{address}:{DEFAULT_PORT}")
        };
        Self {
            address,
            key: String::new(),
            name: "KYNEX".into(),
            retry: Duration::from_secs(1),
        }
    }
}

/// What happened since the last [`KreateLink::poll`].
#[derive(Clone, Debug, PartialEq)]
pub enum LinkEvent {
    /// In the session as participant `site`, with these others.
    Joined {
        site: u16,
        peers: Vec<Peer>,
    },
    /// The session exists but holds no scene yet: Kreate has not opened it
    /// with one. KYNEX will knock again.
    WaitingForScene,
    /// The whole scene arrived, replacing whatever was held before.
    SceneReplaced,
    /// An edit made in Kreate landed; these datablocks changed or went.
    Changed(Changes),
    PeerJoined(Peer),
    PeerLeft {
        site: u16,
    },
    /// Not connected, and why. KYNEX will knock again.
    Offline(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Offline,
    Knocking,
    AwaitingScene,
    Live,
}

/// A live, read-only copy of a Kreate session's scene.
pub struct KreateLink {
    config: LinkConfig,
    conn: Option<Connection>,
    state: State,
    doc: Document,
    next_try: Instant,
    /// The last thing said about being out, so a retry loop says it once.
    last_out: Option<LinkEvent>,
    /// When the last edit landed.
    last_edit: Option<Instant>,
    edits: u64,
}

impl KreateLink {
    /// Starts linking; the first knock happens on the first poll.
    pub fn new(config: LinkConfig) -> Self {
        Self {
            config,
            conn: None,
            state: State::Offline,
            doc: Document::new(),
            next_try: Instant::now(),
            last_out: None,
            last_edit: None,
            edits: 0,
        }
    }

    /// The scene as Kreate has it, as of the last poll. Empty until the
    /// link is live.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn is_live(&self) -> bool {
        self.state == State::Live
    }

    pub fn address(&self) -> &str {
        &self.config.address
    }

    /// How many edits have landed since the link went live.
    pub fn edits(&self) -> u64 {
        self.edits
    }

    pub fn last_edit(&self) -> Option<Instant> {
        self.last_edit
    }

    /// Takes in everything that has arrived, without waiting, and knocks
    /// again when it is time to.
    pub fn poll(&mut self) -> Vec<LinkEvent> {
        let mut events = Vec::new();
        if self.conn.is_none() && Instant::now() >= self.next_try {
            self.knock(&mut events);
        }
        let arrived = match &self.conn {
            Some(conn) => conn.poll(),
            None => Vec::new(),
        };
        for item in arrived {
            match item {
                Incoming::Frame(frame) => self.take(frame.msg, &frame.body, &mut events),
                Incoming::Closed(reason) => {
                    self.go_offline(LinkEvent::Offline(reason), &mut events)
                }
            }
            if self.conn.is_none() {
                break;
            }
        }
        events
    }

    /// Polls until something happens or `timeout` passes.
    pub fn wait(&mut self, timeout: Duration) -> Vec<LinkEvent> {
        let until = Instant::now() + timeout;
        loop {
            let events = self.poll();
            if !events.is_empty() || Instant::now() >= until {
                return events;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn knock(&mut self, events: &mut Vec<LinkEvent>) {
        match Connection::open(
            &self.config.address,
            &self.config.name,
            ROOM,
            &self.config.key,
        ) {
            Ok(conn) => {
                self.conn = Some(conn);
                self.state = State::Knocking;
            }
            Err(e) => self.go_offline(LinkEvent::Offline(e.to_string()), events),
        }
    }

    fn take(&mut self, msg: Msg, body: &[u8], events: &mut Vec<LinkEvent>) {
        match msg {
            Msg::Welcome {
                site,
                peers,
                seeded,
                ..
            } => {
                if !seeded {
                    // Seeding would hand the session an empty scene.
                    self.go_offline(LinkEvent::WaitingForScene, events);
                    return;
                }
                self.state = State::AwaitingScene;
                self.last_out = None;
                events.push(LinkEvent::Joined { site, peers });
            }
            Msg::Snapshot { .. } => {
                let mut fresh = Document::new();
                match kreate_io::sync::apply(&mut fresh, body) {
                    Ok(_) => {
                        self.doc = fresh;
                        self.state = State::Live;
                        self.edits = 0;
                        self.last_edit = Some(Instant::now());
                        events.push(LinkEvent::SceneReplaced);
                    }
                    Err(e) => self.go_offline(
                        LinkEvent::Offline(format!("the session's scene did not read: {e}")),
                        events,
                    ),
                }
            }
            Msg::Applied { .. } if self.state == State::Live => {
                match kreate_io::sync::decode(body) {
                    Ok((patch, changes)) => {
                        self.doc.apply_patch(&patch, &changes);
                        self.edits += 1;
                        self.last_edit = Some(Instant::now());
                        events.push(LinkEvent::Changed(changes));
                    }
                    // The relay applied it, so this copy would now differ
                    // from everyone's: start again from a fresh snapshot.
                    Err(e) => self.go_offline(
                        LinkEvent::Offline(format!("an edit did not read: {e}")),
                        events,
                    ),
                }
            }
            Msg::Joined(peer) => events.push(LinkEvent::PeerJoined(peer)),
            Msg::Left { site } => events.push(LinkEvent::PeerLeft { site }),
            Msg::Refused { reason } => self.go_offline(LinkEvent::Offline(reason), events),
            _ => {}
        }
    }

    fn go_offline(&mut self, event: LinkEvent, events: &mut Vec<LinkEvent>) {
        self.conn = None;
        self.state = State::Offline;
        self.next_try = Instant::now() + self.config.retry;
        if self.last_out.as_ref() != Some(&event) {
            self.last_out = Some(event.clone());
            events.push(event);
        }
    }
}
