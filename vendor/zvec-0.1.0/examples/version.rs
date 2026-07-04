fn main() {
    println!("zvec version: {}", zvec::version());
    println!(
        "parsed: {}.{}.{}",
        zvec::version_major(),
        zvec::version_minor(),
        zvec::version_patch(),
    );
}
