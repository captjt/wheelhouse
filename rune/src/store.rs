//! The app's one view of the hub (Store.swift): its state, kept live over the
//! hub's events, shared by every window. A window subscribes, and its state
//! is told when the hub's changes; actions are the hub's API, called off the
//! UI thread (inline in a headless run, so a script's frames don't depend on
//! the network).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde_json::Value;

use crate::hub::Hub;
use crate::model::{DocketPage, HubState};

type Listener = Box<dyn Fn() + Send>;
type EventListener = Box<dyn Fn(&Value) + Send>;

pub struct Store {
    pub hub: Hub,
    pub headless: bool,
    state: Mutex<Option<Arc<HubState>>>,
    error: Mutex<Option<String>>,
    live: AtomicBool,
    was_live: AtomicBool,
    listeners: Mutex<Vec<Listener>>,
    refresh: Mutex<Option<mpsc::Sender<()>>>,
    following: AtomicBool,
    /// Each event as it comes (the Notifier's `notify` ones).
    events: Mutex<Vec<EventListener>>,
    /// The app may post notifications (macOS allows it, and they're on).
    notifies: AtomicBool,
    /// The open stream told the hub so (`?client=mac`): only then are
    /// they the app's to post (Store.swift's `notifying`).
    notifying: AtomicBool,
    /// Follow again from the top, to tell the hub the change.
    again: AtomicBool,
}

impl Store {
    pub fn new(hub: Hub, headless: bool) -> Arc<Store> {
        let s = Arc::new(Store {
            hub,
            headless,
            state: Mutex::new(None),
            error: Mutex::new(None),
            live: AtomicBool::new(false),
            was_live: AtomicBool::new(false),
            listeners: Mutex::new(Vec::new()),
            refresh: Mutex::new(None),
            following: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
            notifies: AtomicBool::new(false),
            notifying: AtomicBool::new(false),
            again: AtomicBool::new(false),
        });
        // A headless run renders the state as it is at launch.
        if headless {
            s.load_now();
        }
        s
    }

    pub fn state(&self) -> Option<Arc<HubState>> {
        self.state.lock().ok().and_then(|s| s.clone())
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }
    pub fn live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }
    pub fn was_live(&self) -> bool {
        self.was_live.load(Ordering::Relaxed)
    }
    /// The hub isn't answering at all.
    pub fn hub_down(&self) -> bool {
        self.state().is_none() && self.error().as_deref() == Some("The hub isn't running")
    }

    /// Tell `f` whenever the state changes: a window bumps its version.
    pub fn subscribe(&self, f: impl Fn() + Send + 'static) {
        if let Ok(mut l) = self.listeners.lock() {
            l.push(Box::new(f));
        }
    }

    /// Hear each of the hub's events, on the stream's thread.
    pub fn on_event(&self, f: impl Fn(&Value) + Send + 'static) {
        if let Ok(mut l) = self.events.lock() {
            l.push(Box::new(f));
        }
    }

    /// Whether the app posts the Mac's notifications: a change follows the
    /// hub again, telling it (Store.swift's checkNotifying).
    pub fn set_notifies(&self, on: bool) {
        if self.notifies.swap(on, Ordering::SeqCst) != on {
            self.again.store(true, Ordering::SeqCst);
        }
    }

    /// The open stream told the hub the app posts them.
    pub fn notifying(&self) -> bool {
        self.notifying.load(Ordering::Relaxed)
    }

    fn notify(&self) {
        if let Ok(l) = self.listeners.lock() {
            for f in l.iter() {
                f();
            }
        }
    }

    fn landed(&self, r: Result<Value, String>) {
        match r {
            Ok(j) => {
                let next = HubState::parse(j);
                if let Ok(mut s) = self.state.lock() {
                    if s.as_deref() == Some(&next) {
                        return;
                    }
                    *s = Some(Arc::new(next));
                }
                if let Ok(mut e) = self.error.lock() {
                    *e = None;
                }
            }
            Err(e) => {
                if let Ok(mut err) = self.error.lock() {
                    *err = Some(e);
                }
            }
        }
        self.notify();
    }

    /// Fetch the state now, on this thread.
    pub fn load_now(&self) {
        let r = self.hub.get("/api/state");
        self.landed(r);
    }

    /// Ask for the state again: now, inline, in a headless run; soon, once for
    /// a burst, in a window.
    pub fn reload(&self) {
        if self.headless {
            self.load_now();
        } else if let Some(tx) = self.refresh.lock().ok().and_then(|r| r.clone()) {
            let _ = tx.send(());
        }
    }

    /// Follow the hub's events, and refresh after each burst of them, as the
    /// Mac app's Store does: 150 ms after the first event, once for any that
    /// came meanwhile. Once per process.
    pub fn follow(self: &Arc<Self>) {
        if self.headless || self.following.swap(true, Ordering::SeqCst) {
            return;
        }
        let (tx, rx) = mpsc::channel::<()>();
        if let Ok(mut r) = self.refresh.lock() {
            *r = Some(tx.clone());
        }
        let me = self.clone();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                std::thread::sleep(Duration::from_millis(150));
                while rx.try_recv().is_ok() {}
                me.load_now();
            }
        });
        let me = self.clone();
        std::thread::spawn(move || {
            let mut wait = Duration::from_secs(1);
            loop {
                let _ = tx.send(());
                let (m1, m2, poke) = (me.clone(), me.clone(), tx.clone());
                me.again.store(false, Ordering::SeqCst);
                let mac = me.notifies.load(Ordering::SeqCst);
                let why = me.hub.follow_events(
                    mac,
                    move || {
                        m1.live.store(true, Ordering::Relaxed);
                        m1.was_live.store(true, Ordering::Relaxed);
                        m1.notifying.store(mac, Ordering::Relaxed);
                        wait_reset();
                    },
                    move |ev| {
                        if let Ok(l) = m2.events.lock() {
                            for f in l.iter() {
                                f(&ev);
                            }
                        }
                        let _ = poke.send(());
                    },
                    &me.again,
                );
                me.live.store(false, Ordering::Relaxed);
                me.notifying.store(false, Ordering::Relaxed);
                let _ = tx.send(());
                // Followed again on purpose (notifications came on or off): at once.
                if why == "Following again" {
                    continue;
                }
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_secs(10));
            }
        });
        fn wait_reset() {}
    }

    /// A docket's page: its record, evidence, decisions and what's queued.
    pub fn page(&self, id: &str) -> Result<DocketPage, String> {
        self.hub.get(&format!("/api/dockets/{id}")).map(DocketPage::parse)
    }

    /// A headless scene's made-up state: the hub's, as `f` changes it (a
    /// Build the made-up world has none of). Never in a window.
    pub fn amend(&self, f: impl FnOnce(&mut Value)) {
        if !self.headless {
            return;
        }
        let Some(cur) = self.state() else { return };
        let mut j = cur.j.clone();
        f(&mut j);
        if let Ok(mut s) = self.state.lock() {
            *s = Some(Arc::new(HubState::parse(j)));
        }
    }

    /// A write to the hub, on this thread. The state follows on the event.
    pub fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let r = self.hub.post(path, body);
        if r.is_ok() {
            self.reload();
        }
        r
    }
}
