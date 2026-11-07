//! OpenVINO inference for the exported SigLIP 2 towers.
//!
//! Devices are tried in the configured order (default `GPU,CPU`). A device is
//! selected only if both towers compile and a self-test inference succeeds on
//! it, so a missing or broken iGPU falls back to CPU at startup and the chosen
//! device is reported through [`ModelInfo::device`].

use std::borrow::Cow;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use image::DynamicImage;
use log::{info, warn};
use openvino::{
    CompiledModel, Core, DeviceType, ElementType, InferRequest, PropertyKey, RwPropertyKey, Shape,
    Tensor,
};

use crate::embedder::{EmbedError, Embedder, ModelInfo, l2_normalize};
use crate::manifest::{Manifest, TEXT_MODEL, TOKENIZER_FILE, VISION_MODEL};
use crate::preprocess::{ImageSpec, TextEncoder, image_tensor};

const PIXEL_INPUT: &str = "pixel_values";
const TEXT_INPUT: &str = "input_ids";

#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Devices to try in order, e.g. `["GPU", "CPU"]`.
    pub devices: Vec<String>,
    /// CPU inference threads; `None` lets OpenVINO decide. Set it to the
    /// container's CPU quota, which OpenVINO cannot see.
    pub cpu_threads: Option<usize>,
    /// CPU inference precision hint (`f32` or `bf16`); `None` lets OpenVINO
    /// choose (bf16 on CPUs that support it, f32 otherwise).
    pub cpu_precision: Option<String>,
    /// Optional OpenVINO model cache directory (speeds up GPU compilation
    /// after the first start; a tmpfs is enough).
    pub cache_dir: Option<String>,
}

struct Tower {
    // Keeps the compiled model alive for the request's lifetime. `CompiledModel`
    // is `Send` but not `Sync`, hence the mutex.
    _compiled: Mutex<CompiledModel>,
    request: Mutex<InferRequest>,
}

impl Tower {
    fn new(compiled: CompiledModel, request: InferRequest) -> Self {
        Self {
            _compiled: Mutex::new(compiled),
            request: Mutex::new(request),
        }
    }

    fn infer(&self, input: &str, tensor: &Tensor) -> Result<Vec<Vec<f32>>, EmbedError> {
        let inference = |error: openvino::InferenceError| EmbedError::Inference(error.to_string());
        let mut request = self
            .request
            .lock()
            .map_err(|_| EmbedError::Inference("inference request poisoned".into()))?;
        request.set_tensor(input, tensor).map_err(inference)?;
        request.infer().map_err(inference)?;
        let output = request.get_output_tensor_by_index(0).map_err(inference)?;
        let shape = output.get_shape().map_err(inference)?;
        let dims = shape.get_dimensions();
        let [batch, dim] = dims else {
            return Err(EmbedError::Inference(format!(
                "unexpected output shape {dims:?}"
            )));
        };
        let (batch, dim) = (to_usize(*batch)?, to_usize(*dim)?);
        let data = output.get_data::<f32>().map_err(inference)?;
        if data.len() != batch * dim {
            return Err(EmbedError::Inference("output size mismatch".into()));
        }
        Ok(data
            .chunks_exact(dim)
            .map(|row| l2_normalize(row.to_vec()))
            .collect())
    }
}

fn to_usize(value: i64) -> Result<usize, EmbedError> {
    usize::try_from(value).map_err(|_| EmbedError::Inference(format!("invalid dimension {value}")))
}

pub struct OpenVinoEmbedder {
    info: ModelInfo,
    spec: ImageSpec,
    text: TextEncoder,
    vision: Tower,
    text_tower: Tower,
}

