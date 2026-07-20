//! Packed workout encoder — the binary contract with the watch (SPEC.md §4.2).
//!
//! Layout (little-endian, matching the Cortex-M C structs):
//!   WorkoutHeader: name[24] · version u8 · exerciseCount u8 · reserved u16
//!   ExerciseRec:   movementId u8 · flags u8 (bit0 timed, bit1 amrap) ·
//!                  weight u16 (0.25 kg units) · setCount u8 · customNameIdx u8
//!   SetRec:        target u8 · rest u8 (5 s units)

pub const PACK_VERSION: u8 = 1;
pub const PACK_CAP: usize = 228;
pub const NAME_LEN: usize = 24;
pub const MAX_EXERCISES: usize = 16;
pub const MAX_SETS: usize = 10;

pub const FLAG_TIMED: u8 = 1 << 0;
pub const FLAG_AMRAP: u8 = 1 << 1;
pub const NO_CUSTOM_NAME: u8 = 0xFF;

pub struct PackSet {
    /// Reps, or hold seconds when the exercise is timed.
    pub target: u8,
    pub rest_secs: u16,
}

pub struct PackExercise {
    pub movement_id: u8,
    pub timed: bool,
    pub amrap: bool,
    /// Weight in 0.25 kg units (0 = bodyweight).
    pub weight_q: u16,
    pub sets: Vec<PackSet>,
}

pub fn pack_workout(name: &str, exercises: &[PackExercise]) -> Result<Vec<u8>, String> {
    if exercises.is_empty() || exercises.len() > MAX_EXERCISES {
        return Err(format!("workout must have 1–{MAX_EXERCISES} exercises"));
    }
    let mut out = Vec::with_capacity(PACK_CAP);

    let mut name_bytes = [0u8; NAME_LEN];
    let truncated = truncate_utf8(name, NAME_LEN);
    name_bytes[..truncated.len()].copy_from_slice(truncated.as_bytes());
    out.extend_from_slice(&name_bytes);
    out.push(PACK_VERSION);
    out.push(exercises.len() as u8);
    out.extend_from_slice(&[0, 0]);

    for e in exercises {
        if e.sets.is_empty() || e.sets.len() > MAX_SETS {
            return Err(format!("each exercise must have 1–{MAX_SETS} sets"));
        }
        let mut flags = 0u8;
        if e.timed {
            flags |= FLAG_TIMED;
        }
        if e.amrap {
            flags |= FLAG_AMRAP;
        }
        out.push(e.movement_id);
        out.push(flags);
        out.extend_from_slice(&e.weight_q.to_le_bytes());
        out.push(e.sets.len() as u8);
        out.push(NO_CUSTOM_NAME);
        for s in &e.sets {
            out.push(s.target);
            out.push(rest_to_units(s.rest_secs));
        }
    }

    if out.len() > PACK_CAP {
        return Err(format!(
            "workout packs to {} B, over the {PACK_CAP} B watch limit — remove exercises or sets",
            out.len()
        ));
    }
    Ok(out)
}

/// Rest is stored in 5-second units, rounded to nearest, capped at 1275 s.
fn rest_to_units(rest_secs: u16) -> u8 {
    ((u32::from(rest_secs) + 2) / 5).min(255) as u8
}

/// Longest prefix of `s` whose UTF-8 encoding fits in `max` bytes.
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_day() -> Vec<PackExercise> {
        vec![
            PackExercise {
                movement_id: 1, // bench press
                timed: false,
                amrap: false,
                weight_q: 240, // 60 kg
                sets: vec![
                    PackSet { target: 12, rest_secs: 90 },
                    PackSet { target: 10, rest_secs: 90 },
                    PackSet { target: 8, rest_secs: 90 },
                ],
            },
            PackExercise {
                movement_id: 31, // L-sit
                timed: true,
                amrap: false,
                weight_q: 0,
                sets: vec![
                    PackSet { target: 10, rest_secs: 60 },
                    PackSet { target: 10, rest_secs: 60 },
                ],
            },
        ]
    }

    #[test]
    fn packs_expected_layout() {
        let bytes = pack_workout("Push Day", &push_day()).unwrap();
        // header(28) + ex1(6 + 3*2) + ex2(6 + 2*2) = 50
        assert_eq!(bytes.len(), 50);
        assert_eq!(&bytes[..8], b"Push Day");
        assert!(bytes[8..24].iter().all(|&b| b == 0), "name is zero-padded");
        assert_eq!(bytes[24], PACK_VERSION);
        assert_eq!(bytes[25], 2, "exercise count");
        // exercise 1 at offset 28
        assert_eq!(bytes[28], 1, "movement id");
        assert_eq!(bytes[29], 0, "flags");
        assert_eq!(u16::from_le_bytes([bytes[30], bytes[31]]), 240, "60 kg in 0.25 kg units");
        assert_eq!(bytes[32], 3, "set count");
        assert_eq!(bytes[33], NO_CUSTOM_NAME);
        assert_eq!(&bytes[34..40], &[12, 18, 10, 18, 8, 18], "target/rest pairs, rest 90s = 18 units");
        // exercise 2 at offset 40
        assert_eq!(bytes[40], 31);
        assert_eq!(bytes[41], FLAG_TIMED);
        assert_eq!(&bytes[46..50], &[10, 12, 10, 12], "10 s holds, 60 s rest = 12 units");
    }

    #[test]
    fn rejects_oversized_workout() {
        let big: Vec<PackExercise> = (0..16)
            .map(|i| PackExercise {
                movement_id: i,
                timed: false,
                amrap: false,
                weight_q: 0,
                sets: (0..10).map(|_| PackSet { target: 10, rest_secs: 60 }).collect(),
            })
            .collect();
        let err = pack_workout("Too Big", &big).unwrap_err();
        assert!(err.contains("over the 228 B watch limit"), "got: {err}");
    }

    #[test]
    fn truncates_long_names_at_char_boundary() {
        let bytes = pack_workout("Übungsplan für Montagmorgen", &push_day()).unwrap();
        assert_eq!(bytes.len(), 50);
        let name = std::str::from_utf8(&bytes[..24].split(|&b| b == 0).next().unwrap()).unwrap();
        assert!(name.starts_with("Übungsplan"));
    }

    #[test]
    fn rest_rounding_and_cap() {
        assert_eq!(rest_to_units(0), 0);
        assert_eq!(rest_to_units(3), 1);
        assert_eq!(rest_to_units(90), 18);
        assert_eq!(rest_to_units(2000), 255);
    }
}
