use serde_json::Value;

/// Returns (is_correct, marks_awarded) for auto-gradable questions.
/// Returns None for essay questions.
pub fn auto_grade(
    question_type: &str,
    correct_answer: &Value,
    student_answer: &Value,
    max_marks: i32,
) -> Option<(bool, f64)> {
    match question_type {
        "multiple_choice_single" => {
            let correct = correct_answer.get("option_id")?.as_str()?;
            let student = student_answer.get("option_id")?.as_str()?;
            let is_correct = correct == student;
            Some((is_correct, if is_correct {max_marks as f64} else {0.0}))
        }
        "multiple_choice_multi" => {
            let correct_ids: Vec<&str> = correct_answer.get("option_ids")?
                .as_array()?.iter()
                .filter_map(|v| v.as_str())
                .collect();
            let student_ids: Vec<&str> = student_answer.get("option_ids")?
                .as_array()?
                .iter().filter_map(|v|v.as_str())
                .collect();

            let correct_set: std::collections::HashSet<_> = correct_ids.iter().collect();
            let student_set: std::collections::HashSet<_> = student_ids.iter().collect();
            let is_correct = correct_set == student_set;
            Some((is_correct, if is_correct { max_marks as f64} else {0.0}))
        }
        "true_false" => {
            let correct = correct_answer.get("answer")?.as_bool()?;
            let student = student_answer.get("answer")?.as_bool()?;
            let is_correct = correct == student;
            Some((is_correct, if is_correct { max_marks as f64} else { 0.0 }))
        }
        "short_answer" => {
            let accepted: Vec<&str> = correct_answer.get("accepted")?
                .as_array()?.iter()
                .filter_map(|v| v.as_str())
                .collect();
            let student = student_answer.get("text")?.as_str()?.trim().to_lowercase();
            let is_correct = accepted.iter().any(|a| a.trim().to_lowercase() == student);
            Some((is_correct, if is_correct {max_marks as f64} else {0.0}))
        }
        "numeric" => {
            let correct = correct_answer.get("value")?.as_f64()?;
            let tolerance = correct_answer.get("tolerance")?.as_f64().unwrap_or(0.0);
            let student = student_answer.get("value")?.as_f64()?;
            let is_correct = (student - correct).abs() <= tolerance;
            Some((is_correct, if is_correct { max_marks as f64} else {0.0}))
        }
        "fill_in_the_blanks" => {
            let correct_blanks: Vec<&str> = correct_answer.get("blanks")?
                .as_array()?.iter()
                .filter_map(|v| v.as_str())
                .collect();
            let student_blanks: Vec<&str> = student_answer.get("blanks")?
                .as_array()?.iter()
                .filter_map(|v| v.as_str())
                .collect();
            if correct_blanks.len() != student_blanks.len() {
                return Some((false, 0.0));
            }

            let all_match = correct_blanks.iter().zip(student_blanks.iter())
                .all(|(c,s)| c.trim().to_lowercase() == s.trim().to_lowercase());
            Some((all_match, if all_match { max_marks as f64} else {0.0}))
        }
        "essay" => None,
        _ => None,
    }
}