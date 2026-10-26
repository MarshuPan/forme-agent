//! Bounded I/O for a native pipe opened in nonblocking mode.
#![cfg(windows)]

use std::{
    fs::File,
    io::{self, Read, Write},
    thread,
    time::{Duration, Instant},
};

pub(super) struct DeadlinePipe {
    file: File,
    deadline: Instant,
}

impl DeadlinePipe {
    pub(super) fn new(file: File, timeout: Duration) -> Self {
        Self {
            file,
            deadline: Instant::now() + timeout,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "worker pipe deadline exceeded"))
    }

    fn pause(&self) -> io::Result<()> {
        thread::sleep(self.remaining()?.min(Duration::from_millis(5)));
        Ok(())
    }
}

impl Read for DeadlinePipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            self.remaining()?;
            let available =
                match super::windows_agent_execution_native::agent_execution_pipe_available(
                    &self.file,
                )? {
                    None => return Ok(0),
                    Some(0) => {
                        self.pause()?;
                        continue;
                    }
                    Some(available) => available as usize,
                };
            let length = bytes.len().min(available);
            match self.file.read(&mut bytes[..length]) {
                Ok(0) => self.pause()?,
                Err(error) if error.raw_os_error() == Some(232) => self.pause()?, // ERROR_NO_DATA
                Err(error) if error.raw_os_error() == Some(109) => return Ok(0), // ERROR_BROKEN_PIPE
                result => return result,
            }
        }
    }
}

impl Write for DeadlinePipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            self.remaining()?;
            match self.file.write(bytes) {
                Ok(0) => self.pause()?,
                Err(error) if error.raw_os_error() == Some(232) => self.pause()?,
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // File::write is unbuffered. FlushFileBuffers would wait for the peer
        // to consume the pipe and would escape the request deadline.
        self.remaining().map(|_| ())
    }
}
