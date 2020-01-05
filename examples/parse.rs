use ::stas;
use std::env;

fn main() {
    match stas::parse_expr(
        &mut env::args()
            .skip(1)
            .collect::<Vec<String>>()
            .iter()
            .peekable(),
    ) {
        Ok(rules) => {
            for rule in rules {
                println!("rule {}", rule.fmt());
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
