use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const WHISRS_SOCK: &str = "/run/user/1000/whisrs.sock";

const EV_KEY: u16 = 0x01;

const KEY_LEFTCTRL: u16 = 29;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTALT: u16 = 100;

#[repr(C)]
struct InputEvent {
    _tv_sec: i64,
    _tv_usec: i64,
    event_type: u16,
    code: u16,
    value: i32,
}

fn send_toggle() {
    if let Ok(mut sock) = UnixStream::connect(WHISRS_SOCK) {
        let body = br#"{"cmd":"toggle"}"#;
        let len = (body.len() as u32).to_be_bytes();
        let _ = sock.write_all(&len);
        let _ = sock.write_all(body);
    }
}

fn find_keyboards() -> Vec<String> {
    let mut devices = vec![];
    let dir = match fs::read_dir("/sys/class/input") {
        Ok(d) => d,
        Err(_) => return devices,
    };
    for entry in dir.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if !name.starts_with("event") {
            continue;
        }
        let name_path = entry.path().join("device/name");
        if let Ok(dev_name) = fs::read_to_string(&name_path) {
            let n = dev_name.trim().to_lowercase();
            if n.contains("keyboard") || n.contains("kbd") || n.contains("at translated") {
                devices.push(format!("/dev/input/{}", name));
            }
        }
    }
    devices
}

fn main() {
    let ctrl = Arc::new(AtomicBool::new(false));
    let alt = Arc::new(AtomicBool::new(false));
    let recording = Arc::new(AtomicBool::new(false));
    let last_toggle = Arc::new(std::sync::Mutex::new(Instant::now()));

    loop {
        let keyboards = find_keyboards();
        if keyboards.is_empty() {
            eprintln!("no keyboards found, retrying in 5s...");
            thread::sleep(Duration::from_secs(5));
            continue;
        }

        eprintln!("monitoring {} keyboard(s)", keyboards.len());
        let mut handles = vec![];

        for path_str in keyboards {
            let path = path_str;
            let ctrl = ctrl.clone();
            let alt = alt.clone();
            let recording = recording.clone();
            let last_toggle = last_toggle.clone();

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
                    let event: InputEvent = unsafe { std::ptr::read(buf.as_ptr() as *const _) };
                    if event.event_type != EV_KEY {
                        continue;
                    }
                    let value = event.value;
                    if value == 2 {
                        continue;
                    }

                    let is_ctrl = event.code == KEY_LEFTCTRL || event.code == KEY_RIGHTCTRL;
                    let is_alt = event.code == KEY_LEFTALT || event.code == KEY_RIGHTALT;

                    let toggle = |rec: &AtomicBool| {
                        let mut last = last_toggle.lock().unwrap();
                        if last.elapsed() < Duration::from_millis(200) {
                            return;
                        }
                        *last = Instant::now();
                        drop(last);
                        let was = rec.swap(!rec.load(Ordering::SeqCst), Ordering::SeqCst);
                        if was {
                            send_toggle();
                        }
                    };

                    if is_ctrl {
                        ctrl.store(value != 0, Ordering::SeqCst);
                        if value == 0 && recording.load(Ordering::SeqCst) {
                            toggle(&recording);
                        }
                    }

                    if is_alt {
                        alt.store(value != 0, Ordering::SeqCst);
                        if value == 0 && recording.load(Ordering::SeqCst) {
                            toggle(&recording);
                        }
                    }

                    if value != 0 && ctrl.load(Ordering::SeqCst) && alt.load(Ordering::SeqCst) {
                        if !recording.load(Ordering::SeqCst) {
                            toggle(&recording);
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
