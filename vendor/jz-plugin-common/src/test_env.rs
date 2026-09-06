use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn with_home<F, T>(token: &str, f: F) -> T
where
    F: FnOnce(&tempfile::TempDir, mockito::ServerGuard) -> T,
{
    let _guard = lock();
    crate::auth::clear_exchange_cache_for_tests();
    let tmp = tempfile::tempdir().expect("tempdir");
    std::env::set_var(crate::home::PLATFORM_HOME_ENV, tmp.path());
    crate::home::write_durable_token(token).expect("write token");
    let server = mockito::Server::new();
    f(&tmp, server)
}
