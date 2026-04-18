use mempalace_rs::config::MempalaceConfig;

fn main() {
    let config = MempalaceConfig::default();
    println!("Config Dir: {:?}", config.config_dir);
    println!("Palace Path: {}", config.palace_path);
}
