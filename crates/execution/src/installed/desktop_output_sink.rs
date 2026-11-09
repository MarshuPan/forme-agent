//! Host-owned assembly of one immutable screenshot transfer.
use super::desktop_output_storage::OutputStage;
use forme_protocol as p;
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
#[cfg(test)]
#[path = "desktop_output_sink_tests.rs"]
mod tests;

/// Metadata-only contract. Raw pixels cannot be formatted or serialized here.
pub trait DesktopOutputSink {
    fn transfer(&self) -> &p::DesktopOutputTransfer;
    fn begin(&mut self, header: p::DesktopOutputHeader) -> p::Result<()>;
    fn write_chunk(&mut self, header: p::DesktopOutputChunkHeader, bytes: &[u8]) -> p::Result<()>;
    fn finish(&mut self, finish: p::DesktopOutputFinish) -> p::Result<()>;
}

pub(super) struct DesktopOutputAssembler {
    transfer: p::DesktopOutputTransfer,
    request: p::DesktopActionRequest,
    stage: Option<OutputStage>,
    header: Option<p::DesktopOutputHeader>,
    finish: Option<p::DesktopOutputFinish>,
    offset: u64,
    chunks: usize,
    poisoned: bool,
}
impl DesktopOutputAssembler {
    pub(super) fn create(
        root: &Path,
        transfer: p::DesktopOutputTransfer,
        request: p::DesktopActionRequest,
    ) -> p::Result<Self> {
        transfer.validate()?;
        transfer.binding.validate_request(&request)?;
        if now() >= transfer.binding.deadline {
            return Err(invalid());
        }
        let stage = OutputStage::create(root, &transfer)?;
        Ok(Self {
            transfer,
            request,
            stage: Some(stage),
            header: None,
            finish: None,
            offset: 0,
            chunks: 0,
            poisoned: false,
        })
    }
    fn check_open(&self) -> p::Result<()> {
        if self.poisoned || self.finish.is_some() || now() >= self.transfer.binding.deadline {
            return Err(invalid());
        }
        Ok(())
    }
    fn protect<T>(&mut self, operation: impl FnOnce(&mut Self) -> p::Result<T>) -> p::Result<T> {
        let result = self.check_open().and_then(|()| operation(self));
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn read_original(&mut self) -> p::Result<Vec<u8>> {
        let file = &mut self.stage.as_mut().ok_or_else(invalid)?.image;
        if file.metadata().map_err(|_| invalid())?.len() != self.offset {
            return Err(invalid());
        }
        file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let mut bytes = Vec::new();
        file.take(self.transfer.output_budget_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != self.offset {
            return Err(invalid());
        }
        Ok(bytes)
    }
    pub(super) fn verified_bytes(&mut self) -> p::Result<Vec<u8>> {
        if self.poisoned
            || self.finish.is_none()
            || self.stage.is_none()
            || now() >= self.transfer.binding.deadline
        {
            return Err(invalid());
        }
        let bytes = self.read_original()?;
        let header = self.header.as_ref().ok_or_else(invalid)?;
        if p::sha256_content_digest(&bytes) != header.content_digest {
            return Err(invalid());
        }
        Ok(bytes)
    }
    pub(super) fn header(&self) -> p::Result<&p::DesktopOutputHeader> {
        if self.poisoned || self.finish.is_none() {
            return Err(invalid());
        }
        self.header.as_ref().ok_or_else(invalid)
    }
    pub(super) fn receipt(&self) -> p::Result<&p::DesktopActionReceipt> {
        if self.poisoned {
            return Err(invalid());
        }
        self.finish
            .as_ref()
            .map(|value| &value.receipt)
            .ok_or_else(invalid)
    }
    /// Call only after the original manifest and Host descriptor are durable.
    pub(super) fn result_sealed(&mut self) -> p::Result<()> {
        if self.poisoned || self.finish.is_none() {
            return Err(invalid());
        }
        self.poisoned = true;
        self.stage.take().ok_or_else(invalid)?.sealed()
    }
}
impl DesktopOutputSink for DesktopOutputAssembler {
    fn transfer(&self) -> &p::DesktopOutputTransfer {
        &self.transfer
    }
    fn begin(&mut self, header: p::DesktopOutputHeader) -> p::Result<()> {
        self.protect(|this| {
            if this.header.is_some() {
                return Err(invalid());
            }
            header.validate_for(&this.transfer)?;
            this.header = Some(header);
            Ok(())
        })
    }
    fn write_chunk(&mut self, header: p::DesktopOutputChunkHeader, bytes: &[u8]) -> p::Result<()> {
        self.protect(|this| {
            if this.chunks >= p::DESKTOP_OUTPUT_MAX_CHUNKS {
                return Err(invalid());
            }
            header.validate_for(
                this.header.as_ref().ok_or_else(invalid)?,
                this.offset,
                bytes,
            )?;
            this.stage
                .as_mut()
                .ok_or_else(invalid)?
                .image
                .write_all(bytes)
                .map_err(|_| invalid())?;
            this.offset = header.next_offset;
            this.chunks += 1;
            Ok(())
        })
    }
    fn finish(&mut self, finish: p::DesktopOutputFinish) -> p::Result<()> {
        self.protect(|this| {
            let header = this.header.clone().ok_or_else(invalid)?;
            finish.validate_for(&header, &this.request)?;
            if this.offset != header.bytes || this.chunks == 0 {
                return Err(invalid());
            }
            this.stage
                .as_ref()
                .ok_or_else(invalid)?
                .image
                .sync_all()
                .map_err(|_| invalid())?;
            let bytes = this.read_original()?;
            if p::sha256_content_digest(&bytes) != header.content_digest {
                return Err(invalid());
            }
            crate::validate_desktop_png(&bytes, header.width, header.height)?;
            if now() >= this.transfer.binding.deadline {
                return Err(invalid());
            }
            this.stage
                .as_mut()
                .ok_or_else(invalid)?
                .finished(&(header, &finish))?;
            this.finish = Some(finish);
            Ok(())
        })
    }
}
fn now() -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        .unwrap_or(i64::MAX)
}
fn invalid() -> p::Error {
    p::Error("desktop output transfer could not be verified".into())
}
