use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::models::AgentCoreDumpManifest;

const CORE_DUMP_EXTENSION: &str = "agentcore.zst";

pub(crate) struct PersistedCoreDumpArtifact {
    pub(crate) path: PathBuf,
    pub(crate) bytes: usize,
    pub(crate) sha256: String,
}

pub(crate) fn persist_manifest(
    dump_dir: &Path,
    dump_id: &str,
    manifest: &AgentCoreDumpManifest,
) -> Result<PersistedCoreDumpArtifact, String> {
    let json = serde_json::to_vec_pretty(manifest)
        .map_err(|err| format!("serialize core dump manifest: {err}"))?;
    let compressed = zstd::stream::encode_all(json.as_slice(), 3)
        .map_err(|err| format!("compress core dump manifest: {err}"))?;

    fs::create_dir_all(dump_dir)
        .map_err(|err| format!("create core dump directory '{}': {err}", dump_dir.display()))?;

    let artifact_path = dump_dir.join(format!("{dump_id}.{CORE_DUMP_EXTENSION}"));
    let tmp_path = artifact_path.with_extension("tmp");
    fs::write(&tmp_path, &compressed)
        .map_err(|err| format!("write core dump '{}': {err}", artifact_path.display()))?;
    fs::rename(&tmp_path, &artifact_path).map_err(|err| {
        let _ = fs::remove_file(&tmp_path);
        format!(
            "finalize core dump artifact '{}': {err}",
            artifact_path.display()
        )
    })?;

    let sha256 = hex_sha256(&compressed);
    Ok(PersistedCoreDumpArtifact {
        path: artifact_path,
        bytes: compressed.len(),
        sha256,
    })
}

pub(crate) fn load_manifest_json(path: &Path) -> Result<String, String> {
    let compressed = fs::read(path)
        .map_err(|err| format!("read core dump artifact '{}': {err}", path.display()))?;
    let json = zstd::stream::decode_all(compressed.as_slice())
        .map_err(|err| format!("decompress core dump artifact '{}': {err}", path.display()))?;
    String::from_utf8(json)
        .map_err(|err| format!("decode core dump artifact '{}': {err}", path.display()))
}

pub(crate) fn load_manifest(path: &Path) -> Result<AgentCoreDumpManifest, String> {
    let json = load_manifest_json(path)?;
    serde_json::from_str(&json)
        .map_err(|err| format!("parse core dump manifest '{}': {err}", path.display()))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}
