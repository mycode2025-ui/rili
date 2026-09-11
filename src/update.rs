//! 应用更新检查。
//!
//! 与 PcanWork 的发布模式一致：同时检查 GitHub/Gitee，选择较新的有效发布，
//! 只提供已由发布接口确认存在的 Windows 程序下载地址。安装由用户确认后执行，
//! 不在后台静默替换正在运行的可执行文件。

use semver::Version;
use serde::Deserialize;
use std::time::Duration;

const OWNER: &str = "mycode2025-ui";
const REPOSITORY: &str = "rili";
const GITHUB_LATEST_API: &str = "https://api.github.com/repos/mycode2025-ui/rili/releases/latest";
const GITEE_LATEST_API: &str = "https://gitee.com/api/v5/repos/mycode2025-ui/rili/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub github_download: String,
    pub gitee_download: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckResult {
    Available(UpdateInfo),
    Current { latest: String },
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone, Copy)]
enum Source {
    Github,
    Gitee,
}

/// 检查两个发布镜像。一个源不可用时继续使用另一个；两者都失败才返回错误。
pub fn check(current: &str) -> Result<CheckResult, String> {
    let (github, gitee) = std::thread::scope(|scope| {
        let github = scope.spawn(|| check_source(current, GITHUB_LATEST_API, Source::Github));
        let gitee = scope.spawn(|| check_source(current, GITEE_LATEST_API, Source::Gitee));
        (
            github
                .join()
                .unwrap_or_else(|_| Err("GitHub 检查线程异常".into())),
            gitee
                .join()
                .unwrap_or_else(|_| Err("Gitee 检查线程异常".into())),
        )
    });
    let mut result = merge_results(github, gitee)?;
    if let CheckResult::Available(info) = &mut result {
        populate_reachable_mirrors(info);
    }
    Ok(result)
}

fn populate_reachable_mirrors(info: &mut UpdateInfo) {
    if info.github_download.is_empty() {
        let candidate = expected_download(Source::Github, &info.version);
        if download_exists(&candidate) {
            info.github_download = candidate;
        }
    }
    if info.gitee_download.is_empty() {
        let candidate = expected_download(Source::Gitee, &info.version);
        if download_exists(&candidate) {
            info.gitee_download = candidate;
        }
    }
}

fn expected_download(source: Source, version: &str) -> String {
    let tag = format!("v{version}");
    match source {
        Source::Github => {
            format!("https://github.com/{OWNER}/{REPOSITORY}/releases/download/{tag}/TimeHub.exe")
        }
        Source::Gitee => {
            format!("https://gitee.com/{OWNER}/{REPOSITORY}/releases/download/{tag}/TimeHub.exe")
        }
    }
}

