pub fn mean_pooling(
    last_hidden_state: &[f32],
    attention_mask: &[i64],
    seq_length: usize,
    hidden_size: usize,
) -> Vec<f32> {
    let mut pooled = vec![0.0; hidden_size];
    let mut mask_sum = 0.0;

    for i in 0..seq_length {
        let mask = attention_mask[i] as f32;
        mask_sum += mask;
        for j in 0..hidden_size {
            pooled[j] += last_hidden_state[i * hidden_size + j] * mask;
        }
    }

    if mask_sum > 0.0 {
        for val in pooled.iter_mut() {
            *val /= mask_sum;
        }
    }

    pooled
}

pub fn l2_normalize(mut embeddings: Vec<f32>) -> Vec<f32> {
    let epsilon = 1e-12;
    let mut sum_sq = 0.0;

    for &val in embeddings.iter() {
        sum_sq += val * val;
    }

    let norm = (sum_sq.max(epsilon)).sqrt();

    for val in embeddings.iter_mut() {
        *val /= norm;
    }

    embeddings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mean_pooling() {
        let last_hidden_state = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let attention_mask = vec![1, 1, 0];
        let seq_length = 3;
        let hidden_size = 3;

        let pooled = mean_pooling(&last_hidden_state, &attention_mask, seq_length, hidden_size);
        assert_eq!(pooled.len(), 3);

        // Expected:
        // mask_sum = 2.0
        // sum[0] = 1.0 + 4.0 = 5.0 -> 2.5
        // sum[1] = 2.0 + 5.0 = 7.0 -> 3.5
        // sum[2] = 3.0 + 6.0 = 9.0 -> 4.5
        assert!((pooled[0] - 2.5).abs() < 1e-6);
        assert!((pooled[1] - 3.5).abs() < 1e-6);
        assert!((pooled[2] - 4.5).abs() < 1e-6);
    }

    #[test]
    fn test_mean_pooling_all_masked() {
        let last_hidden_state = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let attention_mask = vec![0, 0];
        let seq_length = 2;
        let hidden_size = 3;

        let pooled = mean_pooling(&last_hidden_state, &attention_mask, seq_length, hidden_size);
        assert_eq!(pooled, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_l2_normalize() {
        let embeddings = vec![3.0, 4.0];
        let normalized = l2_normalize(embeddings);

        // sum_sq = 9.0 + 16.0 = 25.0, norm = 5.0
        // normalized = [3.0/5.0, 4.0/5.0] = [0.6, 0.8]
        assert!((normalized[0] - 0.6).abs() < 1e-6);
        assert!((normalized[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_l2_normalize_zero() {
        let embeddings = vec![0.0, 0.0];
        let normalized = l2_normalize(embeddings);

        // sum_sq = 0.0, norm = sqrt(1e-12) = 1e-6
        // normalized = [0.0, 0.0]
        assert_eq!(normalized, vec![0.0, 0.0]);
    }
}
