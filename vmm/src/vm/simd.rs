#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

// ============================================================================
// SIMD Float Operations
// ============================================================================

pub fn simd_vadd_f(a: &[f64], b: &[f64]) -> Vec<f64> {
    let len = a.len().min(b.len());
    let mut out = vec![0.0f64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            while i + 4 <= len {
                unsafe {
                    let va = _mm256_loadu_pd(a.as_ptr().add(i));
                    let vb = _mm256_loadu_pd(b.as_ptr().add(i));
                    let vr = _mm256_add_pd(va, vb);
                    _mm256_storeu_pd(out.as_mut_ptr().add(i), vr);
                }
                i += 4;
            }
        }
    }

    while i < len {
        out[i] = a[i] + b[i];
        i += 1;
    }
    out
}

pub fn simd_vsub_f(a: &[f64], b: &[f64]) -> Vec<f64> {
    let len = a.len().min(b.len());
    let mut out = vec![0.0f64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            while i + 4 <= len {
                unsafe {
                    let va = _mm256_loadu_pd(a.as_ptr().add(i));
                    let vb = _mm256_loadu_pd(b.as_ptr().add(i));
                    let vr = _mm256_sub_pd(va, vb);
                    _mm256_storeu_pd(out.as_mut_ptr().add(i), vr);
                }
                i += 4;
            }
        }
    }

    while i < len {
        out[i] = a[i] - b[i];
        i += 1;
    }
    out
}

pub fn simd_vmul_f(a: &[f64], b: &[f64]) -> Vec<f64> {
    let len = a.len().min(b.len());
    let mut out = vec![0.0f64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            while i + 4 <= len {
                unsafe {
                    let va = _mm256_loadu_pd(a.as_ptr().add(i));
                    let vb = _mm256_loadu_pd(b.as_ptr().add(i));
                    let vr = _mm256_mul_pd(va, vb);
                    _mm256_storeu_pd(out.as_mut_ptr().add(i), vr);
                }
                i += 4;
            }
        }
    }

    while i < len {
        out[i] = a[i] * b[i];
        i += 1;
    }
    out
}

pub fn simd_vdot_f(a: &[f64], b: &[f64]) -> f64 {
    let len = a.len().min(b.len());
    let mut i = 0;
    let mut sum = 0.0f64;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            unsafe {
                let mut vsum = _mm256_setzero_pd();
                while i + 4 <= len {
                    let va = _mm256_loadu_pd(a.as_ptr().add(i));
                    let vb = _mm256_loadu_pd(b.as_ptr().add(i));
                    vsum = _mm256_fmadd_pd(va, vb, vsum);
                    i += 4;
                }
                let mut temp = [0.0f64; 4];
                _mm256_storeu_pd(temp.as_mut_ptr(), vsum);
                sum += temp[0] + temp[1] + temp[2] + temp[3];
            }
        }
    }

    while i < len {
        sum += a[i] * b[i];
        i += 1;
    }
    sum
}

pub fn simd_vsum_f(a: &[f64]) -> f64 {
    let len = a.len();
    let mut i = 0;
    let mut sum = 0.0f64;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let mut vsum = _mm256_setzero_pd();
                while i + 4 <= len {
                    let va = _mm256_loadu_pd(a.as_ptr().add(i));
                    vsum = _mm256_add_pd(vsum, va);
                    i += 4;
                }
                let mut temp = [0.0f64; 4];
                _mm256_storeu_pd(temp.as_mut_ptr(), vsum);
                sum += temp[0] + temp[1] + temp[2] + temp[3];
            }
        }
    }

    while i < len {
        sum += a[i];
        i += 1;
    }
    sum
}

pub fn simd_vdiv_f(a: &[f64], b: &[f64]) -> Vec<f64> {
    let len = a.len().min(b.len());
    let mut out = vec![0.0f64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            while i + 4 <= len {
                unsafe {
                    let va = _mm256_loadu_pd(a.as_ptr().add(i));
                    let vb = _mm256_loadu_pd(b.as_ptr().add(i));
                    let vr = _mm256_div_pd(va, vb);
                    _mm256_storeu_pd(out.as_mut_ptr().add(i), vr);
                }
                i += 4;
            }
        }
    }

    while i < len {
        out[i] = if b[i] == 0.0 { 0.0 } else { a[i] / b[i] };
        i += 1;
    }
    out
}

