//! One copy of Startline per user session. A named mutex, taken before anything else, decides it: the first copy keeps
//! it while it runs, and Windows lets go of it when that copy ends, however it ends. Looking for the running copy's
//! window, as Startline did before, came too late: that window only exists seconds after the start, so copies started
//! together (two autostart entries at logon, several clicks, the installed copy and a build of the source) all found
//! nothing and all stayed.

use crate::platform::wide;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::PCWSTR;

/// The mutex for this session and account. Local\ makes it one per session, whatever folder the program runs from.
pub fn session_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default().replace('\\', "_");
    format!("Local\\Startline.OneCopy.{user}")
}

/// Held by the copy that runs. Dropping it lets the next copy have it; so does the end of the process.
pub struct Claim(Option<HANDLE>);

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            unsafe {
                let _ = ReleaseMutex(handle);
                let _ = CloseHandle(handle);
            }
        }
    }
}

// Only ever used from the thread that took it.
unsafe impl Send for Claim {}

/// Takes the mutex `name`: at once when no copy holds it, or after waiting up to `wait` for the one that does to end.
/// None: another copy runs. When Windows can't make the mutex at all, this copy runs rather than none.
pub fn claim(name: &str, wait: Duration) -> Option<Claim> {
    let name = wide(name);
    let made = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) };
    let handle = match made {
        // A copy with administrator rights made it, so this one may not open it: that copy runs.
        Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => return None,
        Err(_) => return Some(Claim(None)),
        Ok(handle) => handle,
    };
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Some(Claim(Some(handle)));
    }
    let waited = unsafe { WaitForSingleObject(handle, wait.as_millis().min(u32::MAX as u128) as u32) };
    if waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED {
        Some(Claim(Some(handle)))
    } else {
        let _ = unsafe { CloseHandle(handle) };
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn test_name(what: &str) -> String {
        format!("Local\\Startline.Test.{what}.{}", std::process::id())
    }

    #[test]
    fn only_the_first_of_many_starting_at_once_runs() {
        let name = test_name("race");
        // Eight starts at the same moment, each on its own thread, as separate copies would be.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (name, barrier, tx, release_rx) = (name.clone(), barrier.clone(), tx.clone(), release_rx.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    let claim = claim(&name, Duration::ZERO);
                    let won = claim.is_some();
                    tx.send(won).unwrap();
                    if won {
                        // The winner keeps running until the test is done counting.
                        let _ = release_rx.lock().unwrap().recv();
                    }
                    drop(claim);
                })
            })
            .collect();
        let winners = (0..8).filter(|_| rx.recv().unwrap()).count();
        assert_eq!(winners, 1, "exactly one copy keeps running");
        release_tx.send(()).unwrap();
        for thread in threads {
            thread.join().unwrap();
        }
    }

    #[test]
    fn the_next_copy_runs_once_the_first_has_ended() {
        let name = test_name("after");
        let (held_tx, held_rx) = mpsc::channel();
        let (end_tx, end_rx) = mpsc::channel::<()>();
        let first = {
            let name = name.clone();
            std::thread::spawn(move || {
                let claim = claim(&name, Duration::ZERO);
                held_tx.send(claim.is_some()).unwrap();
                let _ = end_rx.recv();
                // Ends without letting go on purpose, as a copy that's killed would: Windows lets go for it.
                std::mem::forget(claim);
            })
        };
        assert!(held_rx.recv().unwrap());
        assert!(claim(&name, Duration::from_millis(50)).is_none(), "while the first runs, a second gives way");
        end_tx.send(()).unwrap();
        first.join().unwrap();
        // An update's new copy waits for the old one; here it has already gone.
        assert!(claim(&name, Duration::from_secs(2)).is_some(), "after it, the next one runs");
    }

    #[test]
    fn a_copy_waits_for_the_one_it_replaces_but_never_forever() {
        let name = test_name("wait");
        let held = claim(&name, Duration::ZERO).expect("free at first");
        let other = {
            let name = name.clone();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let got = claim(&name, Duration::from_millis(300)).is_some();
                (got, started.elapsed())
            })
        };
        let (got, took) = other.join().unwrap();
        assert!(!got && took >= Duration::from_millis(250) && took < Duration::from_secs(5), "gave way after its wait: {took:?}");
        drop(held);
    }
}
