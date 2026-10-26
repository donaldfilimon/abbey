use crate::io;
use anyhow::{Context, Result, bail, ensure};
use candle_core::{DType, Device, Tensor};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn load(base: &Path, device: &Device, dtype: DType) -> Result<BTreeMap<String, Tensor>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(base)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|e| e == "safetensors" || e == "gguf")
        {
            files.push(path);
        }
    }
    files.sort();
    ensure!(
        !files.is_empty(),
        "native checkpoint contains no tensor files"
    );
    let mut raw = BTreeMap::new();
    for path in files {
        if path.extension().is_some_and(|e| e == "gguf") {
            let mut file = fs::File::open(path)?;
            let content = candle_core::quantized::gguf_file::Content::read(&mut file)?;
            let names: Vec<_> = content.tensor_infos.keys().cloned().collect();
            for name in names {
                ensure!(!raw.contains_key(&name), "duplicate tensor name");
                raw.insert(
                    name.clone(),
                    content
                        .tensor(&mut file, &name, &Device::Cpu)?
                        .dequantize(&Device::Cpu)?,
                );
            }
        } else {
            for (name, tensor) in candle_core::safetensors::load(path, &Device::Cpu)? {
                ensure!(raw.insert(name, tensor).is_none(), "duplicate tensor name");
            }
        }
    }
    ensure!(raw.len() <= 10000, "too many model tensors");
    let config = io::read_json(&base.join("config.json"))?;
    let quant = config
        .get("quantization")
        .or_else(|| config.get("quantization_config"));
    let mut dense = BTreeMap::new();
    for (name, tensor) in &raw {
        if name.ends_with(".scales") || name.ends_with(".biases") {
            continue;
        }
        let value = if tensor.dtype() == DType::U32 && name.ends_with(".weight") {
            let prefix = name.trim_end_matches(".weight");
            let scales = raw
                .get(&format!("{prefix}.scales"))
                .context("packed weight missing affine scales")?;
            let biases = raw
                .get(&format!("{prefix}.biases"))
                .context("packed weight missing affine biases")?;
            let bits = quant
                .and_then(|q| q["bits"].as_u64())
                .context("packed weights require quantization config")?
                as usize;
            let group = quant
                .and_then(|q| q["group_size"].as_u64())
                .context("packed weights require group size")? as usize;
            ensure!(
                quant.and_then(|q| q["mode"].as_str()).unwrap_or("affine") == "affine",
                "only affine packed weights supported"
            );
            dequantize_affine(tensor, scales, biases, bits, group)?
        } else {
            ensure!(
                matches!(tensor.dtype(), DType::F32 | DType::F16 | DType::BF16),
                "unsupported model tensor dtype"
            );
            tensor.to_dtype(DType::F32)?
        };
        dense.insert(name.clone(), value.to_dtype(dtype)?.to_device(device)?);
    }
    Ok(dense)
}

fn dequantize_affine(
    weight: &Tensor,
    scales: &Tensor,
    biases: &Tensor,
    bits: usize,
    group: usize,
) -> Result<Tensor> {
    ensure!(
        matches!(bits, 4 | 8) && group > 0 && group <= 256,
        "unsupported affine quantization"
    );
    let (rows, packed) = weight.dims2()?;
    let (srows, groups) = scales.dims2()?;
    ensure!(
        srows == rows && biases.dims() == scales.dims(),
        "affine shape mismatch"
    );
    let cols = groups
        .checked_mul(group)
        .context("affine dimensions overflow")?;
    ensure!(
        packed.checked_mul(32 / bits) == Some(cols),
        "packed weight width mismatch"
    );
    let w = weight.flatten_all()?.to_vec1::<u32>()?;
    let s = scales
        .to_dtype(DType::F32)?
        .flatten_all()?
        .to_vec1::<f32>()?;
    let b = biases
        .to_dtype(DType::F32)?
        .flatten_all()?
        .to_vec1::<f32>()?;
    let mut out = Vec::with_capacity(rows.checked_mul(cols).context("tensor shape overflow")?);
    let mask = (1u32 << bits) - 1;
    for r in 0..rows {
        for c in 0..cols {
            let q = (w[r * packed + c / (32 / bits)] >> ((c % (32 / bits)) * bits)) & mask;
            let at = r * groups + c / group;
            let value = q as f32 * s[at] + b[at];
            ensure!(value.is_finite(), "nonfinite dequantized tensor");
            out.push(value);
        }
    }
    Ok(Tensor::from_vec(out, (rows, cols), &Device::Cpu)?)
}

