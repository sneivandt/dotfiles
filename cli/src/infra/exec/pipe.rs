//! Interruptible readers for Unix child output pipes.

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::{PollFd, PollFlags, poll};
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::sync::mpsc::Receiver;

use crate::infra::atomic_flag::AtomicFlag;

use super::POLL_INTERVAL;

#[derive(Debug, Default)]
pub(super) struct ReaderShutdown(AtomicFlag);

impl ReaderShutdown {
    pub(super) fn reader<R: Read + AsFd>(&self, pipe: R) -> OutputPipe<R> {
        let pipe = (|| {
            let flags = OFlag::from_bits_retain(fcntl(&pipe, FcntlArg::F_GETFL)?);
            fcntl(&pipe, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
            Ok(pipe)
        })();
        OutputPipe {
            pipe,
            stop: self.0.clone(),
        }
    }

    pub(super) fn finish(&self, readers: &Receiver<()>) {
        // Give the readers a chance to drain final diagnostics after killing the
        // process group. A writer that escaped the group can keep a pipe open.
        let _drain = readers.recv_timeout(POLL_INTERVAL);
        self.0.set();
    }
}

impl Drop for ReaderShutdown {
    fn drop(&mut self) {
        self.0.set();
    }
}

pub(super) struct OutputPipe<R> {
    pipe: nix::Result<R>,
    stop: AtomicFlag,
}

impl<R: Read + AsFd> Read for OutputPipe<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let pipe = self.pipe.as_mut().map_err(|error| *error)?;
        let timeout = u16::try_from(POLL_INTERVAL.as_millis()).unwrap_or(u16::MAX);
        while !self.stop.get() {
            match pipe.read(buffer) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    poll(&mut [PollFd::new(pipe.as_fd(), PollFlags::POLLIN)], timeout)?;
                }
                result => return result,
            }
        }
        Ok(0)
    }
}
