//! Vector operations for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::sqlite_vec_store::config::QJL_MAGIC;
use anyhow::Result;

/// Register sqlite vec extension.
pub fn register_sqlite_vec_extension() -> Result<()> {
    unsafe {
        #[cfg(target_os = "android")]
        type CharPtr = u8;
        #[cfg(not(target_os = "android"))]
        type CharPtr = i8;

        let init = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut CharPtr,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    }
    Ok(())
}

/// Serialize embedding.
pub fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Serialize embedding qjl.
pub fn serialize_embedding_qjl(embedding: &[f32]) -> Vec<u8> {
    let dims = embedding.len() as u32;
    let max_abs = embedding
        .iter()
        .fold(0.0_f32, |acc, value| acc.max(value.abs()));
    let scale_1 = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
    let coarse: Vec<i8> = embedding
        .iter()
        .map(|value| ((value / scale_1).round().clamp(-127.0, 127.0)) as i8)
        .collect();
    let residuals: Vec<f32> = embedding
        .iter()
        .zip(coarse.iter())
        .map(|(value, quantized)| value - (*quantized as f32 * scale_1))
        .collect();
    let residual_max = residuals
        .iter()
        .fold(0.0_f32, |acc, value| acc.max(value.abs()));
    let scale_2 = if residual_max > 0.0 {
        residual_max / 127.0
    } else {
        1.0
    };
    let residual_quantized: Vec<i8> = residuals
        .iter()
        .map(|value| ((value / scale_2).round().clamp(-127.0, 127.0)) as i8)
        .collect();

    let mut bytes = Vec::with_capacity(16 + (embedding.len() * 2));
    bytes.extend_from_slice(QJL_MAGIC);
    bytes.extend_from_slice(&dims.to_le_bytes());
    bytes.extend_from_slice(&scale_1.to_le_bytes());
    bytes.extend_from_slice(&scale_2.to_le_bytes());
    bytes.extend(coarse.into_iter().map(|value| value as u8));
    bytes.extend(residual_quantized.into_iter().map(|value| value as u8));
    bytes
}

/// Deserialize embedding.
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

    data.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}
