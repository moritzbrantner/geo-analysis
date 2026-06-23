#![doc = include_str!("../README.md")]

pub mod surface;
use std::fmt;

/// Error type for deterministic map kernel validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapsKernelError {
    /// Caller supplied invalid numeric path input.
    InvalidArgument(String),
}

impl MapsKernelError {
    /// Creates an invalid-argument error.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument(message.into())
    }
}

impl fmt::Display for MapsKernelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(message) => write!(formatter, "invalid argument: {message}"),
        }
    }
}

impl std::error::Error for MapsKernelError {}

/// Map kernel result type.
pub type Result<T> = std::result::Result<T, MapsKernelError>;

fn invalid_argument(message: impl Into<String>) -> MapsKernelError {
    MapsKernelError::invalid_argument(message)
}

#[derive(Debug, Clone, PartialEq)]
/// Summary for a flat 2D open path or closed ring.
pub struct PathSummary {
    /// Number of 2D points in the path.
    pub point_count: usize,
    /// Number of path segments.
    pub segment_count: usize,
    /// Whether the final point is connected back to the first point.
    pub closed: bool,
    /// Total Euclidean path length.
    pub length: f64,
    /// Bounds as `[min_x, min_y, max_x, max_y]`.
    pub bounds: [f64; 4],
}

/// Summarizes a flat `[x0, y0, x1, y1, ...]` 2D path.
pub fn path_summary_flat(coordinates: &[f64], closed: bool) -> Result<PathSummary> {
    validate_flat_coordinates(coordinates, if closed { 3 } else { 2 }, "path coordinates")?;
    let point_count = coordinates.len() / 2;
    let segment_count = if closed { point_count } else { point_count - 1 };
    Ok(PathSummary {
        point_count,
        segment_count,
        closed,
        length: path_length_flat(coordinates, closed)?,
        bounds: bounds_flat(coordinates),
    })
}

/// Resamples an open line represented as flat `[x0, y0, x1, y1, ...]` coordinates.
pub fn resample_line_flat(coordinates: &[f64], coordinate_count: usize) -> Result<Vec<f64>> {
    validate_flat_coordinates(coordinates, 2, "line coordinates")?;
    validate_coordinate_count(coordinate_count, 2)?;

    let source_count = coordinates.len() / 2;

    if source_count == coordinate_count {
        return Ok(coordinates.to_vec());
    }

    let distances = cumulative_distances(coordinates, false)?;
    let total_distance = *distances.last().unwrap_or(&0.0);

    if total_distance == 0.0 {
        return Ok(repeat_position(coordinates, coordinate_count));
    }

    let mut samples = Vec::with_capacity(coordinate_count * 2);
    let mut segment_index = 0;

    for index in 0..coordinate_count {
        if index == coordinate_count - 1 {
            samples.extend_from_slice(&coordinates[(source_count - 1) * 2..source_count * 2]);
            continue;
        }

        let target_distance = total_distance * index as f64 / (coordinate_count - 1) as f64;
        let sample = interpolate_along_path_from_segment(
            coordinates,
            &distances,
            target_distance,
            false,
            segment_index,
        )?;

        samples.extend_from_slice(&sample.position);
        segment_index = sample.segment_index;
    }

    Ok(samples)
}

/// Resamples an open ring represented as flat `[x0, y0, x1, y1, ...]` coordinates.
pub fn resample_ring_flat(open_ring: &[f64], coordinate_count: usize) -> Result<Vec<f64>> {
    validate_flat_coordinates(open_ring, 3, "ring coordinates")?;
    validate_coordinate_count(coordinate_count, 3)?;

    let distances = cumulative_distances(open_ring, true)?;
    let total_distance = *distances.last().unwrap_or(&0.0);

    if total_distance == 0.0 {
        return Ok(repeat_position(open_ring, coordinate_count));
    }

    let mut samples = Vec::with_capacity(coordinate_count * 2);
    let mut segment_index = 0;

    for index in 0..coordinate_count {
        let target_distance = total_distance * index as f64 / coordinate_count as f64;
        let sample = interpolate_along_path_from_segment(
            open_ring,
            &distances,
            target_distance,
            true,
            segment_index,
        )?;

        samples.extend_from_slice(&sample.position);
        segment_index = sample.segment_index;
    }

    Ok(samples)
}

/// Simplifies an open line with deterministic Douglas-Peucker simplification.
pub fn simplify_line_flat(coordinates: &[f64], tolerance: f64) -> Result<Vec<f64>> {
    validate_flat_coordinates(coordinates, 2, "line coordinates")?;
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(invalid_argument(
            "simplification tolerance must be finite and non-negative",
        ));
    }
    let point_count = coordinates.len() / 2;
    if point_count <= 2 || tolerance == 0.0 {
        return Ok(coordinates.to_vec());
    }

    let mut keep = vec![false; point_count];
    keep[0] = true;
    keep[point_count - 1] = true;
    simplify_range(coordinates, 0, point_count - 1, tolerance, &mut keep);

    let mut output = Vec::with_capacity(coordinates.len());
    for (index, keep_point) in keep.into_iter().enumerate() {
        if keep_point {
            output.extend_from_slice(&coordinates[index * 2..index * 2 + 2]);
        }
    }
    Ok(output)
}

