use super::model::Model;
use crate::io;
use anyhow::{Context, Result, bail, ensure};
use candle_core::{DType, Device, Tensor};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};
use tokenizers::Tokenizer;

fn path(request: &Value, key: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(request[key].as_str().with_context(|| {
        format!("native request requires {key}")
    })?))
}
fn device(request: &Value) -> Result<Device> {
    match request["device"].as_str().unwrap_or("cpu") {
        "cpu" => Ok(Device::Cpu),
        "metal" => {
            #[cfg(feature = "metal")]
            {
                Ok(Device::new_metal(0)?)
            }
            #[cfg(not(feature = "metal"))]
            {
                bail!(
                    "native Metal training requires --features native-training-metal (or abbey-learning/metal)"
                )
            }
        }
        _ => bail!("native device must be cpu or metal; no fallback"),
    }
}
fn scalar(t: &Tensor) -> Result<f32> {
    Ok(t.to_dtype(DType::F32)?
        .to_device(&Device::Cpu)?
        .to_scalar::<f32>()?)
}
fn snapshot(model: &Model) -> Result<Value> {
    let mut map = serde_json::Map::new();
    for (name, var) in model.variables() {
        let values = var
            .as_tensor()
            .to_device(&Device::Cpu)?
            .flatten_all()?
            .to_vec1::<f32>()?;
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        map.insert(name,json!({"sha256":io::digest_bytes(&bytes),"shape":var.dims(),"finite":values.iter().all(|v|v.is_finite())}));
    }
    Ok(map.into())
}
struct Encoding {
    tokenizer: Tokenizer,
    style: u8,
    stop: Vec<u32>,
}
impl Encoding {
    fn new(base: &Path) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(base.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("native tokenizer: {e}"))?;
        let vocab = tokenizer.get_vocab(true);
        let style = if vocab.contains_key("<|im_start|>") && vocab.contains_key("<|im_end|>") {
            1
        } else if vocab.contains_key("<|start_header_id|>") && vocab.contains_key("<|eot_id|>") {
            2
        } else {
            bail!("native chat template not qualified: requires Qwen or Llama3 control tokens")
        };
        let stop = ["<|im_end|>", "<|eot_id|>", "<|endoftext|>", "</s>"]
            .iter()
            .filter_map(|s| vocab.get(*s).copied())
            .collect();
        Ok(Self {
            tokenizer,
            style,
            stop,
        })
    }
    fn prefix(&self, prompt: &str) -> String {
        if self.style == 1 {
            format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n")
        } else {
            format!(
                "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\n{prompt}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"
            )
        }
    }
    fn ids(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self
            .tokenizer
            .encode(text, false)
            .map_err(|e| anyhow::anyhow!("native encoding: {e}"))?
            .get_ids()
            .to_vec())
    }
    fn example(&self, row: &Value) -> Result<(Vec<u32>, usize)> {
        let prefix = self.prefix(row["prompt"].as_str().context("example prompt required")?);
        let response = row["response"]
            .as_str()
            .context("example response required")?;
        let stop = if self.style == 1 {
            "<|im_end|>"
        } else {
            "<|eot_id|>"
        };
        let p = self.ids(&prefix)?;
        let full = self.ids(&format!("{prefix}{response}{stop}"))?;
        let start = p.iter().zip(&full).take_while(|(a, b)| a == b).count();
        ensure!(
            start > 0 && start < full.len() && full.len() <= 512,
            "training/heldout example exceeds native 512-token envelope or has no response tokens"
        );
        Ok((full, start))
    }
    fn generate(&self, model: &mut Model, prompt: &str) -> Result<(String, usize)> {
        let mut ids = self.ids(&self.prefix(prompt))?;
        ensure!(
            !ids.is_empty() && ids.len() <= 384,
            "native prompt exceeds bounded generation envelope"
        );
        model.clear_cache();
        let mut output = Vec::new();
        for index in 0..128 {
            let input = if index == 0 {
                ids.as_slice()
            } else {
                &ids[ids.len() - 1..]
            };
            let logits = model.forward(input, true)?;
            let last = logits
                .narrow(1, logits.dim(1)? - 1, 1)?
                .flatten_all()?
                .to_dtype(DType::F32)?
                .to_device(&Device::Cpu)?
                .to_vec1::<f32>()?;
            ensure!(
                last.iter().all(|v| v.is_finite()),
                "nonfinite native generation logits"
            );
            let next = last
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .context("empty vocabulary")?
                .0 as u32;
            if self.stop.contains(&next) {
                break;
            }
            output.push(next);
            ids.push(next);
        }
        let text = self
            .tokenizer
            .decode(&output, true)
            .map_err(|e| anyhow::anyhow!("native decoding: {e}"))?;
        Ok((text, output.len()))
    }
}

