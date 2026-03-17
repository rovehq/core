use std::path::Path;

pub(super) fn model_exists(path: &str) -> bool {
    Path::new(path).exists()
}
