pub fn format_number(num: f64, language_data: &str) -> String {
    if num == 0.0 || num < 10000.0 {
        return num.to_string();
    } else if num < 100000000.0 {
        let num_string = format!("{:.1}", num / 10000.0);
        if num_string.ends_with(".0") {
            format!(
                "{}{}",
                num_string[..num_string.len() - 2].to_string(),
                language_data
            )
        } else {
            format!("{}{}", num_string, language_data)
        }
    } else {
        let num_string = format!("{:.1}", num / 100000000.0);
        if num_string.ends_with(".0") {
            format!(
                "{}{}",
                num_string[..num_string.len() - 2].to_string(),
                language_data
            )
        } else {
            format!("{}{}", num_string, language_data)
        }
    }
}
