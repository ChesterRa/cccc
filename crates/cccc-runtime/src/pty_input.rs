//! Input serialization is separate from Session lifecycle synchronization.
use crate::RuntimeError;
use crate::cancellation::lock_interruptibly;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

pub(crate) trait PtyInput: Write + Send {
    fn write_all_interruptible(
        &mut self,
        mut data: &[u8],
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<bool> {
        while !data.is_empty() {
            if cancelled() {
                return Ok(false);
            }
            match self.write(data) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => data = &data[written..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        self.flush()?;
        Ok(true)
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) struct NativeInput(pub(crate) Box<dyn Write + Send>);
#[cfg(not(target_os = "linux"))]
impl Write for NativeInput {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.write(data)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
#[cfg(not(target_os = "linux"))]
impl PtyInput for NativeInput {}

pub(crate) type SharedPtyWriter = Arc<Mutex<Box<dyn PtyInput>>>;

pub(crate) fn write_input(writer: &SharedPtyWriter, data: &[u8]) -> Result<(), RuntimeError> {
    let mut writer = writer.lock().map_err(|_| RuntimeError::Poisoned)?;
    writer.write_all(data)?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn write_input_interruptibly(
    writer: &SharedPtyWriter,
    data: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, RuntimeError> {
    let Some(mut writer) = lock_interruptibly(writer, cancelled)? else {
        return Ok(false);
    };
    writer
        .write_all_interruptible(data, cancelled)
        .map_err(RuntimeError::from)
}
