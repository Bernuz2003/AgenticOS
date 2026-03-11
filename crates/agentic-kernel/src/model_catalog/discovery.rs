use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::errors::CatalogError;

use super::metadata::{
    describe_metadata_source, infer_family_from_filename, infer_metadata_path,
    infer_tokenizer_path, load_model_metadata, load_native_model_metadata, merge_model_metadata,
};
use super::{ModelEntry, ModelMetadata};

pub(super) fn discover_entries(models_dir: &Path) -> Result<Vec<ModelEntry>, CatalogError> {
    let mut entries = Vec::new();
    let mut gguf_files = Vec::new();

    collect_gguf_files(models_dir, &mut gguf_files)?;
    for path in gguf_files {
        entries.push(build_entry(models_dir, path));
    }

    entries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(entries)
}

pub(super) fn build_entry(models_dir: &Path, path: PathBuf) -> ModelEntry {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown-model")
        .to_string();

    let tokenizer_path = infer_tokenizer_path(models_dir, &path);
    let (native_metadata, native_from_gguf, native_from_tokenizer) =
        load_native_model_metadata(&path, tokenizer_path.as_deref());
    let metadata_path = infer_metadata_path(&path);
    let sidecar_metadata = metadata_path
        .as_ref()
        .and_then(|meta_path| load_model_metadata(meta_path));
    let metadata = merge_model_metadata(native_metadata, sidecar_metadata);
    let family = resolve_entry_family(&stem, metadata.as_ref());
    let metadata_source = describe_metadata_source(
        native_from_gguf,
        native_from_tokenizer,
        metadata_path.as_deref(),
        metadata.as_ref(),
    );

    ModelEntry {
        id: build_model_id(models_dir, &path),
        path,
        family,
        tokenizer_path,
        metadata_source,
        metadata,
    }
}

pub(super) fn compute_catalog_fingerprint(models_dir: &Path) -> Result<u64, CatalogError> {
    let mut files = Vec::new();
    collect_catalog_signature_files(models_dir, &mut files)?;
    files.sort();

    let mut hasher = DefaultHasher::new();
    for path in files {
        let metadata = fs::metadata(&path).map_err(|err| CatalogError::DirectoryReadFailed {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
        let relative = path.strip_prefix(models_dir).unwrap_or(path.as_path());
        relative.to_string_lossy().hash(&mut hasher);
        metadata.len().hash(&mut hasher);
        match metadata.modified() {
            Ok(modified) => {
                if let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) {
                    since_epoch.as_secs().hash(&mut hasher);
                    since_epoch.subsec_nanos().hash(&mut hasher);
                }
            }
            Err(_) => 0_u8.hash(&mut hasher),
        }
    }

    Ok(hasher.finish())
}

fn resolve_entry_family(
    stem: &str,
    metadata: Option<&ModelMetadata>,
) -> crate::prompting::PromptFamily {
    metadata
        .and_then(ModelMetadata::declared_family)
        .unwrap_or_else(|| infer_family_from_filename(stem))
}

fn collect_catalog_signature_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CatalogError> {
    let entries = fs::read_dir(dir).map_err(|err| CatalogError::DirectoryReadFailed {
        path: dir.display().to_string(),
        detail: err.to_string(),
    })?;

    for entry in entries {
        let path = entry
            .map_err(|err| CatalogError::DirectoryReadFailed {
                path: dir.display().to_string(),
                detail: err.to_string(),
            })?
            .path();

        if path.is_dir() {
            collect_catalog_signature_files(&path, out)?;
            continue;
        }

        if path.is_file() && is_catalog_relevant_file(&path) {
            out.push(path);
        }
    }

    Ok(())
}

fn collect_gguf_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CatalogError> {
    let entries = fs::read_dir(dir).map_err(|err| CatalogError::DirectoryReadFailed {
        path: dir.display().to_string(),
        detail: err.to_string(),
    })?;

    for entry in entries {
        let path = entry
            .map_err(|err| CatalogError::DirectoryReadFailed {
                path: dir.display().to_string(),
                detail: err.to_string(),
            })?
            .path();

        if path.is_dir() {
            collect_gguf_files(&path, out)?;
            continue;
        }

        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .eq_ignore_ascii_case("gguf")
        {
            out.push(path);
        }
    }

    Ok(())
}

fn is_catalog_relevant_file(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();

    extension.eq_ignore_ascii_case("gguf")
        || file_name.eq_ignore_ascii_case("tokenizer.json")
        || file_name.eq_ignore_ascii_case("metadata.json")
        || file_name.ends_with(".metadata.json")
}

fn build_model_id(models_dir: &Path, model_path: &Path) -> String {
    let relative = model_path
        .strip_prefix(models_dir)
        .unwrap_or(model_path)
        .to_path_buf();
    let mut without_ext = relative;
    without_ext.set_extension("");
    without_ext.to_string_lossy().replace('\\', "/")
}
