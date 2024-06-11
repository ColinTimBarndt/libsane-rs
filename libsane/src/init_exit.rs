use core::fmt;
use std::{
    cell::UnsafeCell,
    error::Error as StdError,
    marker::PhantomData,
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{error, slice_util::slice_as_maybe_uninit, sys, Error, Sane, SaneStr, Version};

static HAS_INSTANCE: AtomicBool = AtomicBool::new(false);
static STATIC_SYNC_DATA: StaticSyncData = StaticSyncData {
    auth_handler: UnsafeCell::new(None),
};

/// Static data that must only be accessed synchronously, which can be achieved by
/// requiring a [`Sane`] reference or by being called from the sane library itself.
struct StaticSyncData {
    /// If the callback is !Send, it must only be accessed from the same thread.
    auth_handler: UnsafeCell<Option<Box<dyn AuthorizationCallback>>>,
}

/// SAFETY: Sane stores the type of the FnMut. If that is !Send, then Sane
/// cannot be passed between threads and StaticData is only ever accessed from
/// the same location.
/// SAFETY: Static Data is never accessed from multiple threads at
/// once.
unsafe impl Sync for StaticSyncData {}

/// Provided by [`AuthorizationCallback`].
/// The requested credentials need to be provided through this struct.
pub struct Authorizer<'a> {
    username: &'a mut [MaybeUninit<u8>; sys::MAX_USERNAME_LEN as usize],
    password: &'a mut [MaybeUninit<u8>; sys::MAX_PASSWORD_LEN as usize],
}

impl Authorizer<'_> {
    pub const fn max_username_len(&self) -> usize {
        self.username.len() - 1 // -1 for NUL byte
    }

    pub const fn max_password_len(&self) -> usize {
        self.password.len() - 1 // -1 for NUL byte
    }

    pub fn provide_credentials(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<AuthOk, AuthError> {
        Self::write_str(self.username, username).map_err(AuthError::Username)?;
        Self::write_str(self.password, password).map_err(AuthError::Password)?;
        Ok(AuthOk(()))
    }

    pub fn provide_credentials_latin1(
        &mut self,
        username: &SaneStr,
        password: &SaneStr,
    ) -> Result<AuthOk, AuthError> {
        Self::write_str_latin1(self.username, username).map_err(AuthError::Username)?;
        Self::write_str_latin1(self.password, password).map_err(AuthError::Password)?;
        Ok(AuthOk(()))
    }

    fn write_str(target: &mut [MaybeUninit<u8>], source: &str) -> Result<(), AuthFieldError> {
        let mut target_iter = target.iter_mut();
        for (dest, ch) in (&mut target_iter).zip(source.chars()) {
            let latin1: u8 = ch.try_into().map_err(|_| AuthFieldError::NotLatin1)?;
            *dest = MaybeUninit::new(latin1);
        }
        let Some(nul) = target_iter.next() else {
            return Err(AuthFieldError::TooLong);
        };
        *nul = MaybeUninit::new(0);
        Ok(())
    }

    fn write_str_latin1(
        target: &mut [MaybeUninit<u8>],
        source: &SaneStr,
    ) -> Result<(), AuthFieldError> {
        let bytes = source.to_bytes_with_nul();
        if bytes.len() > target.len() {
            return Err(AuthFieldError::TooLong);
        }
        target[..bytes.len()].copy_from_slice(slice_as_maybe_uninit(bytes));
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum AuthFieldError {
    NotLatin1,
    TooLong,
}

impl fmt::Display for AuthFieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::NotLatin1 => "field contains non-Latin1 characters",
            Self::TooLong => "field is too long",
        };
        f.write_str(msg)
    }
}

impl StdError for AuthFieldError {}

#[derive(Debug, Clone)]
pub enum AuthError {
    Username(AuthFieldError),
    Password(AuthFieldError),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (msg, field) = match self {
            Self::Username(field) => ("username ", field),
            Self::Password(field) => ("password ", field),
        };
        f.write_str(msg)?;
        fmt::Display::fmt(field, f)
    }
}

impl StdError for AuthError {}

