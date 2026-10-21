use std::sync::{Arc, RwLock};
use crate::lang::types::Value;

/// A segment in an array pattern decomposition.
#[derive(Clone, PartialEq, Debug)]
pub enum ListSegment {
    /// Matches a single element with a Pattern (e.g. `Bind`, `Literal`, `IntRange`).
    Element(Pattern),
    /// Matches a fixed sequence of elements with Patterns (e.g. `[a, b]`).
    Elements(Vec<Pattern>),
    /// Matches a variable slice of elements and binds the sub-array to a register (`..a`).
    BindList(usize),
    /// Matches a variable slice of elements without binding (`..`).
    WildcardList,
}

/// An array pattern supporting head, middle, and tail list decomposition.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ArrayPattern {
    pub segments: Vec<ListSegment>,
}

impl ArrayPattern {
    pub fn new(segments: Vec<ListSegment>) -> Self {
        Self { segments }
    }

    /// Convenience constructor for `[head | tail]` decomposition.
    pub fn head_tail(head: ListSegment, tail: ListSegment) -> Self {
        Self {
            segments: vec![head, tail],
        }
    }

    /// Convenience constructor for `[head | middle | tail]` decomposition.
    pub fn head_mid_tail(head: ListSegment, mid: ListSegment, tail: ListSegment) -> Self {
        Self {
            segments: vec![head, mid, tail],
        }
    }
}

/// Pattern definition for pattern matching in IonVM.
#[derive(Clone, PartialEq, Debug)]
pub enum Pattern {
    /// Wildcard `_` matching any value.
    Wildcard,
    /// Literal match against a specific `Value`.
    Literal(Value),
    /// Variable binding to a register index.
    Bind(usize),
    /// Fixed tuple matching.
    Tuple(Vec<Pattern>),
    /// Object property matching.
    Object(Vec<(String, Pattern)>),
    /// Integer range match: (start, end, inclusive). None represents unbounded side.
    IntRange(Option<i64>, Option<i64>, bool),
    /// Float range match: (start, end, inclusive). None represents unbounded side.
    FloatRange(Option<f64>, Option<f64>, bool),
    /// Generic array pattern matching all array values (`Value::Array`, `Value::IntArray`, `Value::FloatArray`).
    Array(ArrayPattern),
    /// Typed array pattern strictly matching only `Value::IntArray`.
    IntArray(ArrayPattern),
    /// Typed array pattern strictly matching only `Value::FloatArray`.
    FloatArray(ArrayPattern),
}