pub fn simd_vsplat_f(val: f64, len: usize) -> Vec<f64> {
    vec![val; len]
}

// ============================================================================
// SIMD Integer Operations
// ============================================================================

pub fn simd_vadd_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            while i + 4 <= len {
                unsafe {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
                    let vr = _mm256_add_epi64(va, vb);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, vr);
                }
                i += 4;
            }
        }
    }

    while i < len {
        out[i] = a[i].wrapping_add(b[i]);
        i += 1;
    }
    out
}

pub fn simd_vsub_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            while i + 4 <= len {
                unsafe {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
                    let vr = _mm256_sub_epi64(va, vb);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, vr);
                }
                i += 4;
            }
        }
    }

    while i < len {
        out[i] = a[i].wrapping_sub(b[i]);
        i += 1;
    }
    out
}

pub fn simd_vmul_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    for i in 0..len {
        out[i] = a[i].wrapping_mul(b[i]);
    }
    out
}

pub fn simd_vsum_i(a: &[i64]) -> i64 {
    let len = a.len();
    let mut i = 0;
    let mut sum = 0i64;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let mut vsum = _mm256_setzero_si256();
                while i + 4 <= len {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    vsum = _mm256_add_epi64(vsum, va);
                    i += 4;
                }
                let mut temp = [0i64; 4];
                _mm256_storeu_si256(temp.as_mut_ptr() as *mut __m256i, vsum);
                sum = sum.wrapping_add(temp[0]).wrapping_add(temp[1]).wrapping_add(temp[2]).wrapping_add(temp[3]);
            }
        }
    }

    while i < len {
        sum = sum.wrapping_add(a[i]);
        i += 1;
    }
    sum
}

pub fn simd_viota_i(start: i64, len: usize) -> Vec<i64> {
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let v_step = _mm256_set1_epi64x(4);
                let mut v_cur = _mm256_set_epi64x(
                    start.wrapping_add(3),
                    start.wrapping_add(2),
                    start.wrapping_add(1),
                    start,
                );
                while i + 4 <= len {
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, v_cur);
                    v_cur = _mm256_add_epi64(v_cur, v_step);
                    i += 4;
                }
            }
        }
    }

    while i < len {
        out[i] = start.wrapping_add(i as i64);
        i += 1;
    }
    out
}

pub fn simd_vsplat_i(val: i64, len: usize) -> Vec<i64> {
    vec![val; len]
}

pub fn simd_vadd_ic(a: &[i64], c: i64) -> Vec<i64> {
    let len = a.len();
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let vc = _mm256_set1_epi64x(c);
                while i + 4 <= len {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    let vr = _mm256_add_epi64(va, vc);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, vr);
                    i += 4;
                }
            }
        }
    }

    while i < len {
        out[i] = a[i].wrapping_add(c);
        i += 1;
    }
    out
}

pub fn simd_vsub_ic(a: &[i64], c: i64) -> Vec<i64> {
    let len = a.len();
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let vc = _mm256_set1_epi64x(c);
                while i + 4 <= len {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    let vr = _mm256_sub_epi64(va, vc);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, vr);
                    i += 4;
                }
            }
        }
    }

    while i < len {
        out[i] = a[i].wrapping_sub(c);
        i += 1;
    }
    out
}

pub fn simd_vmul_ic(a: &[i64], c: i64) -> Vec<i64> {
    let len = a.len();
    let mut out = vec![0i64; len];
    for i in 0..len {
        out[i] = a[i].wrapping_mul(c);
    }
    out
}

pub fn simd_vdiv_ic(a: &[i64], c: i64) -> Vec<i64> {
    let len = a.len();
    let mut out = vec![0i64; len];
    if c == 0 {
        return out;
    }
    for i in 0..len {
        out[i] = a[i] / c;
    }
    out
}

pub fn simd_vrem_ic(a: &[i64], c: i64) -> Vec<i64> {
    let len = a.len();
    let mut out = vec![0i64; len];
    if c == 0 {
        return out;
    }
    for i in 0..len {
        out[i] = a[i] % c;
    }
    out
}

pub fn simd_vdiv_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    for i in 0..len {
        out[i] = if b[i] == 0 { 0 } else { a[i] / b[i] };
    }
    out
}

pub fn simd_vrem_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    for i in 0..len {
        out[i] = if b[i] == 0 { 0 } else { a[i] % b[i] };
    }
    out
}

