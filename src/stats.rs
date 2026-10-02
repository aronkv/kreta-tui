//! Averages and the derived statistics shown in the UI.

use std::collections::BTreeMap;

use chrono::{DateTime, Local};

use crate::api::models::{Grade, GradeKind};

const EPS: f64 = 1e-9;

/// Weighted average of `(value, weight)` pairs.
pub fn weighted_avg(items: impl IntoIterator<Item = (f64, f64)>) -> Option<f64> {
    let (sum, w) = items.into_iter().fold((0.0, 0.0), |(s, w), (v, wt)| (s + v * wt, w + wt));
    (w > 0.0).then(|| sum / w)
}

/// The grade an average rounds to (x.50 rounds up).
pub fn rounded(avg: f64) -> u8 {
    ((avg + 0.5 + EPS).floor() as u8).clamp(1, 5)
}

#[derive(Debug, Clone)]
pub struct SubjectStats {
    pub name: String,
    /// Indices into the grade list, newest first.
    pub grades: Vec<usize>,
    pub avg: Option<f64>,
    pub sum: f64,
    pub weight_sum: f64,
    pub half_year: Option<u8>,
    pub end_year: Option<u8>,
}

pub fn subjects(grades: &[Grade]) -> Vec<SubjectStats> {
    let mut map: BTreeMap<String, SubjectStats> = BTreeMap::new();
    for (i, g) in grades.iter().enumerate() {
        let s = map.entry(g.subject.clone()).or_insert_with(|| SubjectStats {
            name: g.subject.clone(),
            grades: Vec::new(),
            avg: None,
            sum: 0.0,
            weight_sum: 0.0,
            half_year: None,
            end_year: None,
        });
        s.grades.push(i);
        match g.kind {
            _ if g.counts() => {
                s.sum += g.value as f64 * g.weight;
                s.weight_sum += g.weight;
            }
            GradeKind::HalfYear if g.value > 0 => s.half_year = Some(g.value),
            GradeKind::EndYear if g.value > 0 => s.end_year = Some(g.value),
            _ => {}
        }
    }
    let mut out: Vec<SubjectStats> = map.into_values().collect();
    for s in &mut out {
        s.avg = (s.weight_sum > 0.0).then(|| s.sum / s.weight_sum);
    }
    out
}

/// Overall average: mean of the subject averages (as the official app does).
pub fn overall(subjects: &[SubjectStats]) -> Option<f64> {
    let avgs: Vec<f64> = subjects.iter().filter_map(|s| s.avg).collect();
    (!avgs.is_empty()).then(|| avgs.iter().sum::<f64>() / avgs.len() as f64)
}

/// Overall average after each day that had a new grade.
pub fn overall_timeline(grades: &[Grade]) -> Vec<(DateTime<Local>, f64)> {
    let mut counted: Vec<&Grade> = grades.iter().filter(|g| g.counts()).collect();
    counted.sort_by_key(|g| g.date);
    let mut per_subject: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    let mut out: Vec<(DateTime<Local>, f64)> = Vec::new();
    for g in counted {
        let e = per_subject.entry(&g.subject).or_default();
        e.0 += g.value as f64 * g.weight;
        e.1 += g.weight;
        let avg = per_subject.values().map(|(s, w)| s / w).sum::<f64>() / per_subject.len() as f64;
        match out.last_mut() {
            Some(last) if last.0.date_naive() == g.date.date_naive() => last.1 = avg,
            _ => out.push((g.date, avg)),
        }
    }
    out
}

/// Running average of one subject, oldest first.
pub fn subject_timeline(grades: &[Grade], s: &SubjectStats) -> Vec<f64> {
    let mut items: Vec<&Grade> = s.grades.iter().map(|&i| &grades[i]).filter(|g| g.counts()).collect();
    items.sort_by_key(|g| g.date);
    let (mut sum, mut w) = (0.0, 0.0);
    items
        .iter()
        .map(|g| {
            sum += g.value as f64 * g.weight;
            w += g.weight;
            sum / w
        })
        .collect()
}

/// Count of each grade value 1..=5 among counted grades.
pub fn distribution<'a>(grades: impl IntoIterator<Item = &'a Grade>) -> [u32; 5] {
    let mut d = [0; 5];
    for g in grades.into_iter().filter(|g| g.counts()) {
        d[g.value as usize - 1] += 1;
    }
    d
}

/// How many grades of `value` (at `weight`) it takes to reach `target`.
pub fn needed(sum: f64, wsum: f64, value: f64, weight: f64, target: f64) -> Option<u32> {
    if wsum > 0.0 && sum / wsum + EPS >= target {
        return Some(0);
    }
    if value <= target {
        return None;
    }
    let n = (target * wsum - sum) / (weight * (value - target));
    Some((n - EPS).ceil().max(0.0) as u32)
}

/// How many 1s (at `weight`) fit before the average drops below `floor`.
pub fn buffer_ones(sum: f64, wsum: f64, weight: f64, floor: f64) -> Option<u32> {
    if floor <= 1.0 || wsum == 0.0 || sum / wsum + EPS < floor {
        return None;
    }
    let n = (sum - floor * wsum) / (weight * (floor - 1.0));
    Some((n + EPS).floor().max(0.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding() {
        assert_eq!(rounded(3.49), 3);
        assert_eq!(rounded(3.5), 4);
        assert_eq!(rounded(4.99), 5);
    }

    #[test]
    fn needed_fives() {
        // 3, 3 (weight 100) -> avg 3.0, to reach 3.5 with 5s: (3.5*200-600)/(100*1.5) = 0.67 -> 1
        assert_eq!(needed(600.0, 200.0, 5.0, 100.0, 3.5), Some(1));
        assert_eq!(needed(600.0, 200.0, 5.0, 200.0, 3.5), Some(1));
        assert_eq!(needed(1000.0, 200.0, 5.0, 100.0, 4.5), Some(0));
        assert_eq!(needed(600.0, 200.0, 3.0, 100.0, 3.5), None);
    }

    #[test]
    fn ones_buffer() {
        // 5, 5 -> avg 5.0; floor 4.5: (1000-900)/(100*3.5) = 0.28 -> 0
        assert_eq!(buffer_ones(1000.0, 200.0, 100.0, 4.5), Some(0));
        // 5 x 10 -> (5000-4500)/350 = 1.43 -> 1
        assert_eq!(buffer_ones(5000.0, 1000.0, 100.0, 4.5), Some(1));
    }
}
