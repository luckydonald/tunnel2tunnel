use std::borrow::Cow;

pub fn init_sentry() -> Option<sentry::ClientInitGuard> {
    let dsn = std::env::var("SENTRY_DSN").unwrap_or_default();
    if dsn.is_empty() {
        return None;
    }

    let guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            environment: std::env::var("SENTRY_ENVIRONMENT").ok().map(Into::into),
            release: std::env::var("SENTRY_RELEASE")
                .ok()
                .filter(|s| !s.is_empty())
                .map(Into::into)
                .or_else(release_tag),
            send_default_pii: false,
            traces_sample_rate: parse_sample_rate(
                &std::env::var("SENTRY_TRACES_SAMPLE_RATE").unwrap_or_default(),
            ),
            ..Default::default()
        },
    ));

    sentry::configure_scope(|scope| {
        if let Some(branch) = git_branch() {
            scope.set_tag("git_branch", branch);
        }
        if let Some(build_time) = build_time() {
            scope.set_tag("build_time", build_time);
        }
    });

    Some(guard)
}

fn release_tag() -> Option<Cow<'static, str>> {
    std::env::var("SOURCE_COMMIT")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| run("git", &["rev-parse", "HEAD"]))
        .map(|s| s.trim().to_string().into())
}

fn git_branch() -> Option<String> {
    std::env::var("GIT_BRANCH")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| run("git", &["rev-parse", "--abbrev-ref", "HEAD"]))
}

fn build_time() -> Option<String> {
    std::env::var("BUILD_TIME").ok().filter(|s| !s.is_empty())
}

fn parse_sample_rate(raw: &str) -> f32 {
    raw.parse().unwrap_or(0.0)
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}