pub fn simd_vgather_i(table: &[i64], indices: &[i64]) -> Vec<i64> {
    let len = indices.len();
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && !table.is_empty() {
            unsafe {
                while i + 4 <= len {
                    let v_idx = _mm256_loadu_si256(indices.as_ptr().add(i) as *const __m256i);
                    let gathered = _mm256_i64gather_epi64(table.as_ptr(), v_idx, 8);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, gathered);
                    i += 4;
                }
            }
        }
    }

    while i < len {
        let idx = indices[i];
        if idx >= 0 && (idx as usize) < table.len() {
            out[i] = table[idx as usize];
        }
        i += 1;
    }
    out
}

pub fn simd_vcmp_eq_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let v_one = _mm256_set1_epi64x(1);
                while i + 4 <= len {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
                    let eq = _mm256_cmpeq_epi64(va, vb);
                    let res = _mm256_and_si256(eq, v_one);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, res);
                    i += 4;
                }
            }
        }
    }

    while i < len {
        out[i] = if a[i] == b[i] { 1 } else { 0 };
        i += 1;
    }
    out
}

pub fn simd_vcmp_gt_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    let len = a.len().min(b.len());
    let mut out = vec![0i64; len];
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let v_one = _mm256_set1_epi64x(1);
                while i + 4 <= len {
                    let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                    let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
                    let gt = _mm256_cmpgt_epi64(va, vb);
                    let res = _mm256_and_si256(gt, v_one);
                    _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, res);
                    i += 4;
                }
            }
        }
    }

    while i < len {
        out[i] = if a[i] > b[i] { 1 } else { 0 };
        i += 1;
    }
    out
}

pub fn simd_vcmp_lt_i(a: &[i64], b: &[i64]) -> Vec<i64> {
    simd_vcmp_gt_i(b, a)
}

pub fn simd_vfilter_i(values: &[i64], mask: &[i64]) -> Vec<i64> {
    let len = values.len().min(mask.len());
    let mut out = Vec::new();
    for i in 0..len {
        if mask[i] != 0 {
            out.push(values[i]);
        }
    }
    out
}

// ============================================================================
// SIMD String Operations
// ============================================================================

pub fn simd_str_count_byte(s: &str, byte: u8) -> usize {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut count = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512bw") {
            unsafe {
                let target_vec = _mm512_set1_epi8(byte as i8);
                while i + 64 <= len {
                    let chunk = _mm512_loadu_si512(bytes.as_ptr().add(i) as *const _);
                    let mask = _mm512_cmpeq_epi8_mask(chunk, target_vec);
                    count += mask.count_ones() as usize;
                    i += 64;
                }
            }
        } else if is_x86_feature_detected!("avx2") {
            unsafe {
                let target_vec = _mm256_set1_epi8(byte as i8);
                while i + 32 <= len {
                    let chunk = _mm256_loadu_si256(bytes.as_ptr().add(i) as *const __m256i);
                    let eq = _mm256_cmpeq_epi8(chunk, target_vec);
                    let mask = _mm256_movemask_epi8(eq) as u32;
                    count += mask.count_ones() as usize;
                    i += 32;
                }
            }
        }
    }

    while i < len {
        if bytes[i] == byte {
            count += 1;
        }
        i += 1;
    }
    count
}

pub fn simd_str_to_upper_inplace(bytes: &mut [u8]) {
    let len = bytes.len();
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512bw") {
            unsafe {
                let a = _mm512_set1_epi8(b'a' as i8);
                let z = _mm512_set1_epi8(b'z' as i8);
                let diff = _mm512_set1_epi8(32);

                while i + 64 <= len {
                    let ptr = bytes.as_mut_ptr().add(i);
                    let chunk = _mm512_loadu_si512(ptr as *const _);
                    let mask = _mm512_cmple_epu8_mask(a, chunk) & _mm512_cmple_epu8_mask(chunk, z);
                    if mask != 0 {
                        let upper = _mm512_mask_sub_epi8(chunk, mask, chunk, diff);
                        _mm512_storeu_si512(ptr as *mut _, upper);
                    }
                    i += 64;
                }
            }
        } else if is_x86_feature_detected!("avx2") {
            unsafe {
                let a_sub_128 = _mm256_set1_epi8((b'a' as i8).wrapping_sub(-128));
                let z_sub_128 = _mm256_set1_epi8((b'z' as i8).wrapping_sub(-128));
                let sub_128 = _mm256_set1_epi8(-128);
                let diff = _mm256_set1_epi8(32);

                while i + 32 <= len {
                    let ptr = bytes.as_mut_ptr().add(i);
                    let chunk = _mm256_loadu_si256(ptr as *const __m256i);
                    let signed_chunk = _mm256_add_epi8(chunk, sub_128);
                    let ge_a = _mm256_cmpgt_epi8(signed_chunk, _mm256_sub_epi8(a_sub_128, _mm256_set1_epi8(1)));
                    let le_z = _mm256_cmpgt_epi8(_mm256_add_epi8(z_sub_128, _mm256_set1_epi8(1)), signed_chunk);
                    let is_lower = _mm256_and_si256(ge_a, le_z);
                    if _mm256_movemask_epi8(is_lower) != 0 {
                        let to_sub = _mm256_and_si256(is_lower, diff);
                        let upper = _mm256_sub_epi8(chunk, to_sub);
                        _mm256_storeu_si256(ptr as *mut __m256i, upper);
                    }
                    i += 32;
                }
            }
        }
    }

    while i < len {
        let b = bytes[i];
        if b >= b'a' && b <= b'z' {
            bytes[i] = b - 32;
        }
        i += 1;
    }
}