impl OpenVinoEmbedder {
    pub fn load(model_dir: &Path, config: &BackendConfig) -> Result<Self> {
        let manifest = Manifest::load(model_dir)?;
        let text = TextEncoder::from_file(&model_dir.join(TOKENIZER_FILE), &manifest)?;
        let mut core = Core::new().map_err(|error| anyhow!("loading OpenVINO: {error}"))?;
        let runtime = format!("openvino {}", openvino::version().build_number);
        let available = core
            .available_devices()
            .map(|devices| {
                devices
                    .iter()
                    .map(|device| device.as_ref().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        info!("{runtime}; available devices: {available:?}");
        if let Some(threads) = config.cpu_threads {
            core.set_property(
                &DeviceType::CPU,
                &RwPropertyKey::InferenceNumThreads,
                &threads.to_string(),
            )
            .context("setting CPU inference threads")?;
        }
        if let Some(cache_dir) = &config.cache_dir {
            for device in [DeviceType::CPU, DeviceType::GPU] {
                if let Err(error) = core.set_property(&device, &RwPropertyKey::CacheDir, cache_dir)
                {
                    warn!("model cache unavailable for {device}: {error}");
                }
            }
        }
        if let Some(precision) = &config.cpu_precision {
            core.set_property(
                &DeviceType::CPU,
                &RwPropertyKey::HintInferencePrecision,
                precision,
            )
            .context("setting CPU inference precision")?;
        }
        let mut failures = Vec::new();
        for device in &config.devices {
            match Self::try_device(&mut core, model_dir, &manifest, &text, device) {
                Ok((vision, text_tower, executing)) => {
                    let info = ModelInfo {
                        model_id: manifest.model_id.clone(),
                        variant: manifest.variant.clone(),
                        revision: manifest.revision.clone(),
                        precision: manifest.precision.clone(),
                        embedding_dim: manifest.embedding_dim,
                        image_size: manifest.image_size,
                        text_max_tokens: manifest.text_max_tokens,
                        logit_scale: manifest.logit_scale,
                        logit_bias: manifest.logit_bias,
                        device: executing,
                        runtime,
                        tagger_version: env!("CARGO_PKG_VERSION").to_owned(),
                    };
                    info!("serving {} on {}", info.model_id, info.device);
                    return Ok(Self {
                        info,
                        spec: ImageSpec::from(&manifest),
                        text,
                        vision,
                        text_tower,
                    });
                }
                Err(error) => {
                    warn!("device {device} unavailable: {error:#}");
                    failures.push(format!("{device}: {error:#}"));
                }
            }
        }
        bail!("no usable inference device ({})", failures.join("; "))
    }

    fn try_device(
        core: &mut Core,
        model_dir: &Path,
        manifest: &Manifest,
        text: &TextEncoder,
        device: &str,
    ) -> Result<(Tower, Tower, String)> {
        let device_type = DeviceType::from(device);
        let (vision_model, vision_request) = compile(core, model_dir, VISION_MODEL, &device_type)?;
        let executing = vision_model
            .get_property(&PropertyKey::Other(Cow::Borrowed("EXECUTION_DEVICES")))
            .map_or_else(|_| device.to_owned(), |value| value.trim().to_owned());
        let vision = Tower::new(vision_model, vision_request);
        let (text_model, text_request) = compile(core, model_dir, TEXT_MODEL, &device_type)?;
        let text_tower = Tower::new(text_model, text_request);

        // Self-test: both towers must produce a finite vector of the right size.
        let size = i64::from(manifest.image_size);
        let pixels = Tensor::new(ElementType::F32, &Shape::new(&[1, 3, size, size])?)?;
        let image = vision.infer(PIXEL_INPUT, &pixels)?;
        let ids = text_ids_tensor(&[text.encode("a photo")?], text.max_tokens())?;
        let words = text_tower.infer(TEXT_INPUT, &ids)?;
        for output in image.iter().chain(&words) {
            if output.len() != manifest.embedding_dim
                || !output.iter().all(|value| value.is_finite())
            {
                bail!("self-test produced an invalid embedding on {device}");
            }
        }
        Ok((vision, text_tower, executing))
    }
}

fn compile(
    core: &mut Core,
    model_dir: &Path,
    name: &str,
    device: &DeviceType,
) -> Result<(CompiledModel, InferRequest)> {
    let xml = model_dir.join(format!("{name}.xml"));
    let bin = model_dir.join(format!("{name}.bin"));
    let model = core
        .read_model_from_file(&path_str(&xml)?, &path_str(&bin)?)
        .with_context(|| format!("reading {}", xml.display()))?;
    let mut compiled = core
        .compile_model(&model, device.to_owned())
        .with_context(|| format!("compiling {name} for {device}"))?;
    let request = compiled
        .create_infer_request()
        .context("creating inference request")?;
    Ok((compiled, request))
}

fn path_str(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("model path is not UTF-8: {}", path.display()))
}

fn text_ids_tensor(rows: &[Vec<i64>], max_tokens: usize) -> Result<Tensor, EmbedError> {
    let inference = |error: openvino::InferenceError| EmbedError::Inference(error.to_string());
    let dimension = |value: usize| {
        i64::try_from(value)
            .map_err(|_| EmbedError::Inference(format!("dimension {value} too large")))
    };
    let shape = Shape::new(&[dimension(rows.len())?, dimension(max_tokens)?]).map_err(inference)?;
    let mut tensor = Tensor::new(ElementType::I64, &shape).map_err(inference)?;
    let data = tensor.get_data_mut::<i64>().map_err(inference)?;
    for (target, row) in data.chunks_exact_mut(max_tokens).zip(rows) {
        target.copy_from_slice(row);
    }
    Ok(tensor)
}

impl Embedder for OpenVinoEmbedder {
    fn info(&self) -> &ModelInfo {
        &self.info
    }

    fn embed_image(&self, image: &DynamicImage) -> Result<Vec<f32>, EmbedError> {
        let inference = |error: openvino::InferenceError| EmbedError::Inference(error.to_string());
        let size = i64::from(self.spec.size);
        let shape = Shape::new(&[1, 3, size, size]).map_err(inference)?;
        let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(inference)?;
        tensor
            .get_data_mut::<f32>()
            .map_err(inference)?
            .copy_from_slice(&image_tensor(image, &self.spec));
        self.vision
            .infer(PIXEL_INPUT, &tensor)?
            .pop()
            .ok_or_else(|| EmbedError::Inference("empty vision output".into()))
    }

    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let rows = texts
            .iter()
            .map(|text| self.text.encode(text))
            .collect::<Result<Vec<_>, _>>()?;
        let tensor = text_ids_tensor(&rows, self.text.max_tokens())?;
        self.text_tower.infer(TEXT_INPUT, &tensor)
    }
}