pub(super) fn save(
    base: &Path,
    out: &Path,
    weights: BTreeMap<String, Tensor>,
    bits: Option<u8>,
) -> Result<()> {
    ensure!(!out.exists(), "export output already exists");
    io::create_dir_private(out, true)?;
    let mut config = io::read_json(&base.join("config.json"))?;
    if let Some(object) = config.as_object_mut() {
        object.remove("quantization");
        object.remove("quantization_config");
        object.insert(
            "dtype".into(),
            if weights
                .values()
                .next()
                .is_some_and(|t| t.dtype() == DType::F16)
            {
                "float16"
            } else {
                "float32"
            }
            .into(),
        );
    }
    io::write_json_new(&out.join("config.json"), &config)?;
    for name in [
        "tokenizer.json",
        "tokenizer.model",
        "tokenizer_config.json",
        "chat_template.jinja",
        "generation_config.json",
    ] {
        if base.join(name).is_file() {
            fs::copy(base.join(name), out.join(name))?;
        }
    }
    match bits {
        None => {
            stream_safetensors(out, weights)?;
        }
        Some(bits @ (4 | 8)) => {
            use candle_core::quantized::{GgmlDType, QTensor, gguf_file};
            let mut tensors = Vec::new();
            for (name, tensor) in weights {
                let cpu = tensor
                    .to_device(&Device::Cpu)?
                    .to_dtype(DType::F32)?
                    .contiguous()?;
                let matrix = cpu.rank() == 2 && cpu.dims()[1] % 32 == 0;
                let kind = if matrix {
                    if bits == 4 {
                        GgmlDType::Q4_0
                    } else {
                        GgmlDType::Q8_0
                    }
                } else {
                    GgmlDType::F32
                };
                tensors.push((name, QTensor::quantize(&cpu, kind)?));
            }
            let metadata = [
                (
                    "general.architecture",
                    gguf_file::Value::String("abbey-hf-decoder-v1".into()),
                ),
                (
                    "abbey.config",
                    gguf_file::Value::String(serde_json::to_string(&config)?),
                ),
            ];
            let meta: Vec<_> = metadata.iter().map(|(k, v)| (*k, v)).collect();
            let refs: Vec<_> = tensors.iter().map(|(n, t)| (n.as_str(), t)).collect();
            let mut file = io::private_file_options().open(out.join("model.gguf"))?;
            gguf_file::write(&mut file, &meta, &refs)?;
            file.sync_all()?;
        }
        Some(_) => bail!("quantization must be 4 or 8 bits"),
    }
    Ok(())
}

fn stream_safetensors(out: &Path, weights: BTreeMap<String, Tensor>) -> Result<()> {
    use std::io::Write;
    let mut header = serde_json::Map::new();
    let mut offset = 0usize;
    for (name, tensor) in &weights {
        let (dtype, width) = match tensor.dtype() {
            DType::F32 => ("F32", 4),
            DType::F16 => ("F16", 2),
            DType::BF16 => ("BF16", 2),
            _ => bail!("unsupported dense export dtype"),
        };
        let bytes = tensor
            .elem_count()
            .checked_mul(width)
            .context("export size overflow")?;
        header.insert(name.clone(),serde_json::json!({"dtype":dtype,"shape":tensor.dims(),"data_offsets":[offset,offset+bytes]}));
        offset += bytes;
    }
    let mut metadata = serde_json::to_vec(&header)?;
    while !metadata.len().is_multiple_of(8) {
        metadata.push(b' ');
    }
    ensure!(
        fs4::available_space(out)? > offset as u64 + metadata.len() as u64 + 512 * 1024 * 1024,
        "insufficient export disk headroom"
    );
    let file = io::private_file_options().open(out.join("model.safetensors"))?;
    let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
    writer.write_all(&(metadata.len() as u64).to_le_bytes())?;
    writer.write_all(&metadata)?;
    // Consume one GPU tensor at a time; do not retain a second full CPU model.
    for (_, tensor) in weights {
        let cpu = tensor.to_device(&Device::Cpu)?.flatten_all()?;
        match cpu.dtype() {
            DType::F32 => {
                for value in cpu.to_vec1::<f32>()? {
                    ensure!(value.is_finite(), "nonfinite dense export");
                    writer.write_all(&value.to_le_bytes())?;
                }
            }
            DType::F16 => {
                for value in cpu.to_vec1::<half::f16>()? {
                    ensure!(value.is_finite(), "nonfinite dense export");
                    writer.write_all(&value.to_bits().to_le_bytes())?;
                }
            }
            DType::BF16 => {
                for value in cpu.to_vec1::<half::bf16>()? {
                    ensure!(value.is_finite(), "nonfinite dense export");
                    writer.write_all(&value.to_bits().to_le_bytes())?;
                }
            }
            _ => bail!("unsupported dense export dtype"),
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mlx_affine_unpack_matches_independent_hand_vector() {
        let w = Tensor::from_vec(vec![0x76543210u32], (1, 1), &Device::Cpu).unwrap();
        let s = Tensor::from_vec(vec![2f32], (1, 1), &Device::Cpu).unwrap();
        let b = Tensor::from_vec(vec![-1f32], (1, 1), &Device::Cpu).unwrap();
        assert_eq!(
            dequantize_affine(&w, &s, &b, 4, 8)
                .unwrap()
                .flatten_all()
                .unwrap()
                .to_vec1::<f32>()
                .unwrap(),
            vec![-1., 1., 3., 5., 7., 9., 11., 13.]
        );
    }
}
