use rdev::EventType::{KeyPress, KeyRelease};
use rdev::Key::{Alt, AltGr, ControlLeft, ControlRight, Insert, KeyD, KeyE, KeyS, ShiftLeft};
use rdev::{listen, Event, Key};
use std::collections::HashMap;
use std::error::Error;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
            match state_ptr.is_on() {
                true => {
                    if state_ptr != old_state_ptr {
                        Self::send_on_cmd().expect("Could not send.");
                        old_state_ptr.set_from(&state_ptr);
                    }
                }
                false => {
                    while state_ptr.is_off() {
                        Self::send_off_cmd().expect("Could not send.");
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            };
        });

        Self {
            state,
            old_state,
            update,
        }
    }

    fn send_off_cmd() -> Result<Output, Box<dyn Error>> {
        Ok(Command::new("xset")
            .arg("dpms")
            .arg("force")
            .arg("off")
            .spawn()?
            .wait_with_output()?)
    }

    fn send_on_cmd() -> Result<Output, Box<dyn Error>> {
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

impl KeyStates {
    fn new() -> Self {
        let mut map = HashMap::new();
        Self(Arc::new(Mutex::new(map)))
    }

    fn get_state(&self, key: Key) -> KeyState {
        *self
            .0
            .lock()
            .unwrap()
            .entry(key)
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
}

impl KeyboardState {
    fn new() -> Self {
        let states = KeyStates::new();
        let states_ptr = states.clone();
        let callback = move |event: Event| match event {
            Event {
                time: _,
                name: _,
                event_type,
            } => match event_type {
                KeyPress(key) => states_ptr.set_state(&key, KeyState::Pressed),
                KeyRelease(key) => states_ptr.set_state(&key, KeyState::Released),
                _ => {}
            },
        };

        let update = thread::spawn(move || {
            if let Err(error) = listen(callback) {
                eprintln!("Error: {:?}", error)
            }
        });

        Self { update, states }
    }
}

static DEBOUNCE_MS: u128 = 1000;

fn main() {
    let kb = KeyboardState::new();
    let mut ssenforcer = ScreenStateEnforcer::new();
    let mut time_since_last_toggle = Instant::now();
    loop {
        match (
            kb.states.get_state(ControlLeft),
            kb.states.get_state(Alt),
            kb.states.get_state(KeyD),
        ) {
            (KeyState::Pressed, KeyState::Pressed, KeyState::Pressed) => {
                if time_since_last_toggle.elapsed().as_millis() > DEBOUNCE_MS {
                    ssenforcer.state.toggle();
                    time_since_last_toggle = Instant::now();
                }
            }
            _ => {}
        }
        thread::sleep(Duration::from_millis(10));
    }
}
