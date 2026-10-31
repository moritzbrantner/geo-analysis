use std::collections::BTreeMap;

use geo_core::Result;

use crate::invalid_argument;

/// Numeric metric bag attached to points and aggregated features.
pub type GeoVizMetricRecord = BTreeMap<String, f64>;

pub(crate) fn geo_weight(metrics: &GeoVizMetricRecord, weight_metric: Option<&str>) -> f64 {
    let weight = weight_metric
        .and_then(|key| metrics.get(key))
        .copied()
        .or_else(|| metrics.get("weight").copied())
        .unwrap_or(1.0);

    if weight.is_finite() {
        weight.max(0.0)
    } else {
        0.0
    }
}

pub(crate) fn maximum_weight<T>(weighted: &[(T, f64)]) -> f64 {
    weighted
        .iter()
        .map(|(_, weight)| *weight)
        .fold(0.0_f64, f64::max)
}

pub(crate) fn finite_sum(left: f64, right: f64, label: &str) -> Result<f64> {
    let sum = left + right;
    if !sum.is_finite() {
        return Err(invalid_argument(format!(
            "{label} exceeds the finite numeric range"
        )));
    }
    Ok(sum)
}

pub(crate) fn accumulate_metrics(
    target: &mut GeoVizMetricRecord,
    source: &GeoVizMetricRecord,
    metric_keys: &[String],
) -> Result<()> {
    for key in metric_keys {
        let value = source.get(key).copied().unwrap_or(0.0);
        match target.get_mut(key) {
            Some(total) => *total = finite_sum(*total, value, "metric accumulation")?,
            None => {
                target.insert(key.clone(), finite_sum(0.0, value, "metric accumulation")?);
            }
        }
    }
    Ok(())
}

pub(crate) fn sum_metrics<'a>(
    records: impl IntoIterator<Item = &'a GeoVizMetricRecord>,
    metric_keys: &[String],
) -> Result<GeoVizMetricRecord> {
    let mut metrics = metric_keys
        .iter()
        .map(|key| (key.clone(), 0.0))
        .collect::<GeoVizMetricRecord>();

    for record in records {
        accumulate_metrics(&mut metrics, record, metric_keys)?;
    }

    Ok(metrics)
}
