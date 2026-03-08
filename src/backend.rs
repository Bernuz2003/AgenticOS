use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_llama;
use candle_transformers::models::quantized_qwen2;

use crate::prompting::PromptFamily;

pub struct DriverDescriptor {
    pub id: &'static str,
    pub kind: &'static str,
    pub available: bool,
    pub load_supported: bool,
    pub note: &'static str,
    families: &'static [PromptFamily],
    architectures: &'static [&'static str],
}

impl DriverDescriptor {
    fn supports_family(&self, family: PromptFamily) -> bool {
        self.families.contains(&family)
    }

    fn supports_architecture(&self, architecture: Option<&str>) -> bool {
        architecture.is_none()
            || self.architectures.is_empty()
            || self
                .architectures
                .iter()
                .any(|candidate| architecture.is_some_and(|arch| candidate.eq_ignore_ascii_case(arch)))
    }

    fn supports_model(&self, family: PromptFamily, architecture: Option<&str>) -> bool {
        self.supports_family(family) && self.supports_architecture(architecture)
    }
}

#[derive(Debug, Clone)]
pub struct DriverResolution {
    pub resolved_backend_id: String,
    pub resolution_source: &'static str,
    pub resolution_rationale: String,
    pub available: bool,
    pub load_supported: bool,
}

const FAMILIES_LLAMA: [PromptFamily; 1] = [PromptFamily::Llama];
const FAMILIES_QWEN: [PromptFamily; 1] = [PromptFamily::Qwen];
const FAMILIES_COMMON: [PromptFamily; 3] = [PromptFamily::Llama, PromptFamily::Qwen, PromptFamily::Mistral];
const ARCH_LLAMA: [&str; 1] = ["llama"];
const ARCH_QWEN2: [&str; 1] = ["qwen2"];
const ARCH_ANY: [&str; 0] = [];

const DRIVER_REGISTRY: [DriverDescriptor; 3] = [
    DriverDescriptor {
        id: "candle.quantized_llama",
        kind: "internal",
        available: true,
        load_supported: true,
        note: "Built-in Candle quantized Llama backend.",
        families: &FAMILIES_LLAMA,
        architectures: &ARCH_LLAMA,
    },
    DriverDescriptor {
        id: "candle.quantized_qwen2",
        kind: "internal",
        available: true,
        load_supported: true,
        note: "Built-in Candle quantized Qwen2 backend.",
        families: &FAMILIES_QWEN,
        architectures: &ARCH_QWEN2,
    },
    DriverDescriptor {
        id: "external-llamacpp",
        kind: "external-stub",
        available: false,
        load_supported: false,
        note: "Reserved external driver slot for future llama.cpp integration.",
        families: &FAMILIES_COMMON,
        architectures: &ARCH_ANY,
    },
];

pub fn driver_registry() -> &'static [DriverDescriptor] {
    &DRIVER_REGISTRY
}

pub fn resolve_driver_for_family(
    family: PromptFamily,
    backend_preference: Option<&str>,
) -> std::result::Result<DriverResolution, String> {
    resolve_driver_for_model(family, None, backend_preference)
}

