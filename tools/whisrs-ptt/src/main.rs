use std::fs;
use std::fs::File;
use std::io::Read;
use std::mem;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const WHISRS: &str = "/home/dada/.local/bin/whisrs";

const EV_KEY: u16 = 0x01;

const KEY_LEFTCTRL: u16 = 29;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_ENTER: u16 = 28;

#[repr(C, packed)]
struct InputEvent {
    _tv_sec: i64,
    _tv_usec: i64,
    event_type: u16,
    code: u16,
    value: i32,
}

fn find_event_devices() -> Vec<String> {
    let mut devices = vec![];
    let dir = match fs::read_dir("/dev/input") {
        Ok(d) => d,
        Err(_) => return devices,
    };
    for entry in dir.flatten() {
        let path = entry.path();
        let fname = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        if fname.starts_with("event") {
            devices.push(path.to_string_lossy().to_string());
        }
    }
    devices
}

fn main() {
    let ctrl = Arc::new(AtomicBool::new(false));
    let enter = Arc::new(AtomicBool::new(false));
    let recording = Arc::new(AtomicBool::new(false));

    loop {
        let devices = find_event_devices();
        if devices.is_empty() {
            eprintln!("no event devices found, retrying in 5s...");
            thread::sleep(Duration::from_secs(5));
            continue;
        }

        eprintln!("monitoring {} event device(s)", devices.len());
        let mut handles = vec![];

        for path_str in devices {
            let path = path_str;
            let ctrl = ctrl.clone();
            let enter = enter.clone();
            let recording = recording.clone();

            handles.push(thread::spawn(move || loop {
                let mut file = match File::open(&path) {
                    Ok(f) => f,
                    Err(_) => {
                        thread::sleep(Duration::from_secs(5));
                        continue;
                    }
                };

                loop {
                    let mut buf = [0u8; 24];
                    if let Err(e) = file.read_exact(&mut buf) {
                        eprintln!("{path}: {e}, re-enumerating...");
                        break;
                    }
                    let event: InputEvent = unsafe { mem::transmute(buf) };
                    if event.event_type != EV_KEY {
                        continue;
                    }
                    let value = event.value;
                    if value == 2 {
                        continue;
                    }

                    let is_ctrl = event.code == KEY_LEFTCTRL || event.code == KEY_RIGHTCTRL;
                    let is_enter = event.code == KEY_ENTER;

                    if is_ctrl {
                        if value != 0 && !ctrl.swap(true, Ordering::SeqCst) {
                            if enter.load(Ordering::SeqCst)
                                && !recording.swap(true, Ordering::SeqCst)
                            {
                                let _ = Command::new(WHISRS).args(["toggle"]).spawn();
                            }
                        } else if value == 0 && ctrl.swap(false, Ordering::SeqCst) {
                            if recording.swap(false, Ordering::SeqCst) {
                                let _ = Command::new(WHISRS).args(["toggle"]).spawn();
                            }
                        }
                    }

                    if is_enter {
                        if value != 0 && !enter.swap(true, Ordering::SeqCst) {
                            if ctrl.load(Ordering::SeqCst)
                                && !recording.swap(true, Ordering::SeqCst)
                            {
                                let _ = Command::new(WHISRS).args(["toggle"]).spawn();
                            }
                        } else if value == 0 && enter.swap(false, Ordering::SeqCst) {
                            if recording.swap(false, Ordering::SeqCst) {
                                let _ = Command::new(WHISRS).args(["toggle"]).spawn();
                            }
                        }
                    }
                }
            }));
        }

        for handle in handles {
            let _ = handle.join();
        }
        thread::sleep(Duration::from_secs(2));
    }
}