pub fn simd_str_to_lower_inplace(bytes: &mut [u8]) {
    let len = bytes.len();
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512bw") {
            unsafe {
                let a = _mm512_set1_epi8(b'A' as i8);
                let z = _mm512_set1_epi8(b'Z' as i8);
                let diff = _mm512_set1_epi8(32);

                while i + 64 <= len {
                    let ptr = bytes.as_mut_ptr().add(i);
                    let chunk = _mm512_loadu_si512(ptr as *const _);
                    let mask = _mm512_cmple_epu8_mask(a, chunk) & _mm512_cmple_epu8_mask(chunk, z);
                    if mask != 0 {
                        let lower = _mm512_mask_add_epi8(chunk, mask, chunk, diff);
                        _mm512_storeu_si512(ptr as *mut _, lower);
                    }
                    i += 64;
                }
            }
        } else if is_x86_feature_detected!("avx2") {
            unsafe {
                let a_sub_128 = _mm256_set1_epi8((b'A' as i8).wrapping_sub(-128));
                let z_sub_128 = _mm256_set1_epi8((b'Z' as i8).wrapping_sub(-128));
                let sub_128 = _mm256_set1_epi8(-128);
                let diff = _mm256_set1_epi8(32);

                while i + 32 <= len {
                    let ptr = bytes.as_mut_ptr().add(i);
                    let chunk = _mm256_loadu_si256(ptr as *const __m256i);
                    let signed_chunk = _mm256_add_epi8(chunk, sub_128);
                    let ge_a = _mm256_cmpgt_epi8(signed_chunk, _mm256_sub_epi8(a_sub_128, _mm256_set1_epi8(1)));
                    let le_z = _mm256_cmpgt_epi8(_mm256_add_epi8(z_sub_128, _mm256_set1_epi8(1)), signed_chunk);
                    let is_upper = _mm256_and_si256(ge_a, le_z);
                    if _mm256_movemask_epi8(is_upper) != 0 {
                        let to_add = _mm256_and_si256(is_upper, diff);
                        let lower = _mm256_add_epi8(chunk, to_add);
                        _mm256_storeu_si256(ptr as *mut __m256i, lower);
                    }
                    i += 32;
                }
            }
        }
    }

    while i < len {
        let b = bytes[i];
        if b >= b'A' && b <= b'Z' {
            bytes[i] = b + 32;
        }
        i += 1;
    }
}

pub fn simd_str_to_upper_mut(s: &mut String) {
    unsafe {
        simd_str_to_upper_inplace(s.as_mut_vec());
    }
}

pub fn simd_str_to_lower_mut(s: &mut String) {
    unsafe {
        simd_str_to_lower_inplace(s.as_mut_vec());
    }
}

pub fn simd_str_to_upper(s: &str) -> String {
    let mut vec = s.as_bytes().to_vec();
    simd_str_to_upper_inplace(&mut vec);
    unsafe { String::from_utf8_unchecked(vec) }
}

pub fn simd_str_to_lower(s: &str) -> String {
    let mut vec = s.as_bytes().to_vec();
    simd_str_to_lower_inplace(&mut vec);
    unsafe { String::from_utf8_unchecked(vec) }
}

