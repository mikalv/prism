//! Probe: parse a require_auth config exactly like the deployed prism.toml.
use prism::config::Config;
fn main() {
    let toml_str = std::fs::read_to_string(
        std::env::args().nth(1).expect("usage: cfg_probe <file.toml>"),
    )
    .unwrap();
    let cfg: Config = toml::from_str(&toml_str).unwrap();
    println!("security.enabled = {}", cfg.security.enabled);
    println!("require_auth.collections = {:?}", cfg.security.require_auth.collections);
    println!("require_auth.hide = {}", cfg.security.require_auth.hide_from_anonymous);
}