pub fn resolve_driver_for_model(
    family: PromptFamily,
    architecture: Option<&str>,
    backend_preference: Option<&str>,
) -> std::result::Result<DriverResolution, String> {
    if matches!(family, PromptFamily::Unknown) {
        return Err("Cannot resolve driver for unknown model family.".to_string());
    }

    let fallback = DRIVER_REGISTRY
        .iter()
        .find(|driver| {
            driver.supports_model(family, architecture)
                && driver.available
                && driver.load_supported
        });

    let architecture_label = architecture
        .map(|value| format!(" architecture '{}'", value))
        .unwrap_or_default();

    if let Some(preferred_id) = backend_preference {
        if let Some(driver) = DRIVER_REGISTRY.iter().find(|item| item.id == preferred_id) {
            if !driver.supports_family(family) {
                return Err(format!(
                    "Preferred driver '{}' does not support family {:?}.",
                    preferred_id, family
                ));
            }

            if !driver.supports_architecture(architecture) {
                return Err(format!(
                    "Preferred driver '{}' does not support{:?} for family {:?}.",
                    preferred_id, architecture, family
                ));
            }

            if driver.available && driver.load_supported {
                return Ok(DriverResolution {
                    resolved_backend_id: driver.id.to_string(),
                    resolution_source: "metadata-preference",
                    resolution_rationale: format!(
                        "using preferred driver '{}' declared by model metadata for family {:?}{}",
                        preferred_id, family, architecture_label
                    ),
                    available: driver.available,
                    load_supported: driver.load_supported,
                });
            }

            if let Some(fallback_driver) = fallback {
                return Ok(DriverResolution {
                    resolved_backend_id: fallback_driver.id.to_string(),
                    resolution_source: "metadata-preference-fallback",
                    resolution_rationale: format!(
                        "preferred driver '{}' is registered but not loadable yet for family {:?}{}; falling back to '{}': {}",
                        preferred_id, family, architecture_label, fallback_driver.id, driver.note
                    ),
                    available: fallback_driver.available,
                    load_supported: fallback_driver.load_supported,
                });
            }

            return Err(format!(
                "Preferred driver '{}' is registered but not loadable, and no compatible fallback is available for family {:?}{}.",
                preferred_id, family, architecture_label
            ));
        }

        if let Some(fallback_driver) = fallback {
            return Ok(DriverResolution {
                resolved_backend_id: fallback_driver.id.to_string(),
                resolution_source: "metadata-preference-unknown-fallback",
                resolution_rationale: format!(
                    "preferred driver '{}' is unknown; falling back to '{}' for family {:?}{}",
                    preferred_id, fallback_driver.id, family, architecture_label
                ),
                available: fallback_driver.available,
                load_supported: fallback_driver.load_supported,
            });
        }

        return Err(format!(
            "Preferred driver '{}' is unknown and no compatible fallback is available for family {:?}{}.",
            preferred_id, family, architecture_label
        ));
    }

    if let Some(driver) = fallback {
        return Ok(DriverResolution {
            resolved_backend_id: driver.id.to_string(),
            resolution_source: "family-default",
            resolution_rationale: format!(
                "using default loadable driver '{}' for family {:?}{}",
                driver.id, family, architecture_label
            ),
            available: driver.available,
            load_supported: driver.load_supported,
        });
    }

    Err(format!(
        "No registered loadable driver can satisfy family {:?}{}.",
        family, architecture_label
    ))
}

pub trait ModelBackend: Send {
    fn backend_id(&self) -> &'static str;
    fn family(&self) -> PromptFamily;
    fn forward(&mut self, input_tensor: &Tensor, position: usize) -> Result<Tensor>;
    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>>;
}

pub struct RuntimeModel {
    inner: Box<dyn ModelBackend>,
}

impl RuntimeModel {
    pub fn load_from_gguf(
        path: &str,
        family: PromptFamily,
        backend_id: &str,
        device: &Device,
    ) -> Result<Self> {
        let descriptor = driver_registry()
            .iter()
            .find(|driver| driver.id == backend_id)
            .ok_or_else(|| E::msg(format!("Unknown backend id '{}'.", backend_id)))?;

        if !descriptor.supports_family(family) {
            return Err(E::msg(format!(
                "Backend '{}' does not support family {:?}.",
                backend_id, family
            )));
        }

        if !descriptor.available || !descriptor.load_supported {
            return Err(E::msg(format!(
                "Backend '{}' is registered as '{}' but is not loadable in-process yet: {}",
                backend_id, descriptor.kind, descriptor.note
            )));
        }

        let backend: Box<dyn ModelBackend> = match backend_id {
            "candle.quantized_llama" => Box::new(QuantizedLlamaBackend::load(path, device)?),
            "candle.quantized_qwen2" => Box::new(QuantizedQwen2Backend::load(path, device)?),
            _ => {
                return Err(E::msg(format!(
                    "Backend '{}' is registered but has no in-process loader implementation.",
                    backend_id
                )))
            }
        };

        Ok(Self { inner: backend })
    }