pub fn simd_str_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let ba = a.as_bytes();
    let bb = b.as_bytes();
    let len = ba.len();
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512bw") {
            unsafe {
                while i + 64 <= len {
                    let ca = _mm512_loadu_si512(ba.as_ptr().add(i) as *const _);
                    let cb = _mm512_loadu_si512(bb.as_ptr().add(i) as *const _);
                    let mask = _mm512_cmpeq_epi8_mask(ca, cb);
                    if mask != 0xFFFFFFFFFFFFFFFF {
                        return false;
                    }
                    i += 64;
                }
            }
        } else if is_x86_feature_detected!("avx2") {
            unsafe {
                while i + 32 <= len {
                    let ca = _mm256_loadu_si256(ba.as_ptr().add(i) as *const __m256i);
                    let cb = _mm256_loadu_si256(bb.as_ptr().add(i) as *const __m256i);
                    let eq = _mm256_cmpeq_epi8(ca, cb);
                    let mask = _mm256_movemask_epi8(eq) as u32;
                    if mask != 0xFFFFFFFF {
                        return false;
                    }
                    i += 32;
                }
            }
        }
    }

    ba[i..] == bb[i..]
}

pub fn simd_str_starts_with(haystack: &str, prefix: &str) -> bool {
    if prefix.len() > haystack.len() {
        return false;
    }
    simd_str_eq(&haystack[..prefix.len()], prefix)
}

pub fn simd_str_ends_with(haystack: &str, suffix: &str) -> bool {
    if suffix.len() > haystack.len() {
        return false;
    }
    simd_str_eq(&haystack[haystack.len() - suffix.len()..], suffix)
}

