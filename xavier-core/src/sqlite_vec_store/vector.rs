use crate::utils::errors::XavierError;

const QJL_MAGIC: &[u8] = &[0x71, 0x4a, 0x4c, 0x01]; // "qJL\x01"

pub fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn deserialize_embedding(data: &[u8]) -> Vec<f32> {
    if data.len() >= 16 && &data[..4] == QJL_MAGIC {
        let dims = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let scale_1 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let scale_2 = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let expected_len = 16 + (dims * 2);
        if data.len() >= expected_len {
            let coarse = &data[16..16 + dims];
            let residual = &data[16 + dims..expected_len];
            return coarse
                .iter()
                .zip(residual.iter())
                .map(|(coarse, residual)| {
                    let coarse = *coarse as i8 as f32;
                    let residual = *residual as i8 as f32;
                    (coarse * scale_1) + (residual * scale_2)
                })
                .collect();
        }
    }

    deserialize_embedding_bytes(data)
}

#[allow(clippy::incompatible_msrv)] // as_chunks stabilized in rust 1.88
fn deserialize_embedding_bytes(data: &[u8]) -> Vec<f32> {
    data.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}