    pub fn backend_id(&self) -> &'static str {
        self.inner.backend_id()
    }

    pub fn family(&self) -> PromptFamily {
        self.inner.family()
    }

    pub fn forward(&mut self, input_tensor: &Tensor, position: usize) -> Result<Tensor> {
        self.inner.forward(input_tensor, position)
    }

    /// Clone the model weights for a new process (zero-copy for backends that support it).
    ///
    /// Returns `None` for non-cloneable backends. The caller must enforce any
    /// single-process guard required by the selected backend.
    pub fn duplicate_if_supported(&self) -> Option<Self> {
        self.inner
            .duplicate_boxed()
            .map(|inner| Self { inner })
    }
}

struct QuantizedLlamaBackend {
    weights: quantized_llama::ModelWeights,
}

impl QuantizedLlamaBackend {
    fn load(path: &str, device: &Device) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| E::msg(format!("Failed to open model file: {}", e)))?;
        let content = gguf_file::Content::read(&mut file)?;
        let weights = quantized_llama::ModelWeights::from_gguf(content, &mut file, device)?;
        Ok(Self { weights })
    }
}

impl ModelBackend for QuantizedLlamaBackend {
    fn backend_id(&self) -> &'static str {
        "candle.quantized_llama"
    }

    fn family(&self) -> PromptFamily {
        PromptFamily::Llama
    }

    fn forward(&mut self, input_tensor: &Tensor, position: usize) -> Result<Tensor> {
        Ok(self.weights.forward(input_tensor, position)?)
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>> {
        Some(Box::new(Self {
            weights: self.weights.clone(),
        }))
    }
}

struct QuantizedQwen2Backend {
    weights: quantized_qwen2::ModelWeights,
}

impl QuantizedQwen2Backend {
    fn load(path: &str, device: &Device) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| E::msg(format!("Failed to open model file: {}", e)))?;
        let content = gguf_file::Content::read(&mut file)?;

        match quantized_qwen2::ModelWeights::from_gguf(content, &mut file, device) {
            Ok(weights) => Ok(Self { weights }),
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("cannot find tensor info for output_norm.weight") {
                    Err(E::msg(
                        "Qwen load failed: missing 'output_norm.weight'. The GGUF is likely an incomplete split shard (or otherwise incomplete export). Use a full single-file GGUF, or merge all split parts before LOAD.",
                    ))
                } else {
                    Err(E::msg(msg))
                }
            }
        }
    }
}

impl ModelBackend for QuantizedQwen2Backend {
    fn backend_id(&self) -> &'static str {
        "candle.quantized_qwen2"
    }

    fn family(&self) -> PromptFamily {
        PromptFamily::Qwen
    }

    fn forward(&mut self, input_tensor: &Tensor, position: usize) -> Result<Tensor> {
        Ok(self.weights.forward(input_tensor, position)?)
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_driver_for_family, resolve_driver_for_model, PromptFamily};

    #[test]
    fn resolves_family_default_driver() {
        let resolution =
            resolve_driver_for_family(PromptFamily::Llama, None).expect("resolve llama driver");
        assert_eq!(resolution.resolved_backend_id, "candle.quantized_llama");
        assert_eq!(resolution.resolution_source, "family-default");
    }

    #[test]
    fn preferred_external_driver_falls_back_when_stub_only() {
        let resolution = resolve_driver_for_family(
            PromptFamily::Qwen,
            Some("external-llamacpp"),
        )
        .expect("resolve qwen fallback driver");
        assert_eq!(resolution.resolved_backend_id, "candle.quantized_qwen2");
        assert_eq!(resolution.resolution_source, "metadata-preference-fallback");
    }

    #[test]
    fn unsupported_family_without_loadable_driver_errors() {
        let err = resolve_driver_for_family(PromptFamily::Mistral, None)
            .expect_err("mistral should not resolve to loadable driver yet");
        assert!(err.contains("No registered loadable driver"));
    }

    #[test]
    fn architecture_specific_driver_resolution_rejects_qwen35_for_qwen2_backend() {
        let err = resolve_driver_for_model(PromptFamily::Qwen, Some("qwen35"), None)
            .expect_err("qwen35 should not resolve to qwen2 backend");
        assert!(err.contains("qwen35"));
    }
}