pub fn simd_str_find(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    if n.len() > h.len() {
        return None;
    }
    if n.len() == 1 {
        // Fast byte search via AVX2
        let target = n[0];
        let len = h.len();
        let mut i = 0;
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") {
                unsafe {
                    let target_vec = _mm256_set1_epi8(target as i8);
                    while i + 32 <= len {
                        let chunk = _mm256_loadu_si256(h.as_ptr().add(i) as *const __m256i);
                        let eq = _mm256_cmpeq_epi8(chunk, target_vec);
                        let mask = _mm256_movemask_epi8(eq) as u32;
                        if mask != 0 {
                            return Some(i + mask.trailing_zeros() as usize);
                        }
                        i += 32;
                    }
                }
            }
        }
        while i < len {
            if h[i] == target {
                return Some(i);
            }
            i += 1;
        }
        return None;
    }

    // Two-byte vectorized probe (first and last byte)
    let first = n[0];
    let last = n[n.len() - 1];
    let last_offset = n.len() - 1;
    let end = h.len() - n.len();
    let mut i = 0;

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let first_vec = _mm256_set1_epi8(first as i8);
                let last_vec = _mm256_set1_epi8(last as i8);
                while i + 32 <= end {
                    let chunk_first = _mm256_loadu_si256(h.as_ptr().add(i) as *const __m256i);
                    let chunk_last = _mm256_loadu_si256(h.as_ptr().add(i + last_offset) as *const __m256i);
                    let eq_first = _mm256_cmpeq_epi8(chunk_first, first_vec);
                    let eq_last = _mm256_cmpeq_epi8(chunk_last, last_vec);
                    let mut mask = (_mm256_movemask_epi8(_mm256_and_si256(eq_first, eq_last))) as u32;

                    while mask != 0 {
                        let bit = mask.trailing_zeros() as usize;
                        let cand = i + bit;
                        if &h[cand..cand + n.len()] == n {
                            return Some(cand);
                        }
                        mask &= mask - 1;
                    }
                    i += 32;
                }
            }
        }
    }

    while i <= end {
        if h[i] == first && h[i + last_offset] == last && &h[i..i + n.len()] == n {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ============================================================================
// Direct SIMD Munchausen Range
// ============================================================================

const TABLE_DATA: [u32; 16] = [
    0, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489,
    0, 0, 0, 0, 0, 0
];

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn check_simd_range_avx512(start: u32, end: u32) -> Vec<i64> {
    unsafe {
        let mut matches = Vec::new();
        let table = _mm512_loadu_si512(TABLE_DATA.as_ptr() as *const _);
        let offsets = _mm512_set_epi32(15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0);
        let v_zero = _mm512_setzero_si512();
        let v_magic = _mm512_set1_epi64(0xCCCCCCCDi64);

        let mut base = start;
        while base + 16 <= end {
            let v_base = _mm512_set1_epi32(base as i32);
            let orig = _mm512_add_epi32(v_base, offsets);
            let mut n = orig;
            let mut sum = _mm512_setzero_si512();

            for _ in 0..7 {
                let n_even = n;
                let prod_even = _mm512_mul_epu32(n_even, v_magic);
                let q_even = _mm512_srli_epi64(prod_even, 35);

                let n_odd = _mm512_srli_epi64(n, 32);
                let prod_odd = _mm512_mul_epu32(n_odd, v_magic);
                let q_odd = _mm512_srli_epi64(prod_odd, 35);

                let q_odd_shifted = _mm512_slli_epi64(q_odd, 32);
                let q = _mm512_or_si512(q_even, q_odd_shifted);

                let q_mul_10 = _mm512_add_epi32(_mm512_slli_epi32(q, 3), _mm512_slli_epi32(q, 1));
                let rem = _mm512_sub_epi32(n, q_mul_10);

                let p = _mm512_permutexvar_epi32(rem, table);
                sum = _mm512_add_epi32(sum, p);
                n = q;
            }

            let eq_mask = _mm512_cmpeq_epi32_mask(sum, orig);
            let gt_zero_mask = _mm512_cmpgt_epi32_mask(orig, v_zero);
            let mask = eq_mask & gt_zero_mask;

            if mask != 0 {
                for i in 0..16 {
                    if (mask & (1 << i)) != 0 {
                        matches.push((base + i) as i64);
                    }
                }
            }

            base += 16;
        }

        for n in base..end {
            if n == 0 {
                continue;
            }
            let mut cur = n;
            let mut total = 0u32;
            while cur > 0 {
                let d = (cur % 10) as usize;
                total += TABLE_DATA[d];
                cur /= 10;
            }
            if total == n {
                matches.push(n as i64);
            }
        }

        matches
    }
}

pub fn simd_munchausen_range(start: i64, end: i64) -> Vec<i64> {
    let s = start.max(0) as u32;
    let e = end.max(0) as u32;
    if s >= e {
        return Vec::new();
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f") {
            return unsafe { check_simd_range_avx512(s, e) };
        }
    }

    let mut matches = Vec::new();
    for n in s..e {
        if n == 0 {
            continue;
        }
        let mut cur = n;
        let mut total = 0u32;
        while cur > 0 {
            let d = (cur % 10) as usize;
            total += TABLE_DATA[d];
            cur /= 10;
        }
        if total == n {
            matches.push(n as i64);
        }
    }
    matches
}

// ============================================================================
// Fixed 4-Lane Stack SIMD Operations (Zero Heap Allocation)
// ============================================================================

#[inline(always)]
pub fn simd_v4add_f(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_pd(a.as_ptr());
                let vb = _mm256_loadu_pd(b.as_ptr());
                let vr = _mm256_add_pd(va, vb);
                let mut out = [0.0f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), vr);
                return out;
            }
        }
    }
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

#[inline(always)]
pub fn simd_v4sub_f(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_pd(a.as_ptr());
                let vb = _mm256_loadu_pd(b.as_ptr());
                let vr = _mm256_sub_pd(va, vb);
                let mut out = [0.0f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), vr);
                return out;
            }
        }
    }
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

#[inline(always)]
pub fn simd_v4mul_f(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_pd(a.as_ptr());
                let vb = _mm256_loadu_pd(b.as_ptr());
                let vr = _mm256_mul_pd(va, vb);
                let mut out = [0.0f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), vr);
                return out;
            }
        }
    }
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

#[inline(always)]
pub fn simd_v4div_f(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_pd(a.as_ptr());
                let vb = _mm256_loadu_pd(b.as_ptr());
                let vr = _mm256_div_pd(va, vb);
                let mut out = [0.0f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), vr);
                return out;
            }
        }
    }
    [a[0] / b[0], a[1] / b[1], a[2] / b[2], a[3] / b[3]]
}

#[inline(always)]
pub fn simd_v4dot_f(a: [f64; 4], b: [f64; 4]) -> f64 {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_pd(a.as_ptr());
                let vb = _mm256_loadu_pd(b.as_ptr());
                let prod = _mm256_mul_pd(va, vb);
                let mut out = [0.0f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), prod);
                return out[0] + out[1] + out[2] + out[3];
            }
        }
    }
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

