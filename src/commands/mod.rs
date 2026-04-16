pub mod hints;
pub mod parser;
pub mod types;

pub(crate) fn split_command(input: &str) -> (&str, Option<&str>) {
    if let Some(idx) = input.find(char::is_whitespace) {
        let (name, tail) = input.split_at(idx);
        (name, Some(tail.trim()))
    } else {
        (input, None)
    }
}
