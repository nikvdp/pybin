use miette::{Context, IntoDiagnostic, Result, miette};
use std::{fs, path::Path};

const FOOTER_MAGIC: &[u8] = b"PYBIN_SFX_V1__\0";
const FOOTER_LEN: usize = FOOTER_MAGIC.len() + 8 + 4 + 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleMetadata {
    pub exec_relpath: String,
    pub build_uid: String,
    pub payload_offset: u64,
    pub payload_len: u64,
}

pub fn encode_metadata(exec_relpath: &str, build_uid: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(exec_relpath.len() + build_uid.len() + 1);
    bytes.extend_from_slice(exec_relpath.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(build_uid.as_bytes());
    bytes
}

pub fn footer_bytes(payload_len: u64, metadata_len: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FOOTER_LEN);
    bytes.extend_from_slice(FOOTER_MAGIC);
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes.extend_from_slice(&metadata_len.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

pub fn read_bundle(path: &Path) -> Result<BundleMetadata> {
    let bytes = fs::read(path).into_diagnostic()?;
    let footer = bytes
        .get(bytes.len().saturating_sub(FOOTER_LEN)..)
        .ok_or_else(|| miette!("packed executable was too small to contain a footer"))?;

    if footer.len() != FOOTER_LEN || &footer[..FOOTER_MAGIC.len()] != FOOTER_MAGIC {
        return Err(miette!(
            "packed executable did not contain a valid pybin footer"
        ));
    }

    let payload_len = u64::from_le_bytes(
        footer[FOOTER_MAGIC.len()..FOOTER_MAGIC.len() + 8]
            .try_into()
            .expect("payload length slice has a fixed width"),
    );
    let metadata_len = u32::from_le_bytes(
        footer[FOOTER_MAGIC.len() + 8..FOOTER_MAGIC.len() + 12]
            .try_into()
            .expect("metadata length slice has a fixed width"),
    ) as usize;

    let footer_offset = bytes.len() - FOOTER_LEN;
    let metadata_end = footer_offset;
    let metadata_start = metadata_end
        .checked_sub(metadata_len)
        .ok_or_else(|| miette!("packed footer declared an invalid metadata length"))?;
    let payload_end = metadata_start;
    let payload_start = payload_end
        .checked_sub(payload_len as usize)
        .ok_or_else(|| miette!("packed footer declared an invalid payload length"))?;
    let metadata = &bytes[metadata_start..metadata_end];

    decode_metadata(metadata, payload_start as u64, payload_len)
}

fn decode_metadata(bytes: &[u8], payload_offset: u64, payload_len: u64) -> Result<BundleMetadata> {
    let separator = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| miette!("packed metadata did not include an entrypoint separator"))?;

    let exec_relpath = String::from_utf8(bytes[..separator].to_vec())
        .into_diagnostic()
        .wrap_err("packed entrypoint was not valid utf-8")?;
    let build_uid = String::from_utf8(bytes[separator + 1..].to_vec())
        .into_diagnostic()
        .wrap_err("packed build uid was not valid utf-8")?;

    if exec_relpath.is_empty() {
        return Err(miette!("packed metadata did not include an entrypoint"));
    }

    Ok(BundleMetadata {
        exec_relpath,
        build_uid,
        payload_offset,
        payload_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip_preserves_lengths() {
        let metadata = encode_metadata("bin/tool", "abc123");
        let footer = footer_bytes(42, metadata.len() as u32);

        let mut bundle = vec![0_u8; 9];
        bundle.extend_from_slice(&[1_u8; 42]);
        bundle.extend_from_slice(&metadata);
        bundle.extend_from_slice(&footer);

        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("bundle");
        fs::write(&path, bundle).expect("write bundle");

        let parsed = read_bundle(&path).expect("read bundle");
        assert_eq!(
            parsed,
            BundleMetadata {
                exec_relpath: "bin/tool".to_string(),
                build_uid: "abc123".to_string(),
                payload_offset: 9,
                payload_len: 42,
            }
        );
    }
}
