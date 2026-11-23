use notify::{RecursiveMode, Watcher};
use std::cell::RefCell;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

enum PublicationEvent {
    FilesChanged(notify::Result<()>),
    ProducerEnded(io::Result<()>),
}

/// The CLI publishes its descriptors by rename. A producer's exit and a file
/// notification both wake the reader; the actual file is authoritative.
pub(super) struct PublishedFiles {
    root: PathBuf,
    events: Receiver<PublicationEvent>,
    signal: Sender<PublicationEvent>,
    producer_end: RefCell<Option<io::Result<()>>>,
}

impl PublishedFiles {
    pub(super) fn new(root: &Path) -> Self {
        let (signal, events) = mpsc::channel();
        Self {
            root: root.to_owned(),
            events,
            signal,
            producer_end: RefCell::new(None),
        }
    }

    pub(super) fn on_exit(&self) -> impl FnOnce(Option<&io::Error>) + Send + 'static {
        let signal = self.signal.clone();
        move |error| {
            let result = error.map_or(Ok(()), |e| Err(io::Error::new(e.kind(), e.to_string())));
            let _ = signal.send(PublicationEvent::ProducerEnded(result));
        }
    }

    /// A plain child owns the write end of this pipe for its lifetime.
    pub(super) fn observe_pipe<W: Write + Send + 'static>(
        &self,
        mut pipe: impl Read + Send + 'static,
        mut destination: W,
    ) -> std::thread::JoinHandle<io::Result<W>> {
        let closed = self.on_exit();
        std::thread::spawn(move || {
            let result = io::copy(&mut pipe, &mut destination);
            closed(result.as_ref().err());
            result.map(|_| destination)
        })
    }

    pub(super) fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let published = || match fs::read(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => match &*self
                .producer_end
                .borrow()
            {
                None => Ok(None),
                Some(Ok(())) => Err(error),
                Some(Err(observer)) => Err(io::Error::new(observer.kind(), observer.to_string())),
            },
            result => result.map(Some),
        };
        if let Some(bytes) = published()? {
            return Ok(bytes);
        }
        let changed = self.signal.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if !matches!(&event, Ok(event) if event.kind.is_access()) {
                    let _ = changed.send(PublicationEvent::FilesChanged(event.map(|_| ())));
                }
            })
            .map_err(io::Error::other)?;
        let mut watching = None;
        loop {
            if let Some(bytes) = published()? {
                return Ok(bytes);
            }
            // A producer may create the parent and rename the file before a
            // recursive watcher installs its child watch. Watch the closest
            // existing directory directly and recheck after every registration.
            let directory = path
                .ancestors()
                .skip(1)
                .take_while(|directory| directory.starts_with(&self.root))
                .find(|directory| directory.is_dir())
                .ok_or_else(|| io::Error::other("fixture directory disappeared"))?;
            if watching.as_deref() != Some(directory) {
                watcher
                    .watch(directory, RecursiveMode::NonRecursive)
                    .map_err(io::Error::other)?;
                watching = Some(directory.to_owned());
                continue;
            }
            match self.events.recv().map_err(io::Error::other)? {
                PublicationEvent::FilesChanged(result) => {
                    result.map_err(io::Error::other)?;
                }
                // Recheck the last rename even if the exit notification arrived first.
                PublicationEvent::ProducerEnded(result) => {
                    self.producer_end.replace(Some(result));
                }
            }
        }
    }
}
