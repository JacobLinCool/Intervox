//! PCM conversions. OpenAI Realtime expects mono PCM16 little-endian at
//! 24 kHz, base64 over the websocket (spec §6.2). Internally we work in f32.

use base64::Engine;

const I16_MAX_F: f32 = 32767.0;

pub fn f32_to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * I16_MAX_F).round() as i16
        })
        .collect()
}

pub fn pcm16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32 / I16_MAX_F).collect()
}

pub fn pcm16_to_le_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn le_bytes_to_pcm16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

pub fn pcm16_to_base64(samples: &[i16]) -> String {
    base64::engine::general_purpose::STANDARD.encode(pcm16_to_le_bytes(samples))
}

pub fn base64_to_pcm16(b64: &str) -> Result<Vec<i16>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    Ok(le_bytes_to_pcm16(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_clamps_to_i16_max() {
        assert_eq!(
            f32_to_pcm16(&[1.0, -1.0, 2.0, -2.0]),
            vec![32767, -32767, 32767, -32767]
        );
    }

    #[test]
    fn f32_pcm16_round_trips_within_one_lsb() {
        let input: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0) * 2.0 - 1.0).collect();
        let back = pcm16_to_f32(&f32_to_pcm16(&input));
        for (a, b) in input.iter().zip(back.iter()) {
            assert!((a - b).abs() <= 1.0 / I16_MAX_F + 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn little_endian_byte_order() {
        // 0x0102 -> [0x02, 0x01]
        assert_eq!(pcm16_to_le_bytes(&[0x0102]), vec![0x02, 0x01]);
        assert_eq!(le_bytes_to_pcm16(&[0x02, 0x01]), vec![0x0102]);
    }

    #[test]
    fn base64_round_trips() {
        let pcm = vec![0i16, 1, -1, 12345, -12345, 32767, -32767];
        let b64 = pcm16_to_base64(&pcm);
        assert_eq!(base64_to_pcm16(&b64).unwrap(), pcm);
    }
}
