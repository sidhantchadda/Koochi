pub fn ignored_config_probe() {
    // Unsafe: this config should be ignored by --config.
}

pub fn override_config_probe() {
    // Unsafe: the explicit --config file should run.
}
