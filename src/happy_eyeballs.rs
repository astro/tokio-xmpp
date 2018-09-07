use std::mem;
use std::net::SocketAddr;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::cell::RefCell;
use futures::{Future, Poll, Async};
use tokio::net::{ConnectFuture, TcpStream};
use trust_dns_resolver::{IntoName, Name, ResolverFuture, error::ResolveError};
use trust_dns_resolver::lookup::SrvLookupFuture;
use trust_dns_resolver::lookup_ip::LookupIpFuture;
use {Error, ConnecterError};

enum State {
    AwaitResolver(Box<Future<Item = ResolverFuture, Error = ResolveError> + Send>),
    ResolveSrv(ResolverFuture, SrvLookupFuture),
    ResolveTarget(ResolverFuture, LookupIpFuture, u16),
    Connecting(Option<ResolverFuture>, Vec<RefCell<ConnectFuture>>),
    Invalid,
}

pub struct Connecter {
    fallback_port: u16,
    srv_domain: Option<Name>,
    domain: Name,
    state: State,
    targets: VecDeque<(Name, u16)>,
    error: Option<Error>,
}

impl Connecter {
    pub fn from_lookup(domain: &str, srv: Option<&str>, fallback_port: u16) -> Result<Connecter, ConnecterError> {
        if let Ok(ip) = domain.parse() {
            // use specified IP address, not domain name, skip the whole dns part
            let connect =
                RefCell::new(TcpStream::connect(&SocketAddr::new(ip, fallback_port)));
            return Ok(Connecter {
                fallback_port,
                srv_domain: None,
                domain: "nohost".into_name()?,
                state: State::Connecting(None, vec![connect]),
                targets: VecDeque::new(),
                error: None,
            });
        }

        let resolver_future = ResolverFuture::from_system_conf()?;
        let state = State::AwaitResolver(resolver_future);
        let srv_domain = match srv {
            Some(srv) =>
                Some(format!("{}.{}.", srv, domain).into_name()?),
            None =>
                None,
        };

        Ok(Connecter {
            fallback_port,
            srv_domain,
            domain: domain.into_name()?,
            state,
            targets: VecDeque::new(),
            error: None,
        })
    }
}

impl Future for Connecter {
    type Item = TcpStream;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let state = mem::replace(&mut self.state, State::Invalid);
        match state {
            State::AwaitResolver(mut resolver_future) => {
                match resolver_future.poll().map_err(ConnecterError::Resolve)? {
                    Async::NotReady => {
                        self.state = State::AwaitResolver(resolver_future);
                        Ok(Async::NotReady)
                    }
                    Async::Ready(resolver) => {
                        match &self.srv_domain {
                            &Some(ref srv_domain) => {
                                let srv_lookup = resolver.lookup_srv(srv_domain);
                                self.state = State::ResolveSrv(resolver, srv_lookup);
                            }
                            None => {
                                self.targets =
                                    [(self.domain.clone(), self.fallback_port)].into_iter()
                                    .cloned()
                                    .collect();
                                self.state = State::Connecting(Some(resolver), vec![]);
                            }
                        }
                        self.poll()
                    }
                }
            }
            State::ResolveSrv(resolver, mut srv_lookup) => {
                match srv_lookup.poll() {
                    Ok(Async::NotReady) => {
                        self.state = State::ResolveSrv(resolver, srv_lookup);
                        Ok(Async::NotReady)
                    }
                    Ok(Async::Ready(srv_result)) => {
                        let mut srv_map: BTreeMap<_, _> =
                            srv_result.iter()
                            .map(|srv| (srv.priority(), (srv.target().clone(), srv.port())))
                            .collect();
                        let targets =
                            srv_map.into_iter()
                            .map(|(_, tp)| tp)
                            .collect();
                        self.targets = targets;
                        self.state = State::Connecting(Some(resolver), vec![]);
                        self.poll()
                    }
                    Err(_) => {
                        // ignore, fallback
                        self.targets =
                            [(self.domain.clone(), self.fallback_port)].into_iter()
                            .cloned()
                            .collect();
                        self.state = State::Connecting(Some(resolver), vec![]);
                        self.poll()
                    }
                }
            }
            State::Connecting(resolver, mut connects) => {
                if resolver.is_some() && connects.len() == 0 && self.targets.len() > 0 {
                    let resolver = resolver.unwrap();
                    let (host, port) = self.targets.pop_front().unwrap();
                    let ip_lookup = resolver.lookup_ip(host);
                    self.state = State::ResolveTarget(resolver, ip_lookup, port);
                    self.poll()
                } else if connects.len() > 0 {
                    let mut success = None;
                    connects.retain(|connect| {
                        match connect.borrow_mut().poll() {
                            Ok(Async::NotReady) => true,
                            Ok(Async::Ready(connection)) => {
                                success = Some(connection);
                                false
                            }
                            Err(e) => {
                                if self.error.is_none() {
                                    self.error = Some(e.into());
                                }
                                false
                            },
                        }
                    });
                    match success {
                        Some(connection) =>
                            Ok(Async::Ready(connection)),
                        None => {
                            self.state = State::Connecting(resolver, connects);
                            Ok(Async::NotReady)
                        },
                    }
                } else {
                    // All targets tried
                    match self.error.take() {
                        None =>
                            Err(ConnecterError::AllFailed.into()),
                        Some(e) =>
                            Err(e),
                    }
                }
            }
            State::ResolveTarget(resolver, mut ip_lookup, port) => {
                match ip_lookup.poll() {
                    Ok(Async::NotReady) => {
                        self.state = State::ResolveTarget(resolver, ip_lookup, port);
                        Ok(Async::NotReady)
                    }
                    Ok(Async::Ready(ip_result)) => {
                        let connects =
                            ip_result.iter()
                            .map(|ip| RefCell::new(TcpStream::connect(&SocketAddr::new(ip, port))))
                            .collect();
                        self.state = State::Connecting(Some(resolver), connects);
                        self.poll()
                    }
                    Err(e) => {
                        if self.error.is_none() {
                            self.error = Some(ConnecterError::Resolve(e).into());
                        }
                        // ignore, next…
                        self.state = State::Connecting(Some(resolver), vec![]);
                        self.poll()
                    }
                }
            }
            _ => panic!("")
        }
    }
}

