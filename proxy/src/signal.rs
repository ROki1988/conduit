//! Unix signal handling for the proxy binary.

extern crate futures;
extern crate tokio_signal;

use self::futures::Future;

type ShutdownSignal = Box<Future<Item=(), Error=()> + Send>;

/// Returns a `Future` that completes when the proxy should start to shutdown.
pub fn shutdown() -> ShutdownSignal {
    imp::shutdown()
}

#[cfg(unix)]
mod imp {
    use std::fmt;

    use super::futures::{future, Future, Stream};
    use super::tokio_signal::unix::{Signal, SIGINT, SIGTERM};
    use super::ShutdownSignal;

    pub(super) fn shutdown() -> ShutdownSignal {
        // SIGTERM - Kubernetes sends this to start a graceful shutdown.
        // SIGINT  - To allow Ctrl-c to emulate SIGTERM while developing.
        //
        // If you add to this list, you should also update DisplaySignal below
        // to output nicer signal names.
        let on_any_signal = future::lazy(|| {
            // Do everything lazily to prevent `Signal::new()` from calling
            // `Handle::current()` and starting the background reactor when
            // we haven't initialized the runtime yet.
            Ok([SIGINT, SIGTERM].into_iter())
        }).and_then(|signals| {
            let signals = signals.map(|&sig| {
                // Create a Future that completes the first
                // time the process receives 'sig'.
                Signal::new(sig)
                    .flatten_stream()
                    .into_future()
                    .map(move |_| {
                        info!(
                            // use target to remove 'imp' from output
                            target: "conduit_proxy::signal",
                            "received {}, starting shutdown",
                            DisplaySignal(sig),
                        );
                    })
            });
            // With a list of Futures that could notify us,
            // we just want to know when the first one triggers,
            // so select over them all.
            future::select_all(signals)
                .map(|_| ())
                .map_err(|_| unreachable!("Signal never returns an error"))
        });

        Box::new(on_any_signal)
    }

    struct DisplaySignal(i32);

    impl fmt::Display for DisplaySignal {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let s = match self.0 {
                SIGINT => "SIGINT",
                SIGTERM => "SIGTERM",
                other => return write!(f, "signal {}", other),
            };
            f.write_str(s)
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::{tokio_signal, Handle, ShutdownSignal};
    use super::futures::{Future, Stream};

    pub(super) fn shutdown() -> ShutdownSignal {
        // On Windows, we don't have all the signals, but Windows also
        // isn't our expected deployment target. This implementation allows
        // developers on Windows to simulate proxy graceful shutdown
        // by pressing Ctrl-C.
        let on_ctrl_c = tokio_signal::ctrl_c()
            .flatten_stream()
            .into_future()
            .map(|_| {
                info!(
                    // use target to remove 'imp' from output
                    target: "conduit_proxy::signal",
                    "received Ctrl-C, starting shutdown",
                );
            })
            .map_err(|_| unreachable!("ctrl_c never returns errors"));

        Box::new(on_ctrl_c)
    }
}