fn download_exists(url: &str) -> bool {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(7))
        .build()
        .get(url)
        .set("Range", "bytes=0-0")
        .set(
            "User-Agent",
            &format!("TimeHub/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .is_ok()
}

fn check_source(current: &str, endpoint: &str, source: Source) -> Result<CheckResult, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    let response = match agent
        .get(endpoint)
        .set("Accept", "application/json")
        .set(
            "User-Agent",
            &format!("TimeHub/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
    {
        Ok(response) => response,
        // 新项目在首次创建 Release 前，latest 接口按约定返回 404。这不是网络故障，
        // 也不应在“关于”页显示红色错误。
        Err(ureq::Error::Status(404, _)) => {
            return Ok(CheckResult::Current {
                latest: parse_version(current)?.to_string(),
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let release = response
        .into_json::<ApiRelease>()
        .map_err(|error| error.to_string())?;
    evaluate_release(current, release, source)
}

fn merge_results(
    github: Result<CheckResult, String>,
    gitee: Result<CheckResult, String>,
) -> Result<CheckResult, String> {
    match (github, gitee) {
        (Ok(github), Ok(gitee)) => merge_successes(github, gitee),
        (Ok(result), Err(_)) | (Err(_), Ok(result)) => Ok(result),
        (Err(github), Err(gitee)) => Err(format!("GitHub: {github}; Gitee: {gitee}")),
    }
}

fn merge_successes(github: CheckResult, gitee: CheckResult) -> Result<CheckResult, String> {
    match (github, gitee) {
        (CheckResult::Available(mut github), CheckResult::Available(gitee)) => {
            let github_version = parse_version(&github.version)?;
            let gitee_version = parse_version(&gitee.version)?;
            if gitee_version > github_version {
                return Ok(CheckResult::Available(gitee));
            }
            if gitee_version == github_version {
                github.gitee_download = gitee.gitee_download;
                if github.notes.is_empty() {
                    github.notes = gitee.notes;
                }
            }
            Ok(CheckResult::Available(github))
        }
        (CheckResult::Available(info), CheckResult::Current { .. })
        | (CheckResult::Current { .. }, CheckResult::Available(info)) => {
            Ok(CheckResult::Available(info))
        }
        (CheckResult::Current { latest: github }, CheckResult::Current { latest: gitee }) => {
            Ok(CheckResult::Current {
                latest: if parse_version(&gitee)? > parse_version(&github)? {
                    gitee
                } else {
                    github
                },
            })
        }
    }
}

fn evaluate_release(
    current: &str,
    release: ApiRelease,
    source: Source,
) -> Result<CheckResult, String> {
    let current = parse_version(current)?;
    let latest = parse_version(&release.tag_name)?;
    let display_version = latest.to_string();
    if latest <= current {
        return Ok(CheckResult::Current {
            latest: display_version,
        });
    }
    let asset = select_windows_asset(&release.assets)
        .ok_or_else(|| format!("{} 未包含 TimeHub Windows 程序", release.tag_name))?;
    let (github_download, gitee_download) = match source {
        Source::Github => (asset.browser_download_url.clone(), String::new()),
        Source::Gitee => (String::new(), asset.browser_download_url.clone()),
    };
    Ok(CheckResult::Available(UpdateInfo {
        version: display_version,
        notes: compact_notes(&release.body),
        github_download,
        gitee_download,
    }))
}

fn parse_version(value: &str) -> Result<Version, String> {
    Version::parse(value.trim().trim_start_matches(['v', 'V']))
        .map_err(|error| format!("无法解析版本 {value}: {error}"))
}

fn select_windows_asset(assets: &[ApiAsset]) -> Option<&ApiAsset> {
    assets
        .iter()
        .find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            (name.starts_with("timehub-setup-") || name == "timehub.exe") && name.ends_with(".exe")
        })
        .or_else(|| {
            assets
                .iter()
                .find(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
        })
}

fn compact_notes(notes: &str) -> String {
    let text = notes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| !line.starts_with("SHA-256") && !line.starts_with("安装包"))
        .map(|line| {
            line.strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line)
        })
        .take(3)
        .map(|line| format!("• {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    if text.chars().count() <= 220 {
        text
    } else {
        format!("{}...", text.chars().take(217).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: &[(&str, &str)]) -> ApiRelease {
        ApiRelease {
            tag_name: tag.into(),
            body: "- 修复提醒\n- 改进日历\n- 优化同步\n- 不显示第四项".into(),
            assets: assets
                .iter()
                .map(|(name, url)| ApiAsset {
                    name: (*name).into(),
                    browser_download_url: (*url).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn versions_are_compared_semantically() {
        assert!(matches!(
            evaluate_release("0.1.9", release("v0.1.10", &[("TimeHub.exe", "new")]), Source::Github).unwrap(),
            CheckResult::Available(info) if info.version == "0.1.10"
        ));
        assert!(matches!(
            evaluate_release("0.2.0", release("v0.1.10", &[]), Source::Github).unwrap(),
            CheckResult::Current { .. }
        ));
    }

    #[test]
    fn equal_versions_combine_confirmed_mirrors() {
        let github = evaluate_release(
            "0.1.0",
            release("v0.2.0", &[("TimeHub.exe", "github")]),
            Source::Github,
        );
        let gitee = evaluate_release(
            "0.1.0",
            release("v0.2.0", &[("TimeHub.exe", "gitee")]),
            Source::Gitee,
        );
        let CheckResult::Available(info) = merge_results(github, gitee).unwrap() else {
            panic!("expected update");
        };
        assert_eq!(info.github_download, "github");
        assert_eq!(info.gitee_download, "gitee");
    }

    #[test]
    fn newer_mirror_wins_and_one_failed_mirror_is_tolerated() {
        let old = evaluate_release(
            "0.1.0",
            release("v0.2.0", &[("TimeHub.exe", "github")]),
            Source::Github,
        );
        let new = evaluate_release(
            "0.1.0",
            release("v0.3.0", &[("TimeHub.exe", "gitee")]),
            Source::Gitee,
        );
        assert!(
            matches!(merge_results(old, new).unwrap(), CheckResult::Available(info) if info.version == "0.3.0")
        );
        assert!(matches!(
            merge_results(
                Err("offline".into()),
                Ok(CheckResult::Current {
                    latest: "0.1.0".into()
                })
            )
            .unwrap(),
            CheckResult::Current { .. }
        ));
    }

    #[test]
    fn windows_asset_and_release_notes_are_filtered() {
        let info = evaluate_release(
            "0.1.0",
            release("v0.2.0", &[("notes.txt", "bad"), ("TimeHub.exe", "good")]),
            Source::Github,
        )
        .unwrap();
        let CheckResult::Available(info) = info else {
            panic!("expected update")
        };
        assert_eq!(info.github_download, "good");
        assert_eq!(info.notes.lines().count(), 3);
    }

    #[test]
    fn mirror_urls_follow_the_release_asset_convention() {
        assert_eq!(
            expected_download(Source::Github, "0.2.0"),
            "https://github.com/mycode2025-ui/rili/releases/download/v0.2.0/TimeHub.exe"
        );
        assert_eq!(
            expected_download(Source::Gitee, "0.2.0"),
            "https://gitee.com/mycode2025-ui/rili/releases/download/v0.2.0/TimeHub.exe"
        );
    }

    #[test]
    #[ignore = "requires public GitHub/Gitee release APIs"]
    fn live_release_check_handles_the_current_public_state() {
        let result = check(env!("CARGO_PKG_VERSION")).expect("public update check");
        assert!(matches!(
            result,
            CheckResult::Current { .. } | CheckResult::Available(_)
        ));
    }
}
