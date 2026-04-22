//! Compact-block-relay failure payload size fractions (by failure index).
//!
//! - **Control** nodes: 210× `0.01`.
//! - **Churn** nodes: piecewise-constant ramp as `(fraction, repeat_count)` runs expanding to 945
//!   entries.

/// Number of discrete failure indices for the control (non-churn) distribution.
const CBR_FAILURE_FRACTIONS_CONTROL_LEN: usize = 210;

static CBR_FAILURE_FRACTIONS_CONTROL_INNER: [f32; CBR_FAILURE_FRACTIONS_CONTROL_LEN] =
    [0.01_f32; CBR_FAILURE_FRACTIONS_CONTROL_LEN];

pub static CBR_FAILURE_FRACTIONS_CONTROL: &[f32] = &CBR_FAILURE_FRACTIONS_CONTROL_INNER;

const CBR_FAILURE_FRACTIONS_CHURN_LEN: usize = 945;

/// `(value, count)` runs that expand in order to [`CBR_FAILURE_FRACTIONS_CHURN`].
const CBR_FAILURE_FRACTIONS_CHURN_RUNS: &[(f32, usize)] = &[
    (0.01_f32, 546),
    (0.02_f32, 66),
    (0.03_f32, 39),
    (0.04_f32, 27),
    (0.05_f32, 21),
    (0.06_f32, 17),
    (0.07_f32, 14),
    (0.08_f32, 12),
    (0.09_f32, 11),
    (0.1_f32, 10),
    (0.11_f32, 9),
    (0.12_f32, 8),
    (0.13_f32, 7),
    (0.14_f32, 7),
    (0.15_f32, 6),
    (0.16_f32, 6),
    (0.17_f32, 5),
    (0.18_f32, 5),
    (0.19_f32, 5),
    (0.2_f32, 4),
    (0.21_f32, 4),
    (0.22_f32, 4),
    (0.23_f32, 4),
    (0.24_f32, 4),
    (0.25_f32, 3),
    (0.26_f32, 3),
    (0.27_f32, 3),
    (0.28_f32, 3),
    (0.29_f32, 3),
    (0.3_f32, 3),
    (0.31_f32, 3),
    (0.32_f32, 3),
    (0.33_f32, 2),
    (0.34_f32, 2),
    (0.35_f32, 2),
    (0.36_f32, 2),
    (0.37_f32, 2),
    (0.38_f32, 2),
    (0.39_f32, 2),
    (0.4_f32, 2),
    (0.41_f32, 2),
    (0.42_f32, 2),
    (0.43_f32, 2),
    (0.44_f32, 2),
    (0.45_f32, 2),
    (0.46_f32, 2),
    (0.47_f32, 2),
    (0.48_f32, 2),
    (0.49_f32, 1),
    (0.5_f32, 1),
    (0.51_f32, 1),
    (0.52_f32, 1),
    (0.53_f32, 1),
    (0.54_f32, 1),
    (0.55_f32, 1),
    (0.56_f32, 1),
    (0.57_f32, 1),
    (0.58_f32, 1),
    (0.59_f32, 1),
    (0.6_f32, 1),
    (0.61_f32, 1),
    (0.62_f32, 1),
    (0.63_f32, 1),
    (0.64_f32, 1),
    (0.65_f32, 1),
    (0.66_f32, 1),
    (0.67_f32, 1),
    (0.68_f32, 1),
    (0.69_f32, 1),
    (0.7_f32, 1),
    (0.71_f32, 1),
    (0.72_f32, 1),
    (0.73_f32, 1),
    (0.74_f32, 1),
    (0.75_f32, 1),
    (0.76_f32, 1),
    (0.77_f32, 1),
    (0.78_f32, 1),
    (0.79_f32, 1),
    (0.8_f32, 1),
    (0.81_f32, 1),
    (0.82_f32, 1),
    (0.83_f32, 1),
    (0.84_f32, 1),
    (0.85_f32, 1),
    (0.86_f32, 1),
    (0.87_f32, 1),
    (0.88_f32, 1),
    (0.89_f32, 1),
    (0.9_f32, 1),
    (0.91_f32, 1),
    (0.92_f32, 1),
    (0.93_f32, 1),
    (0.94_f32, 1),
    (0.95_f32, 1),
    (0.96_f32, 1),
];

static CBR_FAILURE_FRACTIONS_CHURN_INNER: [f32; CBR_FAILURE_FRACTIONS_CHURN_LEN] = {
    let mut out = [0.0_f32; CBR_FAILURE_FRACTIONS_CHURN_LEN];
    let mut i = 0usize;
    let mut ri = 0usize;
    while ri < CBR_FAILURE_FRACTIONS_CHURN_RUNS.len() {
        let (v, count) = CBR_FAILURE_FRACTIONS_CHURN_RUNS[ri];
        let mut j = 0usize;
        while j < count {
            out[i] = v;
            i += 1;
            j += 1;
        }
        ri += 1;
    }
    assert!(i == CBR_FAILURE_FRACTIONS_CHURN_LEN);
    out
};

pub static CBR_FAILURE_FRACTIONS_CHURN: &[f32] = &CBR_FAILURE_FRACTIONS_CHURN_INNER;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn churn_table_matches_runs() {
        let mut i = 0usize;
        for &(v, count) in CBR_FAILURE_FRACTIONS_CHURN_RUNS {
            for _ in 0..count {
                assert_eq!(CBR_FAILURE_FRACTIONS_CHURN[i], v);
                i += 1;
            }
        }
        assert_eq!(i, CBR_FAILURE_FRACTIONS_CHURN_LEN);
        assert_eq!(
            CBR_FAILURE_FRACTIONS_CHURN.len(),
            CBR_FAILURE_FRACTIONS_CHURN_LEN
        );
    }

    #[test]
    fn control_table_is_uniform() {
        assert_eq!(
            CBR_FAILURE_FRACTIONS_CONTROL.len(),
            CBR_FAILURE_FRACTIONS_CONTROL_LEN
        );
        for &x in CBR_FAILURE_FRACTIONS_CONTROL {
            assert_eq!(x, 0.01_f32);
        }
    }
}