/// Matches a `Value` against a `Pattern`. If matching succeeds, register bindings are collected.
pub fn match_value(val: &Value, pattern: &Pattern, bindings: &mut Vec<(usize, Value)>) -> bool {
    match pattern {
        Pattern::Wildcard => true,
        Pattern::Literal(expected) => val.eql(expected),
        Pattern::Bind(reg) => {
            bindings.push((*reg, val.clone()));
            true
        }
        Pattern::IntRange(start, end, inclusive) => {
            if let Value::Int(i) = val {
                if let Some(s) = start {
                    if *i < *s {
                        return false;
                    }
                }
                if let Some(e) = end {
                    if *inclusive {
                        if *i > *e {
                            return false;
                        }
                    } else if *i >= *e {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        }
        Pattern::FloatRange(start, end, inclusive) => {
            if let Value::Float(f) = val {
                if let Some(s) = start {
                    if *f < *s {
                        return false;
                    }
                }
                if let Some(e) = end {
                    if *inclusive {
                        if *f > *e {
                            return false;
                        }
                    } else if *f >= *e {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        }
        Pattern::Tuple(pats) => match val {
            Value::Array(arr) => {
                let borrowed = arr.read().unwrap();
                if borrowed.len() != pats.len() {
                    return false;
                }
                let mut local_bindings = Vec::new();
                for (v, p) in borrowed.iter().zip(pats.iter()) {
                    if !match_value(v, p, &mut local_bindings) {
                        return false;
                    }
                }
                bindings.extend(local_bindings);
                true
            }
            Value::IntArray(arr) => {
                let borrowed = arr.read().unwrap();
                if borrowed.len() != pats.len() {
                    return false;
                }
                let mut local_bindings = Vec::new();
                for (v, p) in borrowed.iter().zip(pats.iter()) {
                    if !match_value(&Value::Int(*v), p, &mut local_bindings) {
                        return false;
                    }
                }
                bindings.extend(local_bindings);
                true
            }
            Value::FloatArray(arr) => {
                let borrowed = arr.read().unwrap();
                if borrowed.len() != pats.len() {
                    return false;
                }
                let mut local_bindings = Vec::new();
                for (v, p) in borrowed.iter().zip(pats.iter()) {
                    if !match_value(&Value::Float(*v), p, &mut local_bindings) {
                        return false;
                    }
                }
                bindings.extend(local_bindings);
                true
            }
            _ => false,
        },
        Pattern::Object(props) => {
            if let Value::Object(obj) = val {
                let borrowed = obj.read().unwrap();
                let mut local_bindings = Vec::new();
                for (key, pat) in props {
                    if let Some(prop_val) = borrowed.get_property(key) {
                        if !match_value(&prop_val, pat, &mut local_bindings) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                bindings.extend(local_bindings);
                true
            } else {
                false
            }
        }
        Pattern::Array(arr_pat) => match val {
            Value::Array(arr) => {
                let borrowed = arr.read().unwrap();
                let len = borrowed.len();
                match_array_segments(
                    len,
                    arr_pat,
                    bindings,
                    |idx| borrowed[idx].clone(),
                    |start, end| Value::Array(Arc::new(RwLock::new(borrowed[start..end].to_vec()))),
                )
            }
            Value::IntArray(arr) => {
                let borrowed = arr.read().unwrap();
                let len = borrowed.len();
                match_array_segments(
                    len,
                    arr_pat,
                    bindings,
                    |idx| Value::Int(borrowed[idx]),
                    |start, end| Value::IntArray(Arc::new(RwLock::new(borrowed[start..end].to_vec()))),
                )
            }
            Value::FloatArray(arr) => {
                let borrowed = arr.read().unwrap();
                let len = borrowed.len();
                match_array_segments(
                    len,
                    arr_pat,
                    bindings,
                    |idx| Value::Float(borrowed[idx]),
                    |start, end| Value::FloatArray(Arc::new(RwLock::new(borrowed[start..end].to_vec()))),
                )
            }
            _ => false,
        },
        Pattern::IntArray(arr_pat) => match val {
            Value::IntArray(arr) => {
                let borrowed = arr.read().unwrap();
                let len = borrowed.len();
                match_array_segments(
                    len,
                    arr_pat,
                    bindings,
                    |idx| Value::Int(borrowed[idx]),
                    |start, end| Value::IntArray(Arc::new(RwLock::new(borrowed[start..end].to_vec()))),
                )
            }
            _ => false,
        },
        Pattern::FloatArray(arr_pat) => match val {
            Value::FloatArray(arr) => {
                let borrowed = arr.read().unwrap();
                let len = borrowed.len();
                match_array_segments(
                    len,
                    arr_pat,
                    bindings,
                    |idx| Value::Float(borrowed[idx]),
                    |start, end| Value::FloatArray(Arc::new(RwLock::new(borrowed[start..end].to_vec()))),
                )
            }
            _ => false,
        },
    }
}

/// Helper matching array segments supporting fixed element sequences and variable slice distributions.
fn match_array_segments<G, S>(
    len: usize,
    arr_pat: &ArrayPattern,
    bindings: &mut Vec<(usize, Value)>,
    get_elem: G,
    make_slice: S,
) -> bool
where
    G: Fn(usize) -> Value,
    S: Fn(usize, usize) -> Value,
{
    let mut fixed_count = 0;
    let mut var_count = 0;

    for seg in &arr_pat.segments {
        match seg {
            ListSegment::Element(_) => fixed_count += 1,
            ListSegment::Elements(pats) => fixed_count += pats.len(),
            ListSegment::BindList(_) | ListSegment::WildcardList => var_count += 1,
        }
    }

    if len < fixed_count {
        return false;
    }
    if var_count == 0 && len != fixed_count {
        return false;
    }

    let rem = len - fixed_count;
    let (base_var_len, mut extra) = if var_count > 0 {
        (rem / var_count, rem % var_count)
    } else {
        (0, 0)
    };

    let mut local_bindings = Vec::new();
    let mut curr_idx = 0;

    for seg in &arr_pat.segments {
        match seg {
            ListSegment::Element(pat) => {
                let elem = get_elem(curr_idx);
                if !match_value(&elem, pat, &mut local_bindings) {
                    return false;
                }
                curr_idx += 1;
            }
            ListSegment::Elements(pats) => {
                for pat in pats {
                    let elem = get_elem(curr_idx);
                    if !match_value(&elem, pat, &mut local_bindings) {
                        return false;
                    }
                    curr_idx += 1;
                }
            }
            ListSegment::WildcardList => {
                let take_len = base_var_len + if extra > 0 { extra -= 1; 1 } else { 0 };
                curr_idx += take_len;
            }
            ListSegment::BindList(reg) => {
                let take_len = base_var_len + if extra > 0 { extra -= 1; 1 } else { 0 };
                let sub = make_slice(curr_idx, curr_idx + take_len);
                local_bindings.push((*reg, sub));
                curr_idx += take_len;
            }
        }
    }

    bindings.extend(local_bindings);
    true
}

/// Evaluates a list of match cases against a target value.
/// Returns the jump offset and variable bindings of the first matching case.
pub fn evaluate_match(
    val: &Value,
    cases: &[(Pattern, i64)],
) -> Option<(i64, Vec<(usize, Value)>)> {
    for (pat, offset) in cases {
        let mut bindings = Vec::new();
        if match_value(val, pat, &mut bindings) {
            return Some((*offset, bindings));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_range_matching() {
        let mut bindings = Vec::new();
        // Half-open start..end (5..10)
        let pat_range = Pattern::IntRange(Some(5), Some(10), false);
        assert!(match_value(&Value::Int(5), &pat_range, &mut bindings));
        assert!(match_value(&Value::Int(9), &pat_range, &mut bindings));
        assert!(!match_value(&Value::Int(10), &pat_range, &mut bindings));
        assert!(!match_value(&Value::Int(4), &pat_range, &mut bindings));

        // Inclusive start..=end (5..=10)
        let pat_range_inc = Pattern::IntRange(Some(5), Some(10), true);
        assert!(match_value(&Value::Int(10), &pat_range_inc, &mut bindings));

        // Less than 10 (..10)
        let pat_lt = Pattern::IntRange(None, Some(10), false);
        assert!(match_value(&Value::Int(-5), &pat_lt, &mut bindings));
        assert!(match_value(&Value::Int(9), &pat_lt, &mut bindings));
        assert!(!match_value(&Value::Int(10), &pat_lt, &mut bindings));

        // Greater than or equal to 5 (5..)
        let pat_gte = Pattern::IntRange(Some(5), None, false);
        assert!(match_value(&Value::Int(5), &pat_gte, &mut bindings));
        assert!(match_value(&Value::Int(100), &pat_gte, &mut bindings));
        assert!(!match_value(&Value::Int(4), &pat_gte, &mut bindings));
    }

    #[test]
    fn test_float_range_matching() {
        let mut bindings = Vec::new();
        let pat = Pattern::FloatRange(Some(1.5), Some(4.5), false);
        assert!(match_value(&Value::Float(2.0), &pat, &mut bindings));
        assert!(!match_value(&Value::Float(4.5), &pat, &mut bindings));
        assert!(!match_value(&Value::Float(1.4), &pat, &mut bindings));
    }

    #[test]
    fn test_array_head_tail_decomposition() {
        // [a | ..b] on [1, 2, 3, 4, 5, 6, 7]
        let list = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3, 4, 5, 6, 7])));
        let pat = Pattern::Array(ArrayPattern::head_tail(
            ListSegment::Element(Pattern::Bind(0)),
            ListSegment::BindList(1),
        ));

        let mut bindings = Vec::new();
        assert!(match_value(&list, &pat, &mut bindings));
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0], (0, Value::Int(1)));
        if let (1, Value::IntArray(tail)) = &bindings[1] {
            assert_eq!(*tail.read().unwrap(), vec![2, 3, 4, 5, 6, 7]);
        } else {
            panic!("Expected Value::IntArray for tail");
        }
    }

    #[test]
    fn test_array_bindlist_head_element_tail() {
        // [..a | b] on [1, 2, 3, 4, 5, 6, 7]
        let list = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3, 4, 5, 6, 7])));
        let pat = Pattern::Array(ArrayPattern::head_tail(
            ListSegment::BindList(0),
            ListSegment::Element(Pattern::Bind(1)),
        ));

        let mut bindings = Vec::new();
        assert!(match_value(&list, &pat, &mut bindings));
        assert_eq!(bindings.len(), 2);
        if let (0, Value::IntArray(head)) = &bindings[0] {
            assert_eq!(*head.read().unwrap(), vec![1, 2, 3, 4, 5, 6]);
        } else {
            panic!("Expected Value::IntArray for head");
        }
        assert_eq!(bindings[1], (1, Value::Int(7)));
    }

    #[test]
    fn test_array_head_mid_tail_decomposition() {
        // [..a | b | ..c] on [1, 2, 3, 4, 5, 6, 7] -> a=[1,2,3], b=4, c=[5,6,7]
        let list = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3, 4, 5, 6, 7])));
        let pat = Pattern::Array(ArrayPattern::head_mid_tail(
            ListSegment::BindList(0),
            ListSegment::Element(Pattern::Bind(1)),
            ListSegment::BindList(2),
        ));

        let mut bindings = Vec::new();
        assert!(match_value(&list, &pat, &mut bindings));
        assert_eq!(bindings.len(), 3);
        if let (0, Value::IntArray(a)) = &bindings[0] {
            assert_eq!(*a.read().unwrap(), vec![1, 2, 3]);
        } else {
            panic!("Expected Value::IntArray for a");
        }
        assert_eq!(bindings[1], (1, Value::Int(4)));
        if let (2, Value::IntArray(c)) = &bindings[2] {
            assert_eq!(*c.read().unwrap(), vec![5, 6, 7]);
        } else {
            panic!("Expected Value::IntArray for c");
        }
    }

    #[test]
    fn test_array_elements_and_bindlist() {
        // [a, b | ..c] on [1, 2, 3, 4, 5, 6, 7] -> a=1, b=2, c=[3,4,5,6,7]
        let list = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3, 4, 5, 6, 7])));
        let pat = Pattern::Array(ArrayPattern::new(vec![
            ListSegment::Elements(vec![Pattern::Bind(0), Pattern::Bind(1)]),
            ListSegment::BindList(2),
        ]));

        let mut bindings = Vec::new();
        assert!(match_value(&list, &pat, &mut bindings));
        assert_eq!(bindings.len(), 3);
        assert_eq!(bindings[0], (0, Value::Int(1)));
        assert_eq!(bindings[1], (1, Value::Int(2)));
        if let (2, Value::IntArray(c)) = &bindings[2] {
            assert_eq!(*c.read().unwrap(), vec![3, 4, 5, 6, 7]);
        } else {
            panic!("Expected Value::IntArray for c");
        }
    }

    #[test]
    fn test_array_bindlist_and_elements() {
        // [..a | b, c] on [1, 2, 3, 4, 5, 6, 7] -> a=[1,2,3,4,5], b=6, c=7
        let list = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3, 4, 5, 6, 7])));
        let pat = Pattern::Array(ArrayPattern::new(vec![
            ListSegment::BindList(0),
            ListSegment::Elements(vec![Pattern::Bind(1), Pattern::Bind(2)]),
        ]));

        let mut bindings = Vec::new();
        assert!(match_value(&list, &pat, &mut bindings));
        assert_eq!(bindings.len(), 3);
        if let (0, Value::IntArray(a)) = &bindings[0] {
            assert_eq!(*a.read().unwrap(), vec![1, 2, 3, 4, 5]);
        } else {
            panic!("Expected Value::IntArray for a");
        }
        assert_eq!(bindings[1], (1, Value::Int(6)));
        assert_eq!(bindings[2], (2, Value::Int(7)));
    }

    #[test]
    fn test_array_range_and_literal_fixed_length() {
        // [..a, b, 7] on [5, 12, 7] with a=10 -> matches!
        let list = Value::IntArray(Arc::new(RwLock::new(vec![5, 12, 7])));
        let pat = Pattern::Array(ArrayPattern::new(vec![
            ListSegment::Element(Pattern::IntRange(None, Some(10), false)),
            ListSegment::Element(Pattern::Bind(0)),
            ListSegment::Element(Pattern::Literal(Value::Int(7))),
        ]));

        let mut bindings = Vec::new();
        assert!(match_value(&list, &pat, &mut bindings));
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0], (0, Value::Int(12)));

        // If length is 4, fails
        let list4 = Value::IntArray(Arc::new(RwLock::new(vec![5, 12, 7, 9])));
        assert!(!match_value(&list4, &pat, &mut Vec::new()));

        // If first element >= 10, fails
        let list_fail = Value::IntArray(Arc::new(RwLock::new(vec![15, 12, 7])));
        assert!(!match_value(&list_fail, &pat, &mut Vec::new()));
    }

    #[test]
    fn test_typed_array_pattern_matching() {
        let v_int = Value::IntArray(Arc::new(RwLock::new(vec![1, 2, 3])));
        let v_dyn = Value::Array(Arc::new(RwLock::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)])));
        let pat_int = Pattern::IntArray(ArrayPattern::new(vec![ListSegment::WildcardList]));
        let pat_gen = Pattern::Array(ArrayPattern::new(vec![ListSegment::WildcardList]));

        // IntArray pattern only matches Value::IntArray
        assert!(match_value(&v_int, &pat_int, &mut Vec::new()));
        assert!(!match_value(&v_dyn, &pat_int, &mut Vec::new()));

        // Generic Array pattern matches both
        assert!(match_value(&v_int, &pat_gen, &mut Vec::new()));
        assert!(match_value(&v_dyn, &pat_gen, &mut Vec::new()));
    }
}
