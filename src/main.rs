use std::fs;

use crate::{cfg::Config, store::Store};

mod cfg;
mod store;

fn main() {
    if let Ok(contents) = fs::read_to_string("static/config.json") {
        if let Ok(c) = serde_json::from_str::<Config>(&contents) {
            let mut s = Store::new(c);

            s.add_public("valera");
            s.add_protected("jora");

            println!("{:?}", s.store);

            let mut i = 0;
            while i < 10 {
                s.consume("valera");
                i = i + 1;
            }

            let mut i = 0;
            while i < 5 {
                s.consume("jora");
                i = i + 1;
            }

            println!("{:?}", s.store);
        }
    }
}