pub(super) fn execute(request: &Value) -> Result<Value> {
    let grant = path(request, "consent")?;
    io::consent(&grant, false)?;
    let action = request["action"]
        .as_str()
        .context("native action required")?;
    match action {
        "train" => train(request),
        "evaluate" => evaluate(request),
        "export" => export(request),
        _ => bail!("unknown native worker action"),
    }
}
fn train(request: &Value) -> Result<Value> {
    let base = path(request, "base")?;
    let data = path(request, "dataset")?;
    let output = path(request, "output")?;
    let manifest = crate::data::verify_dataset(&data)?;
    io::consent(
        &path(request, "consent")?,
        manifest["private"].as_bool().unwrap_or(true),
    )?;
    let iterations = request["iterations"]
        .as_u64()
        .context("iterations required")? as usize;
    ensure!(
        (1..=10000).contains(&iterations),
        "iterations outside native envelope"
    );
    let dev = device(request)?;
    let formatter = Encoding::new(&base)?;
    let examples: Vec<_> = io::read_jsonl(&data.join("train.jsonl"))?
        .iter()
        .map(|row| formatter.example(row))
        .collect::<Result<_>>()?;
    ensure!(!examples.is_empty(), "empty native training split");
    let mut model = Model::load(&base, &dev, true)?;
    model.reseed(request["seed"].as_u64().unwrap_or(17))?;
    let before = snapshot(&model)?;
    let variables = model.variables();
    ensure!(
        !variables.is_empty(),
        "native model exposes no LoRA tensors"
    );
    let mut state: BTreeMap<String, (Tensor, Tensor)> = BTreeMap::new();
    let mut trace = Vec::new();
    let started = Instant::now();
    for step in 0..iterations {
        let (ids, start) = &examples[step % examples.len()];
        let (loss, count) = model.loss(ids, *start)?;
        let value = scalar(&loss)?;
        ensure!(
            value.is_finite() && value >= 0.0,
            "nonfinite native training loss"
        );
        let gradients = loss.backward()?;
        let mut norm = 0.0f64;
        for (_, var) in &variables {
            if let Some(g) = gradients.get(var.as_tensor()) {
                norm += scalar(&g.sqr()?.sum_all()?)? as f64;
            }
        }
        ensure!(norm.is_finite(), "nonfinite gradient norm");
        let clip = if norm > 1.0 { 1.0 / norm.sqrt() } else { 1.0 };
        for (name, var) in &variables {
            if let Some(gradient) = gradients.get(var.as_tensor()) {
                let g = (gradient.to_dtype(DType::F32)? * clip)?.detach();
                let entry = state
                    .entry(name.clone())
                    .or_insert((var.zeros_like()?, var.zeros_like()?));
                let m = ((&entry.0 * 0.9)? + (&g * 0.1)?)?.detach();
                let v = ((&entry.1 * 0.999)? + (g.sqr()? * 0.001)?)?.detach();
                let corrected_m = (&m / (1.0 - 0.9f64.powi((step + 1) as i32)))?;
                let corrected_v = (&v / (1.0 - 0.999f64.powi((step + 1) as i32)))?;
                let delta = corrected_m.broadcast_div(&(corrected_v.sqrt()? + 1e-8)?)?;
                var.set(&(var.as_tensor() - (delta * 1e-4)?)?.detach())?;
                *entry = (m, v);
            }
        }
        trace.push(json!({"step":step+1,"mean_nll":value,"scored_tokens":count}));
    }
    let after = snapshot(&model)?;
    let changed = before
        .as_object()
        .context("tensor map")?
        .iter()
        .filter(|(name, value)| value["sha256"] != after[*name]["sha256"])
        .count();
    ensure!(changed > 0, "no actual adapter tensor update");
    let adapter = output.join("adapter");
    io::create_dir_private(&adapter, false)?;
    let weights: std::collections::HashMap<_, _> = variables
        .iter()
        .map(|(name, var)| {
            Ok((
                name.clone(),
                var.as_tensor().detach().to_device(&Device::Cpu)?,
            ))
        })
        .collect::<Result<_>>()?;
    candle_core::safetensors::save(&weights, adapter.join("adapters.safetensors"))?;
    io::write_json_new(
        &adapter.join("adapter_config.json"),
        &json!({"format":"abbey-native-lora-v1","rank":8,"scale":16.0,"layers":model.config.num_hidden_layers.min(2),"architecture":model.config.model_type,"private":manifest["private"],"base_sha256":io::model_identity(&base)?["sha256"]}),
    )?;
    let evidence = json!({"changed_tensors":changed,"finite":after.as_object().context("tensor map")?.values().all(|v|v["finite"]==true),"before":before,"after":after,"runtime":"native-rust-candle","backend_revision":"70f72e9cfdbfc570846ca363ead2135394af3e9a","seed":request["seed"],"max_sequence_length":512,"optimizer":"Adam, lr=1e-4, global gradient norm clip=1","trace":trace,"elapsed_seconds":started.elapsed().as_secs_f64()});
    io::write_json_new(&output.join("tensors.json"), &evidence)?;
    Ok(evidence)
}
fn evaluate(request: &Value) -> Result<Value> {
    let base = path(request, "base")?;
    let suite = crate::data::verify_suite(&path(request, "suite")?)?;
    let heldout = io::read_jsonl(&path(request, "heldout")?)?;
    let mut private = crate::pipeline::validate_heldout(&heldout)?;
    if let Some(adapter) = request["adapter"].as_str() {
        private |= io::read_json(&Path::new(adapter).join("adapter_config.json"))?["private"]
            .as_bool()
            .unwrap_or(true);
    }
    io::consent(&path(request, "consent")?, private)?;
    let output = path(request, "output")?;
    let dev = device(request)?;
    let formatter = Encoding::new(&base)?;
    let sequences: Vec<_> = heldout
        .iter()
        .map(|r| formatter.example(r))
        .collect::<Result<_>>()?;
    ensure!(
        !sequences.is_empty(),
        "nonempty native heldout data required"
    );
    let sequence_digest = io::digest_value(&serde_json::to_value(&sequences)?);
    let cases = suite["cases"].as_array().context("suite cases required")?;
    ensure!(
        !cases.is_empty() && cases.len() <= 1000,
        "native suite outside envelope"
    );
    let mut rows: Vec<Value> = cases.iter().map(|c| json!({"id":c["id"]})).collect();
    let mut losses = serde_json::Map::new();
    let mut performance = serde_json::Map::new();
    let mut tokens = 0usize;
    for variant in ["base", "candidate"] {
        let adapter = request
            .get("adapter")
            .and_then(Value::as_str)
            .filter(|_| variant == "candidate")
            .map(Path::new);
        let mut model = Model::load(&base, &dev, adapter.is_some())?;
        if let Some(adapter) = adapter {
            model.load_adapter(adapter)?;
        }
        let mut sum = 0.0f64;
        tokens = 0;
        for (ids, start) in &sequences {
            let (loss, count) = model.loss(ids, *start)?;
            let loss = scalar(&loss)?;
            ensure!(
                loss.is_finite() && loss >= 0.0,
                "invalid native heldout loss"
            );
            sum += loss as f64 * count as f64;
            tokens += count;
        }
        losses.insert(variant.into(), json!(sum / tokens as f64));
        let started = Instant::now();
        let mut generated = 0usize;
        for (case, row) in cases.iter().zip(rows.iter_mut()) {
            let (answer, count) = formatter.generate(
                &mut model,
                case["prompt"].as_str().context("suite prompt required")?,
            )?;
            row[variant] = answer.into();
            generated += count;
        }
        let seconds = started.elapsed().as_secs_f64();
        performance.insert(variant.into(),json!({"generation_seconds":seconds,"generated_tokens":generated,"tokens_per_second":generated as f64/seconds.max(1e-9),"device":request["device"],"runtime":"native-rust"}));
    }
    losses.insert("tokens".into(), tokens.into());
    losses.insert("sequence_sha256".into(), sequence_digest.into());
    losses.insert(
        "mask".into(),
        "response-only; identical native tokenizer sequences".into(),
    );
    let report = json!({"cases":rows,"heldout":losses,"performance":performance,"settings":{"greedy":true,"max_new_tokens":128,"max_sequence_length":512}});
    io::write_json_new(&output, &report)?;
    Ok(report)
}
fn export(request: &Value) -> Result<Value> {
    let base = path(request, "base")?;
    let adapter = path(request, "adapter")?;
    let config = io::read_json(&adapter.join("adapter_config.json"))?;
    io::consent(
        &path(request, "consent")?,
        config["private"].as_bool().unwrap_or(true),
    )?;
    let dev = device(request)?;
    let model = Model::load(&base, &dev, true)?;
    model.load_adapter(&adapter)?;
    let bits = request.get("bits").and_then(Value::as_u64).map(|n| n as u8);
    let output = path(request, "output")?;
    let weights = model.fused_weights()?;
    drop(model);
    super::weights::save(&base, &output, weights, bits)?;
    Ok(
        json!({"runtime":"native-rust","bits":bits,"qualification":"unqualified export; independently evaluate this exact runtime and variant"}),
    )
}
