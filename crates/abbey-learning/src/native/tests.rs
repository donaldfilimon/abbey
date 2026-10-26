use super::model::{Config, Model};
use candle_core::{DType, Device, Tensor};
use serde_json::json;
use std::collections::BTreeMap;

pub(super) fn tiny_weights() -> (Config, BTreeMap<String, Tensor>) {
    let config:Config=serde_json::from_value(json!({"model_type":"qwen3","hidden_size":32,"intermediate_size":64,"num_hidden_layers":2,"num_attention_heads":4,"num_key_value_heads":2,"head_dim":8,"vocab_size":64,"rms_norm_eps":0.000001,"rope_theta":10000.0,"tie_word_embeddings":true})).unwrap();
    let mut weights = BTreeMap::new();
    let matrix = |rows, cols| {
        Tensor::from_vec(
            (0..rows * cols)
                .map(|i| (((i * 17 + 3) % 101) as f32 - 50.0) / 500.0)
                .collect(),
            (rows, cols),
            &Device::Cpu,
        )
        .unwrap()
    };
    weights.insert("model.embed_tokens.weight".into(), matrix(64, 32));
    weights.insert(
        "model.norm.weight".into(),
        Tensor::ones(32, DType::F32, &Device::Cpu).unwrap(),
    );
    for layer in 0..2 {
        let prefix = format!("model.layers.{layer}");
        for name in ["input_layernorm", "post_attention_layernorm"] {
            weights.insert(
                format!("{prefix}.{name}.weight"),
                Tensor::ones(32, DType::F32, &Device::Cpu).unwrap(),
            );
        }
        for name in ["q_norm", "k_norm"] {
            weights.insert(
                format!("{prefix}.self_attn.{name}.weight"),
                Tensor::ones(8, DType::F32, &Device::Cpu).unwrap(),
            );
        }
        for (name, rows, cols) in [
            ("self_attn.q_proj", 32, 32),
            ("self_attn.k_proj", 16, 32),
            ("self_attn.v_proj", 16, 32),
            ("self_attn.o_proj", 32, 32),
            ("mlp.gate_proj", 64, 32),
            ("mlp.up_proj", 64, 32),
            ("mlp.down_proj", 32, 64),
        ] {
            weights.insert(format!("{prefix}.{name}.weight"), matrix(rows, cols));
        }
    }
    (config, weights)
}
fn difference(a: &Tensor, b: &Tensor) -> f32 {
    (a - b)
        .unwrap()
        .abs()
        .unwrap()
        .max_all()
        .unwrap()
        .to_scalar::<f32>()
        .unwrap()
}
#[test]
fn future_tokens_cannot_change_prefix_logits() {
    let (cfg, w) = tiny_weights();
    let mut model = Model::from_weights(cfg, w, &Device::Cpu, true).unwrap();
    let a = model.forward(&[2, 3, 4, 5], false).unwrap();
    model.clear_cache();
    let b = model.forward(&[2, 3, 4, 6], false).unwrap();
    assert!(difference(&a.narrow(1, 0, 3).unwrap(), &b.narrow(1, 0, 3).unwrap()) < 1e-6);
}
#[test]
fn native_lora_has_real_gradients_and_preserves_base() {
    let (cfg, w) = tiny_weights();
    let mut model = Model::from_weights(cfg, w, &Device::Cpu, true).unwrap();
    let before = model.forward(&[2, 3, 4, 5], false).unwrap();
    let (loss, count) = model.loss(&[2, 3, 4, 5], 2).unwrap();
    assert_eq!(count, 2);
    let grad = loss.backward().unwrap();
    let mut changed = 0;
    for (_, var) in model.variables() {
        if let Some(g) = grad.get(var.as_tensor())
            && g.abs()
                .unwrap()
                .max_all()
                .unwrap()
                .to_scalar::<f32>()
                .unwrap()
                > 0.0
        {
            var.set(&(var.as_tensor() - &(g * 0.01).unwrap()).unwrap())
                .unwrap();
            changed += 1;
        }
    }
    assert!(changed > 0);
    let after = model.forward(&[2, 3, 4, 5], false).unwrap();
    assert!(difference(&before, &after) > 0.0);
}
#[test]
fn native_cache_and_fused_adapter_match_full_forward() {
    let (cfg, w) = tiny_weights();
    let mut model = Model::from_weights(cfg.clone(), w, &Device::Cpu, true).unwrap();
    let full = model.forward(&[2, 3, 4], false).unwrap();
    model.clear_cache();
    let _ = model.forward(&[2, 3], true).unwrap();
    let tail = model.forward(&[4], true).unwrap();
    assert!(difference(&full.narrow(1, 2, 1).unwrap(), &tail) < 1e-5);
    let fused = model.fused_weights().unwrap();
    let mut plain = Model::from_weights(cfg, fused, &Device::Cpu, false).unwrap();
    let result = plain.forward(&[2, 3, 4], false).unwrap();
    assert!(difference(&full, &result) < 1e-5);
}

