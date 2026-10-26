//! Explicit dense decoder equations with trainable low-rank residuals.
//! Qwen3 per-head normalization and noninterleaved RoPE follow its public
//! architecture; all operators, differentiation and updates execute in Rust.
use crate::io;
use anyhow::{Context, Result, ensure};
use candle_core::{D, DType, Device, Tensor, Var};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct Config {
    pub model_type: String,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    #[serde(default)]
    pub head_dim: usize,
    pub vocab_size: usize,
    #[serde(default = "epsilon")]
    pub rms_norm_eps: f64,
    #[serde(default = "theta")]
    pub rope_theta: f64,
    #[serde(default)]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub attention_bias: bool,
    #[serde(default = "activation")]
    pub hidden_act: String,
    #[serde(default)]
    pub rope_scaling: Option<serde_json::Value>,
    #[serde(default)]
    pub use_sliding_window: bool,
}
fn activation() -> String {
    "silu".into()
}
fn epsilon() -> f64 {
    1e-6
}
fn theta() -> f64 {
    10000.0
}
impl Config {
    fn validate(&mut self) -> Result<()> {
        ensure!(
            ["qwen3", "qwen2", "llama", "mistral"].contains(&self.model_type.as_str()),
            "unsupported native architecture (qwen3/qwen2/llama/mistral only)"
        );
        ensure!(
            self.hidden_size > 0
                && self.hidden_size <= 8192
                && self.intermediate_size > 0
                && self.intermediate_size <= 65536
                && self.num_hidden_layers > 0
                && self.num_hidden_layers <= 128
                && self.vocab_size > 1
                && self.vocab_size <= 262144,
            "model dimensions exceed native envelope"
        );
        ensure!(
            self.num_attention_heads > 0
                && self.num_attention_heads <= 128
                && self.num_key_value_heads > 0
                && self
                    .num_attention_heads
                    .is_multiple_of(self.num_key_value_heads),
            "invalid grouped attention dimensions"
        );
        if self.head_dim == 0 {
            ensure!(
                self.hidden_size.is_multiple_of(self.num_attention_heads),
                "hidden size/head count mismatch"
            );
            self.head_dim = self.hidden_size / self.num_attention_heads;
        }
        ensure!(
            self.head_dim > 0
                && self.head_dim <= 512
                && self.head_dim.is_multiple_of(2)
                && self.rope_theta.is_finite()
                && self.rope_theta > 1.0
                && self.rms_norm_eps.is_finite()
                && self.rms_norm_eps > 0.0,
            "invalid rotary or normalization settings"
        );
        ensure!(
            self.hidden_act == "silu",
            "native decoder requires SiLU activation"
        );
        ensure!(
            self.rope_scaling.is_none() && !self.use_sliding_window,
            "scaled RoPE/sliding-window variants require separate native qualification"
        );
        Ok(())
    }
}
struct Linear {
    name: String,
    weight: Tensor,
    bias: Option<Tensor>,
    a: Option<Var>,
    b: Option<Var>,
}
impl Linear {
    fn new(name: String, w: &BTreeMap<String, Tensor>, train: bool, seed: usize) -> Result<Self> {
        let weight = w
            .get(&format!("{name}.weight"))
            .with_context(|| format!("missing {name}.weight"))?
            .clone();
        let (out, input) = weight.dims2()?;
        let (a, b) = if train {
            let rank = 8;
            let device = weight.device();
            let initial: Vec<f32> = (0..rank * input)
                .map(|i| {
                    (((i.wrapping_mul(1664525)
                        .wrapping_add(seed * 101 + 1013904223))
                        % 65536) as f32
                        / 65536.0
                        - 0.5)
                        * 0.02
                })
                .collect();
            (
                Some(Var::from_tensor(&Tensor::from_vec(
                    initial,
                    (rank, input),
                    device,
                )?)?),
                Some(Var::zeros((out, rank), DType::F32, device)?),
            )
        } else {
            (None, None)
        };
        Ok(Self {
            name: name.clone(),
            weight,
            bias: w.get(&format!("{name}.bias")).cloned(),
            a,
            b,
        })
    }
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mut y = x.broadcast_matmul(&self.weight.t()?)?;
        if let Some(bias) = &self.bias {
            y = y.broadcast_add(bias)?;
        }
        if let (Some(a), Some(b)) = (&self.a, &self.b) {
            let update = x
                .broadcast_matmul(&a.as_tensor().to_dtype(x.dtype())?.t()?)?
                .broadcast_matmul(&b.as_tensor().to_dtype(x.dtype())?.t()?)?;
            y = (y + (update * 16.0)?)?;
        }
        Ok(y)
    }
    fn variables(&self) -> Vec<(String, Var)> {
        match (&self.a, &self.b) {
            (Some(a), Some(b)) => vec![
                (format!("{}.lora_a", self.name), a.clone()),
                (format!("{}.lora_b", self.name), b.clone()),
            ],
            _ => Vec::new(),
        }
    }
    fn fused(&self) -> Result<Tensor> {
        if let (Some(a), Some(b)) = (&self.a, &self.b) {
            Ok((&self.weight
                + (b.as_tensor().matmul(a.as_tensor())? * 16.0)?.to_dtype(self.weight.dtype())?)?)
        } else {
            Ok(self.weight.clone())
        }
    }
}
struct Layer {
    q: Linear,
    k: Linear,
    v: Linear,
    o: Linear,
    gate: Linear,
    up: Linear,
    down: Linear,
    input_norm: Tensor,
    post_norm: Tensor,
    q_norm: Option<Tensor>,
    k_norm: Option<Tensor>,
    cache: Option<(Tensor, Tensor)>,
}
impl Layer {
    fn linears(&self) -> [&Linear; 7] {
        [
            &self.q, &self.k, &self.v, &self.o, &self.gate, &self.up, &self.down,
        ]
    }
}
pub(super) struct Model {
    pub config: Config,
    weights: BTreeMap<String, Tensor>,
    layers: Vec<Layer>,
    embedding: Tensor,
    norm: Tensor,
    head: Tensor,
    device: Device,
    position: usize,
    train_start: usize,
    base_sha256: Option<String>,
}
fn norm(x: &Tensor, weight: &Tensor, epsilon: f64) -> Result<Tensor> {
    let f = x.to_dtype(DType::F32)?;
    let normalized = f.broadcast_div(&(f.sqr()?.mean_keepdim(D::Minus1)? + epsilon)?.sqrt()?)?;
    Ok(normalized.to_dtype(x.dtype())?.broadcast_mul(weight)?)
}
pub(super) fn silu_stable(x: &Tensor) -> Result<Tensor> {
    let f = x.to_dtype(DType::F32)?;
    let sigmoid = f.affine(0.5, 0.0)?.tanh()?.affine(0.5, 0.5)?;
    Ok((f * sigmoid)?.to_dtype(x.dtype())?)
}
fn softmax(x: &Tensor) -> Result<Tensor> {
    let f = x.to_dtype(DType::F32)?;
    let e = f
        .broadcast_sub(&f.max_keepdim(D::Minus1)?.detach())?
        .exp()?;
    Ok(e.broadcast_div(&e.sum_keepdim(D::Minus1)?)?
        .to_dtype(x.dtype())?)
}
impl Model {
    pub fn load(base: &Path, device: &Device, train: bool) -> Result<Self> {
        let mut config: Config = serde_json::from_value(io::read_json(&base.join("config.json"))?)?;
        config.validate()?;
        let dtype = if device.is_cpu() {
            DType::F32
        } else {
            DType::F16
        };
        let h = config.hidden_size as u128;
        let q = (config.num_attention_heads * config.head_dim) as u128;
        let kv = (config.num_key_value_heads * config.head_dim) as u128;
        let embedding = config.vocab_size as u128 * h;
        let layers = config.num_hidden_layers as u128
            * (2 * q * h + 2 * kv * h + 3 * h * config.intermediate_size as u128 + 4 * h);
        let params = embedding
            + layers
            + if config.tie_word_embeddings {
                0
            } else {
                embedding
            };
        let dense = params * if device.is_cpu() { 4 } else { 2 };
        let mut source = 0u128;
        for entry in std::fs::read_dir(base)? {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|s| s == "safetensors" || s == "gguf")
            {
                source += path.metadata()?.len() as u128;
            }
        }
        let limit = if device.is_cpu() { 8u128 } else { 12u128 } * 1024 * 1024 * 1024;
        ensure!(
            dense + source <= limit,
            "native model exceeds conservative resident envelope; use a smaller exact base or a qualified Metal route"
        );
        let weights = super::weights::load(base, device, dtype)?;
        let mut model = Self::from_weights(config, weights, device, train)?;
        model.base_sha256 = io::model_identity(base)?["sha256"]
            .as_str()
            .map(str::to_string);
        Ok(model)
    }
    pub fn from_weights(
        mut config: Config,
        weights: BTreeMap<String, Tensor>,
        device: &Device,
        train: bool,
    ) -> Result<Self> {
        config.validate()?;
        let get = |key: &str| {
            weights
                .get(key)
                .cloned()
                .with_context(|| format!("missing model tensor {key}"))
        };
        let embedding = get("model.embed_tokens.weight")?;
        ensure!(
            embedding.dims() == [config.vocab_size, config.hidden_size],
            "embedding dimensions mismatch"
        );
        let head = if config.tie_word_embeddings {
            embedding.clone()
        } else {
            get("lm_head.weight")?
        };
        let final_norm = get("model.norm.weight")?;
        let train_start = if train {
            config.num_hidden_layers.saturating_sub(2)
        } else {
            config.num_hidden_layers
        };
        let mut layers = Vec::new();
        for index in 0..config.num_hidden_layers {
            let prefix = format!("model.layers.{index}");
            let tuned = index >= train_start;
            let linear = |suffix: &str| {
                Linear::new(format!("{prefix}.{suffix}"), &weights, tuned, index + 17)
            };
            layers.push(Layer {
                q: linear("self_attn.q_proj")?,
                k: linear("self_attn.k_proj")?,
                v: linear("self_attn.v_proj")?,
                o: linear("self_attn.o_proj")?,
                gate: linear("mlp.gate_proj")?,
                up: linear("mlp.up_proj")?,
                down: linear("mlp.down_proj")?,
                input_norm: get(&format!("{prefix}.input_layernorm.weight"))?,
                post_norm: get(&format!("{prefix}.post_attention_layernorm.weight"))?,
                q_norm: if config.model_type == "qwen3" {
                    Some(get(&format!("{prefix}.self_attn.q_norm.weight"))?)
                } else {
                    None
                },
                k_norm: if config.model_type == "qwen3" {
                    Some(get(&format!("{prefix}.self_attn.k_norm.weight"))?)
                } else {
                    None
                },
                cache: None,
            });
        }
        Ok(Self {
            config,
            weights,
            layers,
            embedding,
            norm: final_norm,
            head,
            device: device.clone(),
            position: 0,
            train_start,
            base_sha256: None,
        })
    }
    pub fn reseed(&self, seed: u64) -> Result<()> {
        for (name, var) in self.variables() {
            if !name.ends_with(".lora_a") {
                continue;
            }
            let mut state = seed;
            for byte in name.bytes() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(byte as u64 + 1);
            }
            let count = var.elem_count();
            let scale = 1.0 / (var.dims()[1] as f32).sqrt();
            let values: Vec<f32> = (0..count)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    ((state >> 32) as u32 as f32 / u32::MAX as f32 * 2.0 - 1.0) * scale
                })
                .collect();
            var.set(&Tensor::from_vec(values, var.shape(), &self.device)?)?;
        }
        Ok(())
    }
    pub fn clear_cache(&mut self) {
        self.position = 0;
        for layer in &mut self.layers {
            layer.cache = None;
        }
    }
    pub fn variables(&self) -> Vec<(String, Var)> {
        self.layers
            .iter()
            .flat_map(|l| l.linears().into_iter().flat_map(Linear::variables))
            .collect()
    }
    pub fn load_adapter(&self, path: &Path) -> Result<()> {
        let config = io::read_json(&path.join("adapter_config.json"))?;
        ensure!(
            config["format"] == "abbey-native-lora-v1" && config["scale"] == 16.0,
            "unsupported native adapter format"
        );
        if let Some(identity) = &self.base_sha256 {
            ensure!(
                config["base_sha256"].as_str() == Some(identity.as_str()),
                "native adapter belongs to a different exact base"
            );
        }
        let values =
            candle_core::safetensors::load(path.join("adapters.safetensors"), &self.device)?;
        let vars = self.variables();
        ensure!(values.len() == vars.len(), "adapter tensor names mismatch");
        for (name, var) in vars {
            let tensor = values.get(&name).context("adapter tensor missing")?;
            ensure!(var.dims() == tensor.dims(), "adapter tensor shape mismatch");
            let checked = tensor.to_dtype(DType::F32)?;
            ensure!(
                checked
                    .to_device(&Device::Cpu)?
                    .flatten_all()?
                    .to_vec1::<f32>()?
                    .iter()
                    .all(|v| v.is_finite()),
                "nonfinite native adapter tensor"
            );
            var.set(&checked)?;
        }
        Ok(())
    }
    pub fn fused_weights(&self) -> Result<BTreeMap<String, Tensor>> {
        let mut weights = self.weights.clone();
        for layer in &self.layers {
            for linear in layer.linears() {
                weights.insert(format!("{}.weight", linear.name), linear.fused()?);
            }
        }
        Ok(weights)
    }
    pub fn forward(&mut self, ids: &[u32], cached: bool) -> Result<Tensor> {
        ensure!(
            !ids.is_empty() && ids.iter().all(|i| (*i as usize) < self.config.vocab_size),
            "invalid native token sequence"
        );
        let seq = ids.len();
        let offset = if cached { self.position } else { 0 };
        ensure!(
            offset + seq <= 512,
            "native pilot sequence limit is 512 tokens"
        );
        let indices = Tensor::from_slice(ids, seq, &self.device)?;
        let mut h = self.embedding.index_select(&indices, 0)?.unsqueeze(0)?;
        let cfg = &self.config;
        let heads = cfg.num_attention_heads;
        let kv = cfg.num_key_value_heads;
        let dim = cfg.head_dim;
        let mut angles = Vec::with_capacity(seq * dim / 2);
        for pos in offset..offset + seq {
            for index in 0..dim / 2 {
                angles
                    .push(pos as f32 / cfg.rope_theta.powf((2 * index) as f64 / dim as f64) as f32);
            }
        }
        let angles = Tensor::from_vec(angles, (1, 1, seq, dim / 2), &self.device)?;
        let cos = angles.cos()?.to_dtype(h.dtype())?;
        let sin = angles.sin()?.to_dtype(h.dtype())?;
        let rotate = |x: &Tensor| -> Result<Tensor> {
            let first = x.narrow(D::Minus1, 0, dim / 2)?;
            let second = x.narrow(D::Minus1, dim / 2, dim / 2)?;
            let a = (first.broadcast_mul(&cos)? - second.broadcast_mul(&sin)?)?;
            let b = (second.broadcast_mul(&cos)? + first.broadcast_mul(&sin)?)?;
            Ok(Tensor::cat(&[a, b], D::Minus1)?)
        };
        for (index, layer) in self.layers.iter_mut().enumerate() {
            if index == self.train_start {
                h = h.detach();
            }
            let x = norm(&h, &layer.input_norm, cfg.rms_norm_eps)?;
            let mut q = layer
                .q
                .forward(&x)?
                .reshape((1, seq, heads, dim))?
                .transpose(1, 2)?;
            let mut k = layer
                .k
                .forward(&x)?
                .reshape((1, seq, kv, dim))?
                .transpose(1, 2)?;
            let mut v = layer
                .v
                .forward(&x)?
                .reshape((1, seq, kv, dim))?
                .transpose(1, 2)?;
            if let Some(weight) = &layer.q_norm {
                q = norm(&q, weight, cfg.rms_norm_eps)?;
            }
            if let Some(weight) = &layer.k_norm {
                k = norm(&k, weight, cfg.rms_norm_eps)?;
            }
            q = rotate(&q)?;
            k = rotate(&k)?;
            if cached {
                if let Some((old_k, old_v)) = &layer.cache {
                    k = Tensor::cat(&[old_k, &k], 2)?;
                    v = Tensor::cat(&[old_v, &v], 2)?;
                }
                layer.cache = Some((k.detach(), v.detach()));
            }
            let total = k.dim(2)?;
            let repeat = |x: &Tensor| -> Result<Tensor> {
                Ok(x.unsqueeze(2)?
                    .broadcast_as((1, kv, heads / kv, total, dim))?
                    .reshape((1, heads, total, dim))?)
            };
            let k = repeat(&k)?;
            let v = repeat(&v)?;
            let scores = (q.matmul(&k.transpose(2, 3)?)? / (dim as f64).sqrt())?;
            let mask: Vec<f32> = (0..seq)
                .flat_map(|i| (0..total).map(move |j| if j > offset + i { -1e9 } else { 0.0 }))
                .collect();
            let mask = Tensor::from_vec(mask, (1, 1, seq, total), &self.device)?
                .to_dtype(scores.dtype())?;
            let attention = softmax(&scores.broadcast_add(&mask)?)?
                .matmul(&v)?
                .transpose(1, 2)?
                .reshape((1, seq, heads * dim))?;
            h = (h + layer.o.forward(&attention)?)?;
            let x = norm(&h, &layer.post_norm, cfg.rms_norm_eps)?;
            let gate = layer.gate.forward(&x)?;
            let silu = silu_stable(&gate)?;
            h = (h + layer.down.forward(&(silu * layer.up.forward(&x)?)?)?)?;
        }
        if cached {
            self.position += seq;
        }
        Ok(norm(&h, &self.norm, cfg.rms_norm_eps)?.broadcast_matmul(&self.head.t()?)?)
    }
    pub fn loss(&mut self, ids: &[u32], response_start: usize) -> Result<(Tensor, usize)> {
        ensure!(
            ids.len() >= 2 && response_start > 0 && response_start < ids.len(),
            "positive response token count required"
        );
        self.clear_cache();
        let logits = self
            .forward(&ids[..ids.len() - 1], false)?
            .to_dtype(DType::F32)?;
        let start = response_start - 1;
        let count = ids.len() - response_start;
        let scores = logits.narrow(1, start, count)?.squeeze(0)?;
        let targets = Tensor::from_slice(&ids[response_start..], (count, 1), &self.device)?;
        let correct = scores.gather(&targets, 1)?.squeeze(1)?;
        let logz = scores.log_sum_exp(1)?;
        Ok(((logz - correct)?.mean_all()?, count))
    }
}
