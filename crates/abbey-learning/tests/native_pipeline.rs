//! Real native Rust tensor update and file-format pipeline; synthetic fixtures.
use candle_core::{DType, Device, Tensor};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_abbey-model"))
        .current_dir(root)
        .env_clear()
        .args(args)
        .output()
        .unwrap()
}
fn success(root: &Path, args: &[&str]) -> Value {
    let result = run(root, args);
    assert!(
        result.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn checkpoint(root: &Path) {
    let base = root.join("base");
    fs::create_dir(&base).unwrap();
    let mut tensors = HashMap::new();
    let matrix = |r, c| {
        Tensor::from_vec(
            (0..r * c)
                .map(|i| (((i * 17 + 3) % 101) as f32 - 50.0) / 500.0)
                .collect(),
            (r, c),
            &Device::Cpu,
        )
        .unwrap()
    };
    tensors.insert("model.embed_tokens.weight".to_string(), matrix(64, 32));
    tensors.insert(
        "model.norm.weight".into(),
        Tensor::ones(32, DType::F32, &Device::Cpu).unwrap(),
    );
    for layer in 0..2 {
        let p = format!("model.layers.{layer}");
        for name in ["input_layernorm", "post_attention_layernorm"] {
            tensors.insert(
                format!("{p}.{name}.weight"),
                Tensor::ones(32, DType::F32, &Device::Cpu).unwrap(),
            );
        }
        for name in ["q_norm", "k_norm"] {
            tensors.insert(
                format!("{p}.self_attn.{name}.weight"),
                Tensor::ones(8, DType::F32, &Device::Cpu).unwrap(),
            );
        }
        for (name, r, c) in [
            ("self_attn.q_proj", 32, 32),
            ("self_attn.k_proj", 16, 32),
            ("self_attn.v_proj", 16, 32),
            ("self_attn.o_proj", 32, 32),
            ("mlp.gate_proj", 64, 32),
            ("mlp.up_proj", 64, 32),
            ("mlp.down_proj", 32, 64),
        ] {
            tensors.insert(format!("{p}.{name}.weight"), matrix(r, c));
        }
    }
    candle_core::safetensors::save(&tensors, base.join("model.safetensors")).unwrap();
    write(
        &base.join("config.json"),
        &json!({"model_type":"qwen3","hidden_size":32,"intermediate_size":64,"num_hidden_layers":2,"num_attention_heads":4,"num_key_value_heads":2,"head_dim":8,"vocab_size":64,"rms_norm_eps":0.000001,"rope_theta":10000.0,"tie_word_embeddings":true}),
    );
    let mut vocab = serde_json::Map::new();
    for i in 0..64 {
        vocab.insert(format!("word{i}"), json!(i));
    }
    for (word, id) in [
        ("[UNK]", 0),
        ("<|endoftext|>", 1),
        ("user", 2),
        ("assistant", 3),
        ("Question", 4),
        ("Answer", 5),
        ("correct", 60),
        ("<|im_start|>", 62),
        ("<|im_end|>", 63),
    ] {
        vocab.retain(|_, v| v != &json!(id));
        vocab.insert(word.into(), json!(id));
    }
    let added:Vec<_>=[("<|im_start|>",62),("<|im_end|>",63),("<|endoftext|>",1)].into_iter().map(|(s,id)|json!({"id":id,"content":s,"single_word":false,"lstrip":false,"rstrip":false,"normalized":false,"special":true})).collect();
    write(
        &base.join("tokenizer.json"),
        &json!({"version":"1.0","truncation":null,"padding":null,"added_tokens":added,"normalizer":null,"pre_tokenizer":{"type":"Whitespace"},"post_processor":null,"decoder":null,"model":{"type":"WordLevel","vocab":vocab,"unk_token":"[UNK]"}}),
    );
}
fn exercise_pipeline(private: bool) {
    let root = std::env::temp_dir().join(format!("abbey-native-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let f = Fixture(root);
    let root = &f.0;
    checkpoint(root);
    fs::write(
        root.join("consent"),
        if private {
            "abbey-train-consent: v1\nabbey-private-training-consent: v1\n"
        } else {
            "abbey-train-consent: v1\n"
        },
    )
    .unwrap();
    let base_digest = format!(
        "{:x}",
        Sha256::digest(fs::read(root.join("base/model.safetensors")).unwrap())
    );
    let rows:Vec<_>=(0..200).map(|i|json!({"id":format!("example-{i}"),"lineage":format!("group-{}",i/2),"prompt":format!("Question {i}"),"response":format!("Answer {i}"),"provenance":"synthetic owned native test","approved":true,"private":private})).collect();
    fs::write(
        root.join("source.jsonl"),
        rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let categories = [
        "coding",
        "tool_use",
        "everyday",
        "factual",
        "personality",
        "privacy",
        "public_refusal",
        "private_knowledge",
    ];
    let cases:Vec<_>=categories.iter().map(|c|json!({"id":c,"category":c,"prompt":format!("Explain {c}"),"expected":if *c=="tool_use"{"{\"tool\":\"ok\"}"}else{"correct"}})).collect();
    fs::write(
        root.join("suite.jsonl"),
        cases.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let mut prepare = vec![
        "prepare",
        "--source",
        "source.jsonl",
        "--output",
        "dataset",
        "--consent",
        "consent",
    ];
    if private {
        prepare.push("--private");
    }
    success(root, &prepare);
    success(
        root,
        &[
            "freeze-suite",
            "--source",
            "suite.jsonl",
            "--output",
            "suite.json",
            "--consent",
            "consent",
        ],
    );
    let trained = success(
        root,
        &[
            "train",
            "--base",
            "base",
            "--dataset",
            "dataset",
            "--suite",
            "suite.json",
            "--output",
            "run",
            "--license",
            "synthetic-owned",
            "--iters",
            "2",
            "--consent",
            "consent",
            "--timeout",
            "120",
        ],
    );
    assert!(
        trained["tensor_evidence"]["changed_tensors"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(trained["tensor_evidence"]["runtime"], "native-rust-candle");
    let evaluated = success(
        root,
        &[
            "evaluate",
            "--run",
            "run",
            "--suite",
            "suite.json",
            "--consent",
            "consent",
            "--timeout",
            "120",
        ],
    );
    assert_eq!(evaluated["passed"], false);
    assert!(evaluated["heldout"]["tokens"].as_u64().unwrap() > 0);
    for (name, bits) in [("fused", None), ("q4", Some("4")), ("q8", Some("8"))] {
        let mut args = vec![
            "export",
            "--run",
            "run",
            "--output",
            name,
            "--consent",
            "consent",
            "--timeout",
            "120",
        ];
        if let Some(bits) = bits {
            args.extend(["--bits", bits]);
        }
        success(root, &args);
        assert!(
            root.join(name)
                .join(if bits.is_some() {
                    "model.gguf"
                } else {
                    "model.safetensors"
                })
                .exists()
        );
        success(
            root,
            &[
                "benchmark",
                "--base",
                name,
                "--suite",
                "suite.json",
                "--heldout",
                "dataset/test.jsonl",
                "--output",
                &format!("bench-{name}"),
                "--consent",
                "consent",
                "--timeout",
                "120",
            ],
        );
    }
    let rejected = run(
        root,
        &[
            "promote",
            "--run",
            "run",
            "--registry",
            "registry",
            "--consent",
            "consent",
        ],
    );
    assert!(!rejected.status.success());
    assert!(!root.join("registry/current.json").exists());
    assert_eq!(
        base_digest,
        format!(
            "{:x}",
            Sha256::digest(fs::read(root.join("base/model.safetensors")).unwrap())
        )
    );
}

#[test]
fn native_private_pipeline_keeps_separate_consent_and_refuses_bad_promotion() {
    exercise_pipeline(true);
}
#[test]
fn native_public_pipeline_needs_only_public_consent() {
    exercise_pipeline(false);
}
#[test]
fn native_cli_help_succeeds() {
    let output = Command::new(env!("CARGO_BIN_EXE_abbey-model"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Rust"));
}