#[inline(always)]
pub fn simd_v4sum_f(a: [f64; 4]) -> f64 {
    a[0] + a[1] + a[2] + a[3]
}

#[inline(always)]
pub fn simd_v4add_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_si256(a.as_ptr() as *const __m256i);
                let vb = _mm256_loadu_si256(b.as_ptr() as *const __m256i);
                let vr = _mm256_add_epi64(va, vb);
                let mut out = [0i64; 4];
                _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, vr);
                return out;
            }
        }
    }
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
        a[3].wrapping_add(b[3]),
    ]
}

#[inline(always)]
pub fn simd_v4sub_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_si256(a.as_ptr() as *const __m256i);
                let vb = _mm256_loadu_si256(b.as_ptr() as *const __m256i);
                let vr = _mm256_sub_epi64(va, vb);
                let mut out = [0i64; 4];
                _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, vr);
                return out;
            }
        }
    }
    [
        a[0].wrapping_sub(b[0]),
        a[1].wrapping_sub(b[1]),
        a[2].wrapping_sub(b[2]),
        a[3].wrapping_sub(b[3]),
    ]
}

#[inline(always)]
pub fn simd_v4mul_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        a[0].wrapping_mul(b[0]),
        a[1].wrapping_mul(b[1]),
        a[2].wrapping_mul(b[2]),
        a[3].wrapping_mul(b[3]),
    ]
}

#[inline(always)]
pub fn simd_v4div_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        if b[0] != 0 { a[0] / b[0] } else { 0 },
        if b[1] != 0 { a[1] / b[1] } else { 0 },
        if b[2] != 0 { a[2] / b[2] } else { 0 },
        if b[3] != 0 { a[3] / b[3] } else { 0 },
    ]
}

#[inline(always)]
pub fn simd_v4rem_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        if b[0] != 0 { a[0] % b[0] } else { 0 },
        if b[1] != 0 { a[1] % b[1] } else { 0 },
        if b[2] != 0 { a[2] % b[2] } else { 0 },
        if b[3] != 0 { a[3] % b[3] } else { 0 },
    ]
}

#[inline(always)]
pub fn simd_v4sum_i(a: [i64; 4]) -> i64 {
    a[0].wrapping_add(a[1]).wrapping_add(a[2]).wrapping_add(a[3])
}

#[inline(always)]
pub fn simd_v4add_ic(a: [i64; 4], c: i64) -> [i64; 4] {
    simd_v4add_i(a, [c, c, c, c])
}

#[inline(always)]
pub fn simd_v4sub_ic(a: [i64; 4], c: i64) -> [i64; 4] {
    simd_v4sub_i(a, [c, c, c, c])
}

#[inline(always)]
pub fn simd_v4mul_ic(a: [i64; 4], c: i64) -> [i64; 4] {
    simd_v4mul_i(a, [c, c, c, c])
}

#[inline(always)]
pub fn simd_v4div_ic(a: [i64; 4], c: i64) -> [i64; 4] {
    if c == 0 {
        return [0; 4];
    }
    [a[0] / c, a[1] / c, a[2] / c, a[3] / c]
}

#[inline(always)]
pub fn simd_v4rem_ic(a: [i64; 4], c: i64) -> [i64; 4] {
    if c == 0 {
        return [0; 4];
    }
    [a[0] % c, a[1] % c, a[2] % c, a[3] % c]
}

#[inline(always)]
pub fn simd_v4gather_i(table: &[i64], indices: [i64; 4]) -> [i64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            let t_len = table.len() as i64;
            if indices[0] >= 0 && indices[0] < t_len
                && indices[1] >= 0 && indices[1] < t_len
                && indices[2] >= 0 && indices[2] < t_len
                && indices[3] >= 0 && indices[3] < t_len
            {
                unsafe {
                    let vidx = _mm256_loadu_si256(indices.as_ptr() as *const __m256i);
                    let vr = _mm256_i64gather_epi64(table.as_ptr(), vidx, 8);
                    let mut out = [0i64; 4];
                    _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, vr);
                    return out;
                }
            }
        }
    }
    let t_len = table.len() as i64;
    [
        if indices[0] >= 0 && indices[0] < t_len { table[indices[0] as usize] } else { 0 },
        if indices[1] >= 0 && indices[1] < t_len { table[indices[1] as usize] } else { 0 },
        if indices[2] >= 0 && indices[2] < t_len { table[indices[2] as usize] } else { 0 },
        if indices[3] >= 0 && indices[3] < t_len { table[indices[3] as usize] } else { 0 },
    ]
}