/// This struct is used as a token to ensure that all credentials were
/// successfully written to the [`Authorizer`].
pub struct AuthOk(());

/// A callback used when Sane needs to authenticate to access the given `resource` name.
/// Authorization credentials need to be provided.
pub trait AuthorizationCallback {
    /// An authorization request to `resource` was made.
    /// The `authorizer` needs to be provided with credentials.
    fn authorize(&mut self, resource: &SaneStr, authorizer: Authorizer) -> AuthOk;
}

impl<T> AuthorizationCallback for T
where
    T: FnMut(&SaneStr, Authorizer) -> AuthOk,
{
    fn authorize(&mut self, resource: &SaneStr, authorizer: Authorizer) -> AuthOk {
        self(resource, authorizer)
    }
}

impl<A> Drop for Sane<A> {
    fn drop(&mut self) {
        // SAFETY: All calls and data reads happen with a reference to Sane. Therefore,
        // there are no more references and no more api calls will happen after this.
        // (unless Sane is initialized again, which is okay by spec)
        unsafe { sys::sane_exit() };
        HAS_INSTANCE.store(false, Ordering::Release);
    }
}

impl<A> Sane<A> {
    pub fn init(authorize: Option<Box<A>>) -> Result<(Self, Version), Error>
    where
        A: AuthorizationCallback + 'static,
    {
        let has_instance = HAS_INSTANCE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err();

        assert!(!has_instance, "Sane has already been initialized once");

        if let Some(authorize) = authorize {
            // SAFETY: Only written to directly before sane_init, and locked
            //         by HAS_INSTANCE, therefore no other accesses.
            let ah = unsafe { &mut *STATIC_SYNC_DATA.auth_handler.get() };
            *ah = Some(authorize);
        }

        let mut version = Version::new(0, 0, 0);
        // SAFETY: There is no other instance of Sane, therefore libsane can be initialized.
        let result = error::status_result(unsafe {
            sys::sane_init(version.as_mut(), Some(authorize_callback))
        })
        .map(|()| {
            (
                Sane {
                    _phant: PhantomData,
                },
                version,
            )
        });

        if result.is_err() {
            // SAFETY: HAS_INSTANCE is still locked, therefore no other access possible except here as the
            // authorize_callback may only be called when sane is initalized.
            let ah = unsafe { &mut *STATIC_SYNC_DATA.auth_handler.get() };
            *ah = None;
            HAS_INSTANCE.store(false, Ordering::Release);
        }

        result
    }
}

unsafe extern "C" fn authorize_callback(
    resource: sys::StringConst,
    username: *mut sys::Char,
    password: *mut sys::Char,
) {
    // SAFETY: Only ever called after Sane was initialized and before it was exited
    //         Will be called from the same thread where init was called if
    //         A is !Send (as Sane is !Send in this case)
    let Some(cb) = (unsafe { &mut *STATIC_SYNC_DATA.auth_handler.get() }) else {
        // SAFETY: Both are valid uninitialized allocations for C-Strings.
        // also this code should be unreachable anyways because this callback is only called when auth_handler was provided.
        unsafe {
            username.write(0);
            password.write(0);
        };
        return;
    };

    let resource = SaneStr::from_ptr(resource);

    let username = (username as *mut [MaybeUninit<u8>; sys::MAX_USERNAME_LEN as usize])
        .as_mut()
        .unwrap();
    let password = (password as *mut [MaybeUninit<u8>; sys::MAX_PASSWORD_LEN as usize])
        .as_mut()
        .unwrap();

    let AuthOk(..) = cb.authorize(resource, Authorizer { username, password });
}

/// An implementor of [`AuthorizationCallback`] that can never be created.
/// You can use this type if you are passing `None` as the authorization handler.
#[derive(Debug, Clone, Copy)]
pub enum NoAuth {}

impl AuthorizationCallback for NoAuth {
    fn authorize(&mut self, _resource: &SaneStr, _authorizer: Authorizer) -> AuthOk {
        match *self {}
    }
}

impl Sane<NoAuth> {
    #[inline]
    pub fn init_no_auth() -> Result<(Self, Version), Error> {
        Self::init(None)
    }
}
