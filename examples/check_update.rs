//! Release acceptance: use the real updater without opening a user database.
fn main() {
    let current = std::env::args()
        .nth(1)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    match rili::update::check(&current) {
        Ok(rili::update::CheckResult::Available(info)) => println!(
            "{}",
            serde_json::json!({
                "current": current,
                "state": "available",
                "latest": info.version,
                "github_download": info.github_download,
                "gitee_download": info.gitee_download,
            })
        ),
        Ok(rili::update::CheckResult::Current { latest }) => println!(
            "{}",
            serde_json::json!({ "current": current, "state": "current", "latest": latest })
        ),
        Err(error) => {
            eprintln!("Update check failed: {error}");
            std::process::exit(1);
        }
    }
}