#[test]
fn decoder_matches_independent_embedding_only_oracle() {
    let cfg:Config=serde_json::from_value(json!({"model_type":"qwen3","hidden_size":4,"intermediate_size":4,"num_hidden_layers":1,"num_attention_heads":1,"num_key_value_heads":1,"head_dim":4,"vocab_size":4,"rms_norm_eps":0.000001,"rope_theta":10000.0,"tie_word_embeddings":true})).unwrap();
    let mut w = BTreeMap::new();
    w.insert(
        "model.embed_tokens.weight".into(),
        Tensor::from_vec(
            vec![
                1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            (4, 4),
            &Device::Cpu,
        )
        .unwrap(),
    );
    w.insert(
        "model.norm.weight".into(),
        Tensor::ones(4, DType::F32, &Device::Cpu).unwrap(),
    );
    for name in [
        "input_layernorm",
        "post_attention_layernorm",
        "self_attn.q_norm",
        "self_attn.k_norm",
    ] {
        w.insert(
            format!("model.layers.0.{name}.weight"),
            Tensor::ones(4, DType::F32, &Device::Cpu).unwrap(),
        );
    }
    for name in [
        "self_attn.q_proj",
        "self_attn.k_proj",
        "self_attn.v_proj",
        "self_attn.o_proj",
        "mlp.gate_proj",
        "mlp.up_proj",
        "mlp.down_proj",
    ] {
        w.insert(
            format!("model.layers.0.{name}.weight"),
            Tensor::zeros((4, 4), DType::F32, &Device::Cpu).unwrap(),
        );
    }
    let mut model = Model::from_weights(cfg, w, &Device::Cpu, false).unwrap();
    let out = model
        .forward(&[2], false)
        .unwrap()
        .flatten_all()
        .unwrap()
        .to_vec1::<f32>()
        .unwrap();
    let expected = 1.0f32 / (0.25f32 + 0.000001).sqrt();
    assert_eq!(out[0], 0.0);
    assert_eq!(out[1], 0.0);
    assert_eq!(out[3], 0.0);
    assert!((out[2] - expected).abs() < 1e-6);
}

#[test]
fn silu_saturated_gradients_are_finite_in_float_and_half() {
    for dtype in [DType::F32, DType::F16] {
        let values = Tensor::from_vec(vec![-100f32, -20., -1., 0., 1., 20., 100.], 7, &Device::Cpu)
            .unwrap()
            .to_dtype(dtype)
            .unwrap();
        let var = candle_core::Var::from_tensor(&values).unwrap();
        let out = super::model::silu_stable(var.as_tensor()).unwrap();
        let gradients = out.sum_all().unwrap().backward().unwrap();
        let grad = gradients
            .get(var.as_tensor())
            .unwrap()
            .to_dtype(DType::F32)
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        assert!(grad.iter().all(|v| v.is_finite()));
        assert!((grad[3] - 0.5).abs() < 0.001);
        assert!((grad[6] - 1.0).abs() < 0.001);
    }
}
#[cfg(feature = "metal")]
#[test]
fn native_metal_has_finite_real_adapter_gradients() {
    let device = Device::new_metal(0).expect("explicit Metal cohort requires a device");
    let (cfg, weights) = tiny_weights();
    let weights = weights
        .into_iter()
        .map(|(name, t)| Ok((name, t.to_dtype(DType::F16)?.to_device(&device)?)))
        .collect::<candle_core::Result<BTreeMap<_, _>>>()
        .unwrap();
    let mut model = Model::from_weights(cfg, weights, &device, true).unwrap();
    let (loss, _) = model.loss(&[2, 3, 4, 5], 2).unwrap();
    let gradients = loss.backward().unwrap();
    let mut nonzero = false;
    for (_, var) in model.variables() {
        if let Some(grad) = gradients.get(var.as_tensor()) {
            let values = grad
                .to_dtype(DType::F32)
                .unwrap()
                .to_device(&Device::Cpu)
                .unwrap()
                .flatten_all()
                .unwrap()
                .to_vec1::<f32>()
                .unwrap();
            assert!(values.iter().all(|v| v.is_finite()));
            nonzero |= values.iter().any(|v| *v != 0.0);
        }
    }
    assert!(nonzero);
}