#[inline(always)]
pub fn simd_v4cmp_eq_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_si256(a.as_ptr() as *const __m256i);
                let vb = _mm256_loadu_si256(b.as_ptr() as *const __m256i);
                let vr = _mm256_cmpeq_epi64(va, vb);
                let mut out = [0i64; 4];
                _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, vr);
                return out;
            }
        }
    }
    [
        if a[0] == b[0] { -1 } else { 0 },
        if a[1] == b[1] { -1 } else { 0 },
        if a[2] == b[2] { -1 } else { 0 },
        if a[3] == b[3] { -1 } else { 0 },
    ]
}

#[inline(always)]
pub fn simd_v4cmp_gt_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                let va = _mm256_loadu_si256(a.as_ptr() as *const __m256i);
                let vb = _mm256_loadu_si256(b.as_ptr() as *const __m256i);
                let vr = _mm256_cmpgt_epi64(va, vb);
                let mut out = [0i64; 4];
                _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, vr);
                return out;
            }
        }
    }
    [
        if a[0] > b[0] { -1 } else { 0 },
        if a[1] > b[1] { -1 } else { 0 },
        if a[2] > b[2] { -1 } else { 0 },
        if a[3] > b[3] { -1 } else { 0 },
    ]
}

#[inline(always)]
pub fn simd_v4cmp_lt_i(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    simd_v4cmp_gt_i(b, a)
}

#[inline(always)]
pub fn simd_v4filter_i(values: [i64; 4], mask: [i64; 4]) -> Vec<i64> {
    let mut out = Vec::with_capacity(4);
    for i in 0..4 {
        if mask[i] != 0 {
            out.push(values[i]);
        }
    }
    out
}

#[inline(always)]
pub fn simd_v4select_i(
    mask: [i64; 4],
    on_true: [i64; 4],
    on_false: [i64; 4],
) -> [i64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                // AVX blend instructions inspect the sign bit of each lane.
                // Normalize the VM's 0/1 or other nonzero masks to 0/-1.
                let vm = _mm256_set_epi64x(
                    if mask[3] != 0 { -1 } else { 0 },
                    if mask[2] != 0 { -1 } else { 0 },
                    if mask[1] != 0 { -1 } else { 0 },
                    if mask[0] != 0 { -1 } else { 0 },
                );

                let vt = _mm256_loadu_si256(
                    on_true.as_ptr() as *const __m256i,
                );
                let vf = _mm256_loadu_si256(
                    on_false.as_ptr() as *const __m256i,
                );

                // Select true values wherever the corresponding mask lane
                // is nonzero. All eight bytes of each i64 lane are selected.
                let result = _mm256_blendv_epi8(vf, vt, vm);

                let mut out = [0_i64; 4];
                _mm256_storeu_si256(
                    out.as_mut_ptr() as *mut __m256i,
                    result,
                );

                return out;
            }
        }
    }

    let mut out = [0_i64; 4];
    for i in 0..4 {
        out[i] = if mask[i] != 0 {
            on_true[i]
        } else {
            on_false[i]
        };
    }
    out
}

#[inline(always)]
pub fn simd_v4select_f(
    mask: [i64; 4],
    on_true: [f64; 4],
    on_false: [f64; 4],
) -> [f64; 4] {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe {
                // The mask needs a set sign bit for every selected lane.
                let vm = _mm256_set_epi64x(
                    if mask[3] != 0 { -1 } else { 0 },
                    if mask[2] != 0 { -1 } else { 0 },
                    if mask[1] != 0 { -1 } else { 0 },
                    if mask[0] != 0 { -1 } else { 0 },
                );

                let vt = _mm256_loadu_pd(on_true.as_ptr());
                let vf = _mm256_loadu_pd(on_false.as_ptr());

                // Reinterpret the mask bits; we do not convert the mask
                // numerically to floating point.
                let result = _mm256_blendv_pd(
                    vf,
                    vt,
                    _mm256_castsi256_pd(vm),
                );

                let mut out = [0.0_f64; 4];
                _mm256_storeu_pd(out.as_mut_ptr(), result);

                return out;
            }
        }
    }

    let mut out = [0.0_f64; 4];
    for i in 0..4 {
        out[i] = if mask[i] != 0 {
            on_true[i]
        } else {
            on_false[i]
        };
    }
    out
}