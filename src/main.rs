// So every time I locked one of my machines, lately, the screen would turn off like it's supposed to,
// but then it'd turn right back on. I couldn't find any information on why such a thing would happen,
// but I strongly suspect it's because GNOME3 is utter garbage.

// Anyway, this program just catches the right key combo and then calls `xset` to "correct" the issue.
// Of course, it would turn it back on immediately if it only called xset once to turn the screen off,
// so instead it runs it over and over and over and over and over and over and over and...

// This is horrible
// Do not learn from me, kids
// I am not a good role model.

use futures::executor::block_on;
use futures_signals::signal;
use futures_signals::signal::{Mutable, SignalExt};
use rdev::EventType::{KeyPress, KeyRelease};
use rdev::Key::{Alt, AltGr, ControlLeft, ControlRight, Insert, KeyD, KeyE, KeyS, ShiftLeft};
use rdev::{listen, Event, Key};
use std::collections::HashMap;
use std::error::Error;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct ScreenState(Arc<AtomicBool>);

impl Clone for ScreenState {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl PartialEq for ScreenState {
    fn eq(&self, other: &Self) -> bool {
        other.0.load(Ordering::SeqCst) == self.0.load(Ordering::SeqCst)
    }
}

impl ScreenState {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    fn is_off(&self) -> bool {
        !self.0.load(Ordering::SeqCst)
    }
    fn is_on(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    fn set_on(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn set_off(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
    fn set_from(&mut self, other: &Self) {
        self.0
            .store(other.0.load(Ordering::SeqCst), Ordering::SeqCst);
    }
    fn toggle(&mut self) {
        let current_state = self.0.load(Ordering::SeqCst);
        self.0.store(!current_state, Ordering::SeqCst);
    }
}

struct ScreenStateEnforcer {
    state: ScreenState,
    old_state: ScreenState,
    update: JoinHandle<()>,
}

impl ScreenStateEnforcer {
    fn new() -> Self {
        let state = ScreenState::new();
        let old_state = ScreenState::new();

        let state_ptr = state.clone();
        let mut old_state_ptr = old_state.clone();
        let update = thread::spawn(move || loop {
            while state_ptr.is_off() {
                // Now we turn off the screens over and over EVERY 100 MS
                // faster than whatever's turning them on can act
                // Yes, this is horrifying.
                // Yes, I don't care.
                // Fuck off.
                Self::send_off_cmd().expect("Could not send.");
                thread::sleep(Duration::from_millis(50));
            }
            // only reachable once state_ptr becomes on, we we assume that
            // Debounce against previous state so we only send if state has changed
            if state_ptr != old_state_ptr {
                // Send this command 100 times because I don't trust anyone else's code but my own
                for _ in 0..100 {
                    // Send command to turn screen on
                    Self::send_on_cmd().expect("Could not send.");
                    thread::sleep(Duration::from_millis(100));
                }
                old_state_ptr.set_from(&state_ptr);
            }
            thread::sleep(Duration::from_millis(50));
        });

        Self {
            state,
            old_state,
            update,
        }
    }

    /// Send command to turn screen off
    fn send_off_cmd() -> Result<Output, Box<dyn Error>> {
        println!("send_off_cmd()");
        Ok(Command::new("xset")
            .arg("dpms")
            .arg("force")
            .arg("off")
            .spawn()?
            .wait_with_output()?)
    }

    /// Send command to turn screen on
    fn send_on_cmd() -> Result<Output, Box<dyn Error>> {
        println!("send_on_cmd()");
        Ok(Command::new("xset")
            .arg("dpms")
            .arg("force")
            .arg("on")
            .spawn()?
            .wait_with_output()?)
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum KeyState {
    Pressed,
    Released,
}

#[derive(Debug)]
struct KeyStates(Arc<Mutex<HashMap<Key, KeyState>>>);

impl Clone for KeyStates {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// BTW Wrapping Arc<Mutex<>> like this is probably a bad idea for any _real_ API
// As it would probably be easy for the API-user to end up with deadlocks and be very confused ;)

impl KeyStates {
    fn new() -> Self {
        let mut map = HashMap::new();
        Self(Arc::new(Mutex::new(map)))
    }

    fn get_state(&self, key: Key) -> KeyState {
        *self
            .0
            .lock()
            .unwrap() // ah yes, 500 unwrap() in your codebase
            .entry(key) // truly masterful programming quality
            .or_insert(KeyState::Released)
    }

    fn set_state(&self, key: &Key, state: KeyState) {
        *self.0.lock().unwrap().entry(*key).or_insert(state) = state;
    }
}

#[derive(Debug)]
struct KeyboardState {
    update: JoinHandle<()>,
    states: KeyStates,
    //rx: Receiver<()>,
    updated: Mutable<bool>,
}

impl KeyboardState {
    fn new() -> Self {
        let updated = Mutable::new(false);
        let updated_ptr = updated.clone();
        // let (tx, rx) = mpsc::channel::<()>();
        let states = KeyStates::new();
        let states_ptr = states.clone();
        let callback = move |event: Event| {
            updated_ptr.set(true);
            // tx.send(()).expect("Couldn't send notice of new event");

            match event {
                Event {
                    time: _,
                    name: _,
                    event_type,
                } => match event_type {
                    KeyPress(key) => states_ptr.set_state(&key, KeyState::Pressed),
                    KeyRelease(key) => states_ptr.set_state(&key, KeyState::Released),
                    _ => {}
                },
            }
        };

        let update = thread::spawn(move || {
            if let Err(error) = listen(callback) {
                eprintln!("Error: {:?}", error)
            }
        });

        Self {
            update,
            states,
            updated,
        }
    }

    fn wait_until_next(&self) {
        block_on(self.updated.signal().wait_for(true));
        self.updated.set(false);
        //self.rx.recv().expect("Sender hung up :c");
    }
}

static DEBOUNCE_MS: u128 = 500;

fn main() {
    let kb = KeyboardState::new();
    let mut ssenforcer = ScreenStateEnforcer::new();
    let mut time_since_last_toggle = Instant::now();
    loop {
        kb.wait_until_next();
        match (
            kb.states.get_state(ControlLeft),
            kb.states.get_state(Alt),
            kb.states.get_state(KeyD),
        ) {
            (KeyState::Pressed, KeyState::Pressed, KeyState::Pressed) => {
                if time_since_last_toggle.elapsed().as_millis() > DEBOUNCE_MS {
                    println!("Toggle");
                    ssenforcer.state.toggle();
                    time_since_last_toggle = Instant::now();
                }
            }
            _ => {}
        }
        thread::sleep(Duration::from_millis(10));
    }
}