/// Inserts evenly spaced points so every open-line segment is at most `max_segment_length`.
pub fn densify_line_flat(coordinates: &[f64], max_segment_length: f64) -> Result<Vec<f64>> {
    validate_flat_coordinates(coordinates, 2, "line coordinates")?;
    if !max_segment_length.is_finite() || max_segment_length <= 0.0 {
        return Err(invalid_argument(
            "max segment length must be finite and greater than zero",
        ));
    }

    let point_count = coordinates.len() / 2;
    let mut output = Vec::with_capacity(coordinates.len());
    output.extend_from_slice(&coordinates[0..2]);
    for index in 0..point_count - 1 {
        let start = position_at(coordinates, index);
        let end = position_at(coordinates, index + 1);
        let length = distance(start, end);
        let pieces = (length / max_segment_length).ceil().max(1.0) as usize;
        for piece in 1..=pieces {
            let progress = piece as f64 / pieces as f64;
            output.extend_from_slice(&interpolate_position(start, end, progress));
        }
    }
    Ok(output)
}

fn validate_flat_coordinates(coordinates: &[f64], min_points: usize, label: &str) -> Result<()> {
    if coordinates.len() < min_points * 2 {
        return Err(invalid_argument(format!(
            "{label} must contain at least {min_points} positions"
        )));
    }

    if !coordinates.len().is_multiple_of(2) {
        return Err(invalid_argument(format!("{label} length must be even")));
    }

    if coordinates.iter().any(|value| !value.is_finite()) {
        return Err(invalid_argument(format!("{label} must be finite")));
    }

    Ok(())
}

fn validate_coordinate_count(coordinate_count: usize, minimum: usize) -> Result<()> {
    if coordinate_count < minimum {
        return Err(invalid_argument(format!(
            "coordinate count must be at least {minimum}"
        )));
    }

    Ok(())
}

fn path_length_flat(coordinates: &[f64], closed: bool) -> Result<f64> {
    Ok(cumulative_distances(coordinates, closed)?
        .last()
        .copied()
        .unwrap_or(0.0))
}

