use crate::sfx::BundleMetadata;
use flate2::read::GzDecoder;
use std::{
    fs,
    io::{self, Cursor},
    path::Path,
};
use tar::Archive;

pub fn extract_to(source: &Path, bundle: &BundleMetadata, destination: &Path) -> io::Result<()> {
    let bytes = fs::read(source)?;
    let payload_start = bundle.payload_offset as usize;
    let payload_end = payload_start
        .checked_add(bundle.payload_len as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "payload length overflowed"))?;
    let payload = bytes.get(payload_start..payload_end).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "embedded payload did not fit inside the packed executable",
        )
    })?;

    let cursor = Cursor::new(payload);
    let gz = GzDecoder::new(cursor);
    let mut archive = Archive::new(gz);
    archive.unpack(destination)
}
