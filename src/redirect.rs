use std::fmt;

use ::Url;

/// A type that controls the policy on how to handle the following of redirects.
///
/// The default value will catch redirect loops, and has a maximum of 10
/// redirects it will follow in a chain before returning an error.
#[derive(Debug)]
pub struct RedirectPolicy {
    inner: Policy,
}

#[derive(Debug)]
pub struct RedirectAttempt<'a> {
    next: &'a Url,
    previous: &'a [Url],
}

/// An action to perform when a redirect status code is found.
#[derive(Debug)]
pub struct RedirectAction {
    inner: Action,
}

impl RedirectPolicy {
    /// Create a RedirectPolicy with a maximum number of redirects.
    ///
    /// A `Error::TooManyRedirects` will be returned if the max is reached.
    pub fn limited(max: usize) -> RedirectPolicy {
        RedirectPolicy {
            inner: Policy::Limit(max),
        }
    }

    /// Create a RedirectPolicy that does not follow any redirect.
    pub fn none() -> RedirectPolicy {
        RedirectPolicy {
            inner: Policy::None,
        }
    }

    /// Create a custom RedirectPolicy using the passed function.
    ///
    /// # Note
    ///
    /// The default RedirectPolicy handles redirect loops and a maximum loop
    /// chain, but the custom variant does not do that for you automatically.
    /// The custom policy should have some way of handling those.
    ///
    /// There are variants on `::Error` for both cases that can be used as
    /// return values.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use reqwest::RedirectPolicy;
    /// # let mut client = reqwest::Client::new().unwrap();
    /// client.redirect(RedirectPolicy::custom(|next, previous| {
    ///     if previous.len() > 5 {
    ///         Err(reqwest::Error::TooManyRedirects)
    ///     } else if next.host_str() == Some("example.domain") {
    ///         // prevent redirects to 'example.domain'
    ///         Ok(false)
    ///     } else {
    ///         Ok(true)
    ///     }
    /// }));
    /// ```
    pub fn custom<T>(policy: T) -> RedirectPolicy
    where T: Fn(RedirectAttempt) -> RedirectAction + Send + Sync + 'static {
        RedirectPolicy {
            inner: Policy::Custom(Box::new(policy)),
        }
    }

    fn redirect(&self, attempt: RedirectAttempt) -> RedirectAction {
        match self.inner {
            Policy::Custom(ref custom) => custom(attempt),
            Policy::Limit(max) => {
                if attempt.previous.len() == max {
                    attempt.too_many_redirects()
                } else if attempt.previous.contains(attempt.next) {
                    attempt.loop_detected()
                } else {
                    attempt.follow()
                }
            },
            Policy::None => attempt.stop(),
        }
    }
}

impl Default for RedirectPolicy {
    fn default() -> RedirectPolicy {
        RedirectPolicy::limited(10)
    }
}

impl<'a> RedirectAttempt<'a> {
    pub fn url(&self) -> &Url {
        self.next
    }

    pub fn follow(self) -> RedirectAction {
        RedirectAction {
            inner: Action::Follow,
        }
    }

    pub fn stop(self) -> RedirectAction {
        RedirectAction {
            inner: Action::Stop,
        }
    }
    pub fn loop_detected(self) -> RedirectAction {
        RedirectAction {
            inner: Action::LoopDetected,
        }
    }

    pub fn too_many_redirects(self) -> RedirectAction {
        RedirectAction {
            inner: Action::TooManyRedirects,
        }
    }
}

enum Policy {
    Custom(Box<Fn(RedirectAttempt) -> RedirectAction + Send + Sync + 'static>),
    Limit(usize),
    None,
}

impl fmt::Debug for Policy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Policy::Custom(..) => f.pad("Custom"),
            Policy::Limit(max) => f.debug_tuple("Limit").field(&max).finish(),
            Policy::None => f.pad("None"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Follow,
    Stop,
    LoopDetected,
    TooManyRedirects,
}

#[inline]
pub fn check_redirect(policy: &RedirectPolicy, next: &Url, previous: &[Url]) -> Action {
    policy.redirect(RedirectAttempt {
        next: next,
        previous: previous,
    }).inner
}

/*
This was the desired way of doing it, but ran in to inference issues when
using closures, since the arguments received are references (&Url and &[Url]),
and the compiler could not infer the lifetimes of those references. That means
people would need to annotate the closure's argument types, which is garbase.

pub trait Redirect {
    fn redirect(&self, next: &Url, previous: &[Url]) -> ::Result<bool>;
}

impl<F> Redirect for F
where F: Fn(&Url, &[Url]) -> ::Result<bool> {
    fn redirect(&self, next: &Url, previous: &[Url]) -> ::Result<bool> {
        self(next, previous)
    }
}
*/

#[test]
fn test_redirect_policy_limit() {
    let policy = RedirectPolicy::default();
    let next = Url::parse("http://x.y/z").unwrap();
    let mut previous = (0..9)
        .map(|i| Url::parse(&format!("http://a.b/c/{}", i)).unwrap())
        .collect::<Vec<_>>();


    match policy.redirect(&next, &previous) {
        Ok(true) => {},
        other => panic!("expected Ok(true), got: {:?}", other)
    }

    previous.push(Url::parse("http://a.b.d/e/33").unwrap());

    match policy.redirect(&next, &previous) {
        Err(::Error::TooManyRedirects) => {},
        other => panic!("expected TooManyRedirects, got: {:?}", other)
    }
}

#[test]
fn test_redirect_policy_custom() {
    let policy = RedirectPolicy::custom(|attempt| {
        if attempt.url().host_str() == Some("foo") {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });

    let next = Url::parse("http://bar/baz").unwrap();
    assert_eq!(policy.redirect(&next, &[]).inner, Action::Follow);

    let next = Url::parse("http://foo/baz").unwrap();
    assert_eq!(policy.redirect(&next, &[]).inner, Action::Stop);
}