fn bounds_flat(coordinates: &[f64]) -> [f64; 4] {
    let mut min_x = coordinates[0];
    let mut min_y = coordinates[1];
    let mut max_x = coordinates[0];
    let mut max_y = coordinates[1];
    for point in coordinates.chunks_exact(2).skip(1) {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

fn cumulative_distances(coordinates: &[f64], closed: bool) -> Result<Vec<f64>> {
    let point_count = coordinates.len() / 2;
    let segment_count = if closed { point_count } else { point_count - 1 };
    let mut distances = Vec::with_capacity(segment_count + 1);

    distances.push(0.0);

    for index in 0..segment_count {
        let start = position_at(coordinates, index);
        let end = position_at(coordinates, (index + 1) % point_count);
        let previous_distance = *distances.last().unwrap_or(&0.0);

        distances.push(previous_distance + distance(start, end));
    }

    Ok(distances)
}

struct PathSample {
    position: [f64; 2],
    segment_index: usize,
}

fn interpolate_along_path_from_segment(
    coordinates: &[f64],
    distances: &[f64],
    target_distance: f64,
    closed: bool,
    start_segment_index: usize,
) -> Result<PathSample> {
    let point_count = coordinates.len() / 2;
    let segment_count = if closed { point_count } else { point_count - 1 };
    let max_distance = *distances.last().unwrap_or(&0.0);
    let target_distance = clamp_finite(target_distance, 0.0, max_distance)?;

    for index in start_segment_index..segment_count {
        let segment_start_distance = distances[index];
        let segment_end_distance = distances[index + 1];

        if target_distance > segment_end_distance {
            continue;
        }

        let start = position_at(coordinates, index);
        let end = position_at(coordinates, (index + 1) % point_count);
        let segment_length = segment_end_distance - segment_start_distance;
        let progress = if segment_length == 0.0 {
            0.0
        } else {
            (target_distance - segment_start_distance) / segment_length
        };

        return Ok(PathSample {
            position: interpolate_position(start, end, progress),
            segment_index: index,
        });
    }

    Ok(PathSample {
        position: position_at(coordinates, point_count - 1),
        segment_index: segment_count.saturating_sub(1),
    })
}

fn clamp_finite(value: f64, min: f64, max: f64) -> Result<f64> {
    if !value.is_finite() || !min.is_finite() || !max.is_finite() {
        return Err(invalid_argument("range values must be finite"));
    }
    if min > max {
        return Err(invalid_argument("range min must not exceed max"));
    }
    Ok(value.clamp(min, max))
}

fn repeat_position(coordinates: &[f64], coordinate_count: usize) -> Vec<f64> {
    let position = &coordinates[0..2];
    let mut samples = Vec::with_capacity(coordinate_count * 2);

    for _ in 0..coordinate_count {
        samples.extend_from_slice(position);
    }

    samples
}

fn position_at(coordinates: &[f64], index: usize) -> [f64; 2] {
    let offset = index * 2;

    [coordinates[offset], coordinates[offset + 1]]
}

fn interpolate_position(start: [f64; 2], end: [f64; 2], progress: f64) -> [f64; 2] {
    [
        start[0] + (end[0] - start[0]) * progress,
        start[1] + (end[1] - start[1]) * progress,
    ]
}

fn distance(start: [f64; 2], end: [f64; 2]) -> f64 {
    (end[0] - start[0]).hypot(end[1] - start[1])
}

fn simplify_range(
    coordinates: &[f64],
    start_index: usize,
    end_index: usize,
    tolerance: f64,
    keep: &mut [bool],
) {
    if end_index <= start_index + 1 {
        return;
    }
    let start = position_at(coordinates, start_index);
    let end = position_at(coordinates, end_index);
    let mut max_distance = -1.0;
    let mut max_index = start_index + 1;
    for index in start_index + 1..end_index {
        let point = position_at(coordinates, index);
        let distance = perpendicular_distance(point, start, end);
        if distance > max_distance {
            max_distance = distance;
            max_index = index;
        }
    }
    if max_distance > tolerance {
        keep[max_index] = true;
        simplify_range(coordinates, start_index, max_index, tolerance, keep);
        simplify_range(coordinates, max_index, end_index, tolerance, keep);
    }
}

fn perpendicular_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0.0 {
        return distance(point, start);
    }
    let t = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length_squared)
        .clamp(0.0, 1.0);
    let projection = [start[0] + t * dx, start[1] + t * dy];
    distance(point, projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_one_segment_line() {
        let samples = resample_line_flat(&[0.0, 0.0, 10.0, 0.0], 3).unwrap();

        assert_eq!(samples, vec![0.0, 0.0, 5.0, 0.0, 10.0, 0.0]);
    }

    #[test]
    fn resamples_ring_without_closing_it() {
        let samples = resample_ring_flat(&[0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0], 4).unwrap();

        assert_eq!(samples, vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]);
    }

    #[test]
    fn repeats_zero_distance_line() {
        let samples = resample_line_flat(&[3.0, 4.0, 3.0, 4.0], 4).unwrap();

        assert_eq!(samples, vec![3.0, 4.0, 3.0, 4.0, 3.0, 4.0, 3.0, 4.0]);
    }

    #[test]
    fn repeats_zero_distance_ring() {
        let samples = resample_ring_flat(&[1.0, 2.0, 1.0, 2.0, 1.0, 2.0], 3).unwrap();

        assert_eq!(samples, vec![1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
    }

    #[test]
    fn rejects_odd_length_coordinates() {
        assert!(resample_line_flat(&[0.0, 0.0, 1.0], 2).is_err());
    }

    #[test]
    fn rejects_non_finite_coordinates() {
        assert!(resample_ring_flat(&[0.0, 0.0, f64::NAN, 0.0, 1.0, 1.0], 3).is_err());
    }

    #[test]
    fn rejects_invalid_coordinate_count() {
        assert!(resample_line_flat(&[0.0, 0.0, 1.0, 1.0], 1).is_err());
        assert!(resample_ring_flat(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0], 2).is_err());
    }

    #[test]
    fn summarizes_simplifies_and_densifies_paths() {
        let coordinates = [0.0, 0.0, 1.0, 0.01, 2.0, 0.0, 4.0, 0.0];
        let summary = path_summary_flat(&coordinates, false).unwrap();
        assert_eq!(summary.point_count, 4);
        assert_eq!(summary.segment_count, 3);
        assert_eq!(summary.bounds, [0.0, 0.0, 4.0, 0.01]);

        let simplified = simplify_line_flat(&coordinates, 0.1).unwrap();
        assert_eq!(&simplified[0..2], &[0.0, 0.0]);
        assert_eq!(&simplified[simplified.len() - 2..], &[4.0, 0.0]);

        let densified = densify_line_flat(&[0.0, 0.0, 3.0, 0.0], 1.0).unwrap();
        assert_eq!(densified, vec![0.0, 0.0, 1.0, 0.0, 2.0, 0.0, 3.0, 0.0]);
    }
}
