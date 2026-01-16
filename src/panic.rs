use std::panic;
use std::sync::Mutex;

type Callback = dyn FnOnce() + Send + 'static;
static UNWIND_CALLBACKS: Mutex<Vec<Box<Callback>>> = Mutex::new(Vec::new());

pub fn init() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if let Ok(mut callbacks) = UNWIND_CALLBACKS.lock() {
            for cb in callbacks.drain(..) {
                cb();
            }
        }
        if let Some(loc) = info.location() {
            let err = format!("panic at {}:{}:{}\n  {}",
                loc.file(), loc.line(), loc.column(),
                info.payload_as_str().unwrap_or("<no-panic-string-available>"));
            crate::log::log(&err);
        }
        default_hook(info)
    }));
}

fn on_unwind_(cb: Box<Callback>) {
    match UNWIND_CALLBACKS.lock() {
        Ok(mut callbacks) => {
            if callbacks.is_empty() {
                debug_assert!(callbacks.capacity() == 0);
            }
            callbacks.push(cb);
        }
        Err(err) => {
            if cfg!(debug_assertions) {
                panic!("failed to lock SHUTDOWN_CALLBACKS: {err:?}");
            }
        }
    }
}

pub fn on_unwind(cb: impl FnOnce() + Send + 'static) {
    on_unwind_(Box::new(cb));
}

pub fn leak_unwind<T>(fun: impl FnOnce() -> T + panic::UnwindSafe) -> Option<T> {
    let res = panic::catch_unwind(fun);

    match res {
        Ok(t) => Some(t),
        Err(err) => {
            core::mem::forget(err);
            None
        }
    }
}
