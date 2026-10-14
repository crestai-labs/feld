//! `feld program-capture`, the hidden command that records a program Session's output.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context as _, Result};
use feld_core::time;
use feld_daemon::console::OutputLine;

use crate::args::ProgramCaptureArgs;

/// The output record both streams append to. After the first write error it keeps the error and
/// records nothing more, while the streams are still read to EOF so the program never sees a
/// closed pipe.
struct Recorder<W> {
    output: W,
    sequence: u64,
    error: Option<anyhow::Error>,
}

impl<W: Write> Recorder<W> {
    fn record(&mut self, stream: &str, text: String) {
        if self.error.is_some() {
            return;
        }
        self.sequence += 1;
        let line = OutputLine {
            sequence: self.sequence,
            stream: stream.to_owned(),
            timestamp: time::now(),
            text,
        };
        if let Err(error) = self.write(&line) {
            self.error = Some(error);
        }
    }

    fn write(&mut self, line: &OutputLine) -> Result<()> {
        serde_json::to_writer(&mut self.output, line).context("encode program output")?;
        self.output
            .write_all(b"\n")
            .context("write program output")?;
        self.output.flush().context("flush program output")
    }
}

pub fn run(args: ProgramCaptureArgs) -> Result<()> {
    let output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args.output)
        .with_context(|| format!("open output record {}", args.output.display()))?;
    let recorder = Arc::new(Mutex::new(Recorder {
        output,
        sequence: 0,
        error: None,
    }));
    let stdout = capture_stream(args.stdout_fifo.clone(), "stdout", Arc::clone(&recorder));
    let stderr = capture_stream(args.stderr_fifo.clone(), "stderr", Arc::clone(&recorder));
    let stdout = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("stdout capture thread panicked"));
    let stderr = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("stderr capture thread panicked"));
    stdout??;
    stderr??;
    if let Some(error) = recorder
        .lock()
        .expect("output capture mutex poisoned")
        .error
        .take()
    {
        return Err(error);
    }
    let _ = fs::remove_file(args.stdout_fifo);
    let _ = fs::remove_file(args.stderr_fifo);
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut done = options
        .open(&args.done)
        .with_context(|| format!("write capture marker {}", args.done.display()))?;
    done.write_all(b"complete\n")?;
    done.sync_all()?;
    Ok(())
}

fn capture_stream<W: Write + Send + 'static>(
    fifo: PathBuf,
    stream: &'static str,
    recorder: Arc<Mutex<Recorder<W>>>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let input = File::open(&fifo).with_context(|| format!("open {} FIFO", fifo.display()))?;
        let mut reader = BufReader::new(input);
        loop {
            let mut bytes = Vec::new();
            let read = reader
                .read_until(b'\n', &mut bytes)
                .context("read program output")?;
            if read == 0 {
                break;
            }
            if bytes.ends_with(b"\n") {
                bytes.pop();
                if bytes.ends_with(b"\r") {
                    bytes.pop();
                }
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            recorder
                .lock()
                .expect("output capture mutex poisoned")
                .record(stream, text);
        }
        Ok(())
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::io;
    use std::process::Command;

    use super::*;

    /// Holds `lines` lines, then fails every write as a full disk does.
    struct FullDisk {
        written: Vec<u8>,
        lines: usize,
    }

    impl Write for FullDisk {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.written.iter().filter(|byte| **byte == b'\n').count() == self.lines {
                return Err(io::Error::other("no space left on device"));
            }
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_record_keeps_reading_the_fifo_to_eof() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("stdout");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let recorder = Arc::new(Mutex::new(Recorder {
            output: FullDisk {
                written: Vec::new(),
                lines: 2,
            },
            sequence: 0,
            error: None,
        }));
        let capture = capture_stream(fifo.clone(), "stdout", Arc::clone(&recorder));

        // Far more than a pipe buffer holds, so the program blocks or gets EPIPE unless the
        // capture keeps reading after the record fails.
        let mut program = OpenOptions::new().write(true).open(&fifo).unwrap();
        for index in 0..20_000 {
            writeln!(program, "line {index}").unwrap();
        }
        drop(program);

        capture.join().unwrap().unwrap();
        let recorder = recorder.lock().unwrap();
        let written = String::from_utf8(recorder.output.written.clone()).unwrap();
        assert_eq!(written.lines().count(), 2);
        assert!(written.contains("\"line 1\""));
        assert!(recorder.error.is_some());
    }
}
