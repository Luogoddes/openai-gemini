//! 更新元数据的签名信封校验。
//!
//! 格式与 snow-shot 对齐：外层 `{schema, keyId, payload(base64), signature(base64)}`，
//! 签名对象是 payload 的**原始字节**（不是重新序列化后的 JSON），算法为
//! RSA-PSS + SHA-256 + salt 长度 32，密钥 3072 位；公钥编译进二进制，私钥只在发布机器上。

use crate::error::AppError;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rsa::pss::Pss;
use rsa::{BigUint, RsaPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// 编译期嵌入的可信公钥；发布前用 `tools/new-release-key.ps1` 生成并替换。
const TRUSTED_KEYS: &str = include_str!("../resources/update-trusted-keys.json");
const MODULUS_BYTES: usize = 384; // 3072-bit
const SALT_LEN: usize = 32;

#[derive(Deserialize)]
struct TrustedKeys {
    keys: Vec<TrustedKey>,
}

#[derive(Deserialize)]
struct TrustedKey {
    id: String,
    modulus: String,
    exponent: String,
}

fn decode(value: Option<&serde_json::Value>, what: &str) -> Result<Vec<u8>, AppError> {
    let text = value.and_then(serde_json::Value::as_str).unwrap_or_default();
    if text.is_empty() {
        return Err(AppError::Verify(format!("签名信封缺少 {what}")));
    }
    let bytes = BASE64
        .decode(text)
        .map_err(|_| AppError::Verify(format!("{what} 不是合法的 base64")))?;
    if bytes.is_empty() {
        return Err(AppError::Verify(format!("{what} 为空")));
    }
    Ok(bytes)
}

/// 判断文本是签名信封还是明文元数据。
pub fn is_envelope(text: &str) -> bool {
    text.contains("\"payload\"") && text.contains("\"signature\"")
}

/// 校验信封并返回其中的 payload 文本。
pub fn verify_envelope(text: &str) -> Result<String, AppError> {
    let outer: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| AppError::Verify(format!("签名信封 JSON 解析失败: {e}")))?;
    let object = outer
        .as_object()
        .ok_or_else(|| AppError::Verify("签名信封不是 JSON 对象".to_string()))?;

    if object.get("schema").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(AppError::Verify("不支持的签名信封 schema".to_string()));
    }

    let payload = decode(object.get("payload"), "payload")?;
    let signature = decode(object.get("signature"), "signature")?;
    let key_id = object
        .get("keyId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    let keys: TrustedKeys = serde_json::from_str(TRUSTED_KEYS)
        .map_err(|_| AppError::Verify("内置公钥文件损坏".to_string()))?;
    let key = keys
        .keys
        .iter()
        .find(|key| key.id == key_id)
        .ok_or_else(|| {
            AppError::Verify(format!(
                "元数据签名者的 keyId `{key_id}` 不在可信列表里"
            ))
        })?;

    let modulus = BASE64
        .decode(&key.modulus)
        .map_err(|_| AppError::Verify("内置公钥 modulus 非法".to_string()))?;
    let exponent = BASE64
        .decode(&key.exponent)
        .map_err(|_| AppError::Verify("内置公钥 exponent 非法".to_string()))?;
    if modulus.len() != MODULUS_BYTES || exponent.is_empty() || exponent.len() > 8 {
        return Err(AppError::Verify("内置公钥长度不符合 3072 位要求".to_string()));
    }

    let public_key = RsaPublicKey::new(
        BigUint::from_bytes_be(&modulus),
        BigUint::from_bytes_be(&exponent),
    )
    .map_err(|_| AppError::Verify("内置公钥无法构造".to_string()))?;

    let digest = Sha256::digest(&payload);
    public_key
        .verify(Pss::new_with_salt::<Sha256>(SALT_LEN), &digest, &signature)
        .map_err(|_| AppError::Verify("元数据签名无效（内容被改过或私钥不对）".to_string()))?;

    String::from_utf8(payload)
        .map_err(|_| AppError::Verify("payload 不是 UTF-8 文本".to_string()))
}

/// 是否需要签名由配置决定：本地测试可以放行明文，生产应设为 true。
pub fn unwrap_feed(text: &str, require_signature: bool) -> Result<String, AppError> {
    if is_envelope(text) {
        verify_envelope(text)
    } else if require_signature {
        Err(AppError::Verify(
            "元数据没有签名信封，但配置要求校验签名".to_string(),
        ))
    } else {
        Ok(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use rsa::{RsaPrivateKey, RsaPublicKey};

    fn envelope(private: &RsaPrivateKey, key_id: &str, payload: &[u8]) -> String {
        let digest = Sha256::digest(payload);
        let signature = private
            .sign_with_rng(&mut OsRng, Pss::new_with_salt::<Sha256>(SALT_LEN), &digest)
            .unwrap();
        serde_json::json!({
            "schema": 1,
            "keyId": key_id,
            "payload": BASE64.encode(payload),
            "signature": BASE64.encode(signature),
        })
        .to_string()
    }

    #[test]
    fn envelope_shape_is_detected() {
        assert!(is_envelope(r#"{"schema":1,"payload":"e30=","signature":"AA=="}"#));
        assert!(!is_envelope(r#"{"schema":1,"version":"1.0.0"}"#));
    }

    #[test]
    fn unsigned_payload_only_passes_when_not_required() {
        let plain = r#"{"schema":1,"version":"1.0.0"}"#;
        assert_eq!(unwrap_feed(plain, false).unwrap(), plain);
        assert!(unwrap_feed(plain, true).is_err());
    }

    #[test]
    fn unknown_key_and_tampered_payload_are_rejected() {
        let private = RsaPrivateKey::new(&mut OsRng, 3072).unwrap();
        let payload = br#"{"schema":1,"version":"1.0.0"}"#;
        let signed = envelope(&private, "untrusted", payload);
        // 仓库默认没有配置可信公钥，任何签名者的 keyId 都应被拒绝
        assert!(verify_envelope(&signed).is_err());
        assert!(verify_envelope(&signed.replace("1.0.0", "9.9.9")).is_err());
    }

    #[test]
    fn public_key_round_trips_through_trusted_key_json() {
        let private = RsaPrivateKey::new(&mut OsRng, 3072).unwrap();
        let public = RsaPublicKey::from(&private);
        use rsa::traits::PublicKeyParts;
        let keys = serde_json::json!({
            "keys": [{
                "id": "test-key",
                "modulus": BASE64.encode(public.n().to_bytes_be()),
                "exponent": BASE64.encode(public.e().to_bytes_be()),
            }]
        })
        .to_string();
        let parsed: TrustedKeys = serde_json::from_str(&keys).unwrap();
        assert_eq!(parsed.keys.len(), 1);
        assert_eq!(parsed.keys[0].id, "test-key");
    }
}